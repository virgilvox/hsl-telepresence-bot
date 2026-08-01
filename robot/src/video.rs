//! Video plane. Captures the stereoscopic USB camera once and serves it two
//! ways.
//!
//! # Two paths, because they are wanted for different things
//!
//! A **peer connection** is the low-latency path, about a tenth of a second,
//! and it is what teleoperation needs to feel like driving rather than like
//! issuing instructions. Every peer costs the robot a payloader, DTLS/SRTP, and
//! another full copy of the bitrate out of one Pi on shared WiFi, so there are
//! only ever a handful ([`MAX_PEERS`]) and they go to whoever is driving.
//!
//! The **broadcast** is the other path: the encoded stream published once to a
//! CLASP address and fanned out by the relay (see `crate::broadcast`). It costs
//! the robot exactly the same whether one person is watching or fifty, so the
//! size of the audience stops being the robot's problem at all. It also never
//! touches this pipeline, which matters more than it sounds: arriving to watch
//! cannot interrupt anyone already watching.
//!
//! So the limit that used to be on viewers is now only on peers, and the number
//! of people who can watch is bounded by the relay rather than by the Pi.
//!
//! CLASP still carries no peer media: presence and SDP/ICE only. The broadcast
//! is a deliberate exception, and it is encoded video on a Stream rather than a
//! media transport pretending to be one.
//!
//! # Pipeline
//!
//! v4l2src (MJPEG) -> jpegdec -> H264 (hardware v4l2h264enc) -> h264parse ->
//! tee. One branch parses to byte-stream and lands in an appsink for the
//! broadcast; the other payloads to RTP once and fans that out to a webrtcbin
//! per peer. Capture and encode happen once no matter who is watching or how.
//!
//! # Why the pipeline is rebuilt when a peer joins
//!
//! Adding a branch to a live `tee` means requesting pads and blocking probes on
//! a running pipeline, and getting that subtly wrong deadlocks the streaming
//! thread, which takes the camera down for everyone until the process is
//! restarted. Rebuilding instead is a few hundred milliseconds of black for the
//! existing viewers and cannot wedge: every element is torn down and recreated
//! in a known order. It also solves the keyframe problem for free, because a
//! fresh encoder emits an IDR immediately and the new viewer gets a picture
//! without waiting for the next one.
//!
//! Departures do *not* rebuild. A peer that leaves keeps a dead branch until
//! the next join, which costs nothing but a payloader, and each branch queue is
//! `leaky=downstream` so a stalled or dead peer drops buffers instead of
//! blocking the tee and freezing everybody else. Broadcast watchers never
//! rebuild anything in either direction.
//!
//! The pipeline bus and each WebRTC connection state are watched: a camera
//! drop, encoder fault, or peer disconnect tears the affected thing down so it
//! restarts cleanly, rather than dying silently.
//!
//! This module builds and runs on a host with GStreamer 1.20+ and the good,
//! bad, and nice plugin sets installed.

use crate::config::Config;
use crate::protocol::{Addresses, SignalMessage, VideoEvent};
use clasp_client::prelude::Value;
use clasp_client::Clasp;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_sdp as gst_sdp;
use gstreamer_webrtc as gst_webrtc;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

/// One encoded access unit on its way from the pipeline to the relay.
struct EncodedFrame {
    seq: u32,
    keyframe: bool,
    data: Vec<u8>,
}

/// Frames held between the streaming thread and the publishing task.
///
/// Small on purpose. The broadcast is best-effort by contract, so a relay or an
/// uplink we cannot keep up with has to cost frames. A deep queue would instead
/// spend memory to deliver video that is already too old to be worth watching.
const FRAME_QUEUE: usize = 8;

const STUN_SERVER: &str = "stun://stun.l.google.com:19302";

/// Upper bound on simultaneous *peer* connections, which is not a limit on the
/// audience.
///
/// Encoding is shared, but each extra peer still costs a payloader, DTLS/SRTP,
/// and its own copy of the bitrate out of one Pi 3B+ on shared WiFi, so this
/// number is about what the robot can push, and it is deliberately small.
/// Everyone else watches the CLASP broadcast instead, which the relay fans out,
/// so the number of people watching is bounded by the relay rather than by the
/// robot. A peer is for whoever is driving and actually needs the latency.
const MAX_PEERS: usize = 4;

/// How long a viewer survives without saying hello. Consoles heartbeat every
/// few seconds, so this drops a closed tab or a dead network well before a
/// human notices, without evicting someone on a brief stall.
const VIEWER_TIMEOUT: Duration = Duration::from_secs(20);

/// Joins inside this window are collected into a single rebuild, so a group
/// arriving together costs one interruption rather than one each.
const JOIN_DEBOUNCE: Duration = Duration::from_millis(400);

/// How long to wait before trying again after a failed build. Usually the
/// camera is missing or still re-enumerating; retrying in a tight loop just
/// fills the journal.
const RETRY_DELAY: Duration = Duration::from_secs(3);

/// How often the loop expires stale viewers and acts on a pending rebuild.
const SWEEP: Duration = Duration::from_millis(250);

/// Rate limit on the "too many viewers" warning. A viewer that cannot get in
/// keeps announcing itself, and one line every heartbeat would bury the log.
const FULL_WARNING_EVERY: Duration = Duration::from_secs(30);

/// A signaling message and the viewer it is meant for, tagged with the pipeline
/// generation that produced it so replies to a torn-down pipeline are dropped.
struct Outbound {
    generation: u64,
    to: String,
    message: SignalMessage,
}

/// Something died. Tagged with a generation for the same reason: tearing a
/// pipeline down makes its peers report Closed, and those reports must not
/// evict viewers from the pipeline that replaced it.
enum Failure {
    /// The shared capture pipeline failed; nobody has video.
    Pipeline { generation: u64 },
    /// One peer connection dropped; that viewer is gone.
    Viewer { generation: u64, viewer: String },
}

pub fn spawn(
    client: Arc<Clasp>,
    addr: Addresses,
    cfg: Config,
    mut rx: UnboundedReceiver<VideoEvent>,
) {
    tokio::spawn(async move {
        if let Err(err) = gst::init() {
            tracing::error!(%err, "failed to initialize GStreamer; video disabled");
            return;
        }

        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Outbound>();
        let (fail_tx, mut fail_rx) = mpsc::unbounded_channel::<Failure>();
        let (frames_tx, mut frames_rx) = mpsc::channel::<EncodedFrame>(FRAME_QUEUE);
        // Kept across rebuilds so the sequence a browser reassembles by never
        // jumps backwards when the pipeline is replaced under it.
        let frame_seq = Arc::new(AtomicU32::new(0));

        // Operators on a WebRTC track of their own, and when each was last
        // heard from. Ordered so the pipeline description is stable for a given
        // set. Only this set can force a rebuild.
        let mut peers: BTreeMap<String, Instant> = BTreeMap::new();
        // Everyone watching the CLASP broadcast. They cost the robot nothing
        // per head and never touch the pipeline, which is exactly why the
        // audience can be any size.
        let mut watchers: BTreeMap<String, Instant> = BTreeMap::new();
        let mut broadcast: Option<Broadcast> = None;
        let mut generation: u64 = 0;
        let mut rebuild_at: Option<Instant> = None;
        let mut published_count: Option<usize> = None;
        // A viewer that cannot get in keeps saying hello, so the "full" warning
        // is rate-limited rather than logged every heartbeat.
        let mut last_full_warning: Option<Instant> = None;

        let mut sweep = tokio::time::interval(SWEEP);
        // A rebuild parks this loop for a moment. Without this the interval
        // then fires once for every tick it missed, all at once.
        sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                event = rx.recv() => {
                    let Some(event) = event else { break };
                    match event {
                        VideoEvent::ViewerPresent(presence) => {
                            if presence.session.is_empty() {
                                continue;
                            }
                            let now = Instant::now();

                            if !presence.wants_peer() {
                                // A broadcast watcher. It cannot be refused,
                                // because serving it costs the robot nothing:
                                // the relay does the fanning out.
                                let known = watchers.insert(presence.session.clone(), now).is_some();
                                // Somebody switching to the broadcast should
                                // stop being served a track they left behind.
                                if peers.remove(&presence.session).is_some() {
                                    if let Some(b) = broadcast.as_mut() {
                                        b.forget(&presence.session);
                                    }
                                }
                                if !known {
                                    tracing::info!(
                                        viewer = %presence.session,
                                        watching = peers.len() + watchers.len(),
                                        "watcher joined the broadcast"
                                    );
                                }
                                continue;
                            }

                            let known = peers.contains_key(&presence.session);
                            if !known && peers.len() >= MAX_PEERS {
                                let due = last_full_warning
                                    .map(|t| now.saturating_duration_since(t) >= FULL_WARNING_EVERY)
                                    .unwrap_or(true);
                                if due {
                                    last_full_warning = Some(now);
                                    tracing::warn!(
                                        viewer = %presence.session,
                                        max = MAX_PEERS,
                                        "peer slots full; this viewer must watch the broadcast"
                                    );
                                }
                                // Not turned away, just not given a track. It
                                // still counts as present so the camera keeps
                                // running for it, and its console falls back to
                                // the broadcast when no offer arrives.
                                watchers.insert(presence.session.clone(), now);
                                continue;
                            }
                            watchers.remove(&presence.session);
                            peers.insert(presence.session.clone(), now);
                            if !known {
                                tracing::info!(
                                    viewer = %presence.session,
                                    watching = peers.len() + watchers.len(),
                                    "viewer joined on a peer connection"
                                );
                            }
                        }
                        VideoEvent::Signal(message) => {
                            if let SignalMessage::Bye { from } = &message {
                                let left = peers.remove(from).is_some();
                                let watched = watchers.remove(from).is_some();
                                if left {
                                    if let Some(b) = broadcast.as_mut() {
                                        b.forget(from);
                                    }
                                }
                                if left || watched {
                                    tracing::info!(
                                        viewer = %from,
                                        watching = peers.len() + watchers.len(),
                                        "viewer said goodbye"
                                    );
                                }
                                continue;
                            }
                            if let Some(b) = broadcast.as_ref() {
                                b.handle_signal(message);
                            }
                        }
                    }
                }
                outbound = out_rx.recv() => {
                    let Some(outbound) = outbound else { continue };
                    if outbound.generation != generation {
                        continue; // produced by a pipeline we have since replaced
                    }
                    // Stamped here, not where the message was built: the relay
                    // hands out a new session on every reconnect, and a peer
                    // that replies to a stale one is talking to nobody.
                    let mut message = outbound.message;
                    message.set_from(client.session_id().unwrap_or_default());
                    let payload = crate::link::to_value(
                        serde_json::to_value(&message).unwrap_or(serde_json::Value::Null),
                    );
                    if let Err(err) = client
                        .emit(addr.video_signal(&outbound.to).as_str(), payload)
                        .await
                    {
                        tracing::debug!(%err, "signal emit failed");
                    }
                }
                failed = fail_rx.recv() => {
                    let Some(failure) = failed else { continue };
                    match failure {
                        // A report from a pipeline we have already replaced.
                        // Tearing one down makes its peers report Closed, and
                        // those must not evict viewers from its replacement.
                        Failure::Pipeline { generation: g }
                        | Failure::Viewer { generation: g, .. }
                            if g != generation => {}
                        Failure::Pipeline { .. } => {
                            tracing::warn!("video pipeline failed; tearing down, will rebuild");
                            if let Some(old) = broadcast.take() {
                                old.stop();
                            }
                            rebuild_at = Some(Instant::now() + RETRY_DELAY);
                        }
                        Failure::Viewer { viewer, .. } => {
                            // Only the track is gone. The person may well still
                            // be there on the broadcast, so their presence
                            // heartbeat decides that, not this.
                            if peers.remove(&viewer).is_some() {
                                if let Some(b) = broadcast.as_mut() {
                                    b.forget(&viewer);
                                }
                                tracing::info!(
                                    %viewer,
                                    watching = peers.len() + watchers.len(),
                                    "viewer connection lost"
                                );
                            }
                        }
                    }
                }
                _ = sweep.tick() => {
                    let now = Instant::now();
                    let stale = |seen: &Instant| {
                        now.saturating_duration_since(*seen) >= VIEWER_TIMEOUT
                    };

                    let expired: Vec<String> = peers
                        .iter()
                        .filter(|(_, seen)| stale(seen))
                        .map(|(viewer, _)| viewer.clone())
                        .collect();
                    for viewer in expired {
                        peers.remove(&viewer);
                        if let Some(b) = broadcast.as_mut() {
                            b.forget(&viewer);
                        }
                        tracing::info!(
                            %viewer,
                            watching = peers.len() + watchers.len(),
                            "viewer timed out"
                        );
                    }
                    // Watchers leaving is bookkeeping. It frees no resource and
                    // must not disturb the pipeline, so it is silent.
                    watchers.retain(|_, seen| !stale(seen));

                    let audience = peers.len() + watchers.len();
                    if audience == 0 {
                        if let Some(old) = broadcast.take() {
                            tracing::info!("nobody watching; releasing the camera");
                            teardown(old).await;
                        }
                        rebuild_at = None;
                    } else if broadcast
                        .as_ref()
                        .map_or(true, |b| has_new_viewer(&b.viewers, &peers))
                    {
                        // Only a change in the *peer* set gets here, so an
                        // audience of any size can come and go without ever
                        // interrupting the people already watching.
                        let due = rebuild_at.get_or_insert(now + JOIN_DEBOUNCE);
                        if now >= *due {
                            rebuild_at = None;
                            generation += 1;
                            let old = broadcast.take();
                            let served: Vec<String> = peers.keys().cloned().collect();
                            match rebuild(
                                old,
                                &cfg,
                                served.clone(),
                                generation,
                                &out_tx,
                                &fail_tx,
                                &frames_tx,
                                &frame_seq,
                            )
                            .await
                            {
                                Ok(b) => {
                                    tracing::info!(
                                        peers = served.len(),
                                        watchers = watchers.len(),
                                        generation,
                                        "video pipeline running"
                                    );
                                    broadcast = Some(b);
                                }
                                Err(err) => {
                                    tracing::error!(%err, "failed to start video pipeline");
                                    rebuild_at = Some(Instant::now() + RETRY_DELAY);
                                }
                            }
                        }
                    } else {
                        rebuild_at = None;
                    }

                    // Mirror the audience size for the consoles. Published only
                    // on change: this is a Param, not a heartbeat.
                    if published_count != Some(audience) {
                        published_count = Some(audience);
                        let payload = crate::link::to_value(serde_json::json!(audience));
                        if let Err(err) = client.set(addr.status("viewers").as_str(), payload).await {
                            tracing::debug!(%err, "viewer count publish failed");
                        }
                    }
                }
                frame = frames_rx.recv() => {
                    let Some(frame) = frame else { continue };
                    // Encoding runs for the peers regardless, so with nobody on
                    // the broadcast the frames are dropped here rather than
                    // spending uplink on an audience of nobody.
                    if !watchers.is_empty() {
                        publish_frame(&client, &addr, frame).await;
                    }
                }
            }
        }

        if let Some(broadcast) = broadcast.take() {
            teardown(broadcast).await;
        }
    });
}

/// Replace the running pipeline with one built for `audience`, off the async
/// runtime.
///
/// Both halves of this block: tearing a pipeline down waits for it to reach
/// NULL so the camera file descriptor is really released, and opening a V4L2
/// device that has wedged (which this camera does, see the hardware notes) can
/// stall for a long time. Neither belongs on a runtime worker thread, because
/// the motion watchdog shares the same executor and a stuck camera must never
/// be able to delay coasting the motors.
///
/// The two are done together, in order, on one blocking thread: the old
/// pipeline must have released the device before the new one opens it, or the
/// new `v4l2src` gets EBUSY.
#[allow(clippy::too_many_arguments)]
async fn rebuild(
    old: Option<Broadcast>,
    cfg: &Config,
    served: Vec<String>,
    generation: u64,
    out_tx: &UnboundedSender<Outbound>,
    fail_tx: &UnboundedSender<Failure>,
    frames_tx: &mpsc::Sender<EncodedFrame>,
    frame_seq: &Arc<AtomicU32>,
) -> anyhow::Result<Broadcast> {
    let cfg = cfg.clone();
    let out_tx = out_tx.clone();
    let fail_tx = fail_tx.clone();
    let frames_tx = frames_tx.clone();
    let frame_seq = frame_seq.clone();
    tokio::task::spawn_blocking(move || {
        if let Some(old) = old {
            old.stop();
        }
        Broadcast::start(
            &cfg, &served, generation, &out_tx, &fail_tx, &frames_tx, &frame_seq,
        )
    })
    .await
    .map_err(|err| anyhow::anyhow!("pipeline build task failed: {err}"))?
}

/// Stop a pipeline off the async runtime, for the same reason as [`rebuild`].
async fn teardown(broadcast: Broadcast) {
    if let Err(err) = tokio::task::spawn_blocking(move || broadcast.stop()).await {
        tracing::warn!(%err, "pipeline teardown task failed");
    }
}

/// True when someone is watching who the running pipeline was not built for.
/// Departures alone never trigger a rebuild; see the module docs.
fn has_new_viewer(served: &[String], watching: &BTreeMap<String, Instant>) -> bool {
    watching.keys().any(|v| !served.iter().any(|s| s == v))
}

struct Broadcast {
    pipeline: gst::Pipeline,
    /// One webrtcbin per viewer session this pipeline was built for.
    peers: HashMap<String, gst::Element>,
    /// The viewer set this pipeline was built for, in the order its branches
    /// were created.
    viewers: Vec<String>,
    /// Signals the bus-watch thread to exit on teardown.
    shutdown: Arc<AtomicBool>,
}

impl Broadcast {
    fn start(
        cfg: &Config,
        viewers: &[String],
        generation: u64,
        out_tx: &UnboundedSender<Outbound>,
        fail_tx: &UnboundedSender<Failure>,
        frames_tx: &mpsc::Sender<EncodedFrame>,
        frame_seq: &Arc<AtomicU32>,
    ) -> anyhow::Result<Self> {
        // Resolve the camera fresh each time: the USB camera can re-enumerate
        // to a different /dev/videoN after a power glitch, so a fixed node goes
        // stale. See resolve_camera_device.
        let device = resolve_camera_device(&cfg.camera_device);

        // The Pi's VideoCore H264 encoder requires explicit output caps
        // (a level string), otherwise it fails to process frames. It also
        // maxes out at 1920 wide, so capture the camera's 1280x480 side-by-side
        // mode rather than its 2560-wide modes.
        //
        // The split happens on *encoded* H264, before RTP payloading, so the
        // broadcast branch gets access units it can put on the relay while the
        // peers get the RTP they need. `allow-not-linked` keeps a tee that has
        // no branches at the moment from erroring the pipeline out, which is
        // the normal state when everyone is watching the broadcast.
        let mut description = format!(
            "v4l2src device={device} ! image/jpeg,width={width},height={height},framerate={fps}/1 \
             ! jpegdec ! queue ! videoconvert ! video/x-raw,format=I420 \
             ! v4l2h264enc ! video/x-h264,level=(string)4 \
             ! h264parse config-interval=-1 \
             ! tee name=encoded allow-not-linked=true",
            device = device,
            width = cfg.camera_width,
            height = cfg.camera_height,
            fps = cfg.camera_fps,
        );

        // The broadcast tap. `config-interval=-1` repeats SPS/PPS ahead of
        // every keyframe, which is what lets somebody who arrives mid-stream
        // start decoding at the next one instead of never. byte-stream and
        // alignment=au hand the browser whole access units in the form
        // WebCodecs expects. The sink drops rather than blocks: the camera must
        // never wait on the network.
        description.push_str(
            " encoded. ! queue leaky=downstream max-size-buffers=0 max-size-bytes=0 \
             max-size-time=300000000 \
             ! h264parse config-interval=-1 \
             ! video/x-h264,stream-format=byte-stream,alignment=au \
             ! appsink name=broadcastsink sync=false max-buffers=4 drop=true",
        );

        // The peer chain exists only when somebody is actually on a track.
        // Payloading once and fanning the result out is what keeps a second
        // peer cheaper than the first.
        if !viewers.is_empty() {
            description.push_str(
                " encoded. ! queue ! rtph264pay pt=96 \
                 ! application/x-rtp,media=video,encoding-name=H264,payload=96 \
                 ! tee name=fanout allow-not-linked=true",
            );
        }
        for index in 0..viewers.len() {
            // leaky=downstream is what keeps one wedged peer from stalling the
            // tee, and with it the picture for everyone else. Bounded by time
            // rather than buffers so the limit means the same thing at any
            // bitrate.
            description.push_str(&format!(
                " fanout. ! queue name=branch{index} leaky=downstream \
                 max-size-buffers=0 max-size-bytes=0 max-size-time=300000000 \
                 ! webrtcbin name=peer{index} bundle-policy=max-bundle"
            ));
        }

        let pipeline = gst::parse::launch(&description)?
            .downcast::<gst::Pipeline>()
            .map_err(|_| anyhow::anyhow!("constructed element is not a pipeline"))?;

        let shutdown = Arc::new(AtomicBool::new(false));
        watch_bus(&pipeline, generation, shutdown.clone(), fail_tx.clone());
        attach_broadcast_sink(&pipeline, frames_tx.clone(), frame_seq.clone())?;

        let mut peers = HashMap::with_capacity(viewers.len());
        for (index, viewer) in viewers.iter().enumerate() {
            let name = format!("peer{index}");
            let webrtc = pipeline
                .by_name(&name)
                .ok_or_else(|| anyhow::anyhow!("pipeline has no webrtcbin named '{name}'"))?;
            webrtc.set_property_from_str("stun-server", STUN_SERVER);

            connect_peer(&webrtc, viewer, generation, out_tx, fail_tx);
            peers.insert(viewer.clone(), webrtc);
        }

        pipeline.set_state(gst::State::Playing)?;

        Ok(Self {
            pipeline,
            peers,
            viewers: viewers.to_vec(),
            shutdown,
        })
    }

    fn handle_signal(&self, message: SignalMessage) {
        let Some(webrtc) = self.peers.get(message.from()) else {
            tracing::debug!(from = %message.from(), "signal from a viewer we are not serving");
            return;
        };
        match message {
            SignalMessage::Answer { sdp, .. } => {
                match gst_sdp::SDPMessage::parse_buffer(sdp.as_bytes()) {
                    Ok(sdp) => {
                        let answer = gst_webrtc::WebRTCSessionDescription::new(
                            gst_webrtc::WebRTCSDPType::Answer,
                            sdp,
                        );
                        webrtc.emit_by_name::<()>(
                            "set-remote-description",
                            &[&answer, &None::<gst::Promise>],
                        );
                    }
                    Err(err) => tracing::warn!(%err, "failed to parse answer SDP"),
                }
            }
            SignalMessage::Ice {
                candidate,
                sdp_mline_index,
                ..
            } => {
                webrtc.emit_by_name::<()>("add-ice-candidate", &[&sdp_mline_index, &candidate]);
            }
            SignalMessage::Offer { .. } | SignalMessage::Bye { .. } => {}
        }
    }

    /// Forget a viewer we have stopped serving.
    ///
    /// Without this a viewer whose connection blipped would be dropped from the
    /// live set but still counted as served by the running pipeline, so its
    /// next hello would not look like a new arrival, nothing would rebuild, and
    /// it would never be offered video again. It would sit on "waiting for
    /// robot" forever while everyone else watched.
    fn forget(&mut self, viewer: &str) {
        self.viewers.retain(|v| v != viewer);
        self.peers.remove(viewer);
    }

    fn stop_sink(&self) {
        // Drop the callback before teardown so a sample already in flight on a
        // streaming thread cannot outlive the channel it was going to.
        if let Some(sink) = self.pipeline.by_name("broadcastsink") {
            if let Ok(sink) = sink.downcast::<gst_app::AppSink>() {
                sink.set_callbacks(gst_app::AppSinkCallbacks::builder().build());
            }
        }
    }

    fn stop(&self) {
        self.stop_sink();
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = self.pipeline.set_state(gst::State::Null);
        // Block until the pipeline has actually reached NULL so the camera fd is
        // released before the next one opens the same device. Without this, a
        // rebuild races the old v4l2src and the new one gets EBUSY.
        let _ = self.pipeline.state(gst::ClockTime::from_seconds(2));
    }
}

/// Wire the broadcast tap to the publishing task.
///
/// The callback runs on a GStreamer streaming thread, so it must neither block
/// nor await. It copies the access unit, stamps it, and hands it over; the
/// relay side of the work happens on the tokio task that owns the client.
fn attach_broadcast_sink(
    pipeline: &gst::Pipeline,
    frames: mpsc::Sender<EncodedFrame>,
    seq: Arc<AtomicU32>,
) -> anyhow::Result<()> {
    let sink = pipeline
        .by_name("broadcastsink")
        .ok_or_else(|| anyhow::anyhow!("pipeline has no appsink named 'broadcastsink'"))?
        .downcast::<gst_app::AppSink>()
        .map_err(|_| anyhow::anyhow!("broadcastsink is not an appsink"))?;

    sink.set_callbacks(
        gst_app::AppSinkCallbacks::builder()
            .new_sample(move |sink| {
                let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
                let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;

                // Anything that is not a delta unit is an IDR, which is the
                // only place a decoder that just arrived can start.
                let keyframe = !buffer.flags().contains(gst::BufferFlags::DELTA_UNIT);

                // try_send and never send: blocking here would push back into
                // the encoder and take the picture down for the peers too, to
                // deliver broadcast frames that are already too late to watch.
                let _ = frames.try_send(EncodedFrame {
                    seq: seq.fetch_add(1, Ordering::Relaxed),
                    keyframe,
                    data: map.as_slice().to_vec(),
                });
                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );
    Ok(())
}

/// Cut one access unit into relay-sized chunks and publish them.
///
/// A Stream, not a Param: this is high rate and best effort by contract, and a
/// dropped frame is corrected by the next keyframe rather than by retrying a
/// picture nobody wants any more.
async fn publish_frame(client: &Arc<Clasp>, addr: &Addresses, frame: EncodedFrame) {
    let address = addr.video_broadcast();
    for chunk in crate::broadcast::fragment(frame.seq, frame.keyframe, &frame.data) {
        if let Err(err) = client.stream(address.as_str(), Value::Bytes(chunk)).await {
            tracing::debug!(%err, "broadcast publish failed");
            return;
        }
    }
}

/// Watch the pipeline bus on a dedicated thread. Without this an error (camera
/// unplugged, encoder fault) is silent and the pipeline stays up but dead.
fn watch_bus(
    pipeline: &gst::Pipeline,
    generation: u64,
    shutdown: Arc<AtomicBool>,
    fail_tx: UnboundedSender<Failure>,
) {
    let Some(bus) = pipeline.bus() else { return };
    std::thread::spawn(move || loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        let Some(msg) = bus.timed_pop_filtered(
            gst::ClockTime::from_mseconds(250),
            &[gst::MessageType::Error, gst::MessageType::Eos],
        ) else {
            continue;
        };
        match msg.view() {
            gst::MessageView::Error(err) => tracing::warn!(
                generation,
                error = %err.error(),
                debug = ?err.debug(),
                "video pipeline error"
            ),
            gst::MessageView::Eos(_) => {
                tracing::warn!(generation, "video pipeline reached end of stream")
            }
            _ => continue,
        }
        let _ = fail_tx.send(Failure::Pipeline { generation });
        break;
    });
}

/// Wire one viewer's webrtcbin: offer on negotiation, trickle ICE out, and
/// report the peer going away.
fn connect_peer(
    webrtc: &gst::Element,
    viewer: &str,
    generation: u64,
    out_tx: &UnboundedSender<Outbound>,
    fail_tx: &UnboundedSender<Failure>,
) {
    {
        let fail_tx = fail_tx.clone();
        let viewer = viewer.to_string();
        webrtc.connect_notify(Some("connection-state"), move |webrtc, _pspec| {
            let state =
                webrtc.property::<gst_webrtc::WebRTCPeerConnectionState>("connection-state");
            match state {
                gst_webrtc::WebRTCPeerConnectionState::Connected => {
                    tracing::info!(%viewer, "video peer connected");
                }
                gst_webrtc::WebRTCPeerConnectionState::Failed
                | gst_webrtc::WebRTCPeerConnectionState::Closed => {
                    tracing::warn!(%viewer, ?state, "video peer connection lost");
                    let _ = fail_tx.send(Failure::Viewer {
                        generation,
                        viewer: viewer.clone(),
                    });
                }
                _ => {}
            }
        });
    }

    {
        let out_tx = out_tx.clone();
        let viewer = viewer.to_string();
        webrtc.connect("on-negotiation-needed", false, move |values| {
            let webrtc = values[0].get::<gst::Element>().expect("element argument");
            create_offer(&webrtc, out_tx.clone(), viewer.clone(), generation);
            None
        });
    }

    {
        let out_tx = out_tx.clone();
        let viewer = viewer.to_string();
        webrtc.connect("on-ice-candidate", false, move |values| {
            let sdp_mline_index = values[1].get::<u32>().expect("mline index");
            let candidate = values[2].get::<String>().expect("candidate string");
            let _ = out_tx.send(Outbound {
                generation,
                to: viewer.clone(),
                message: SignalMessage::Ice {
                    // Filled in as the message leaves; see the emit arm.
                    from: String::new(),
                    candidate,
                    sdp_mline_index,
                },
            });
            None
        });
    }
}

/// Pick the camera device to open. An explicit device that exists is trusted as
/// given (this covers a stable `/dev/v4l/by-id/...` path). When the configured
/// node is missing, fall back to the by-id capture symlink, because the USB
/// camera can re-enumerate onto a different `/dev/videoN` after a power glitch.
fn resolve_camera_device(configured: &str) -> String {
    if Path::new(configured).exists() {
        return configured.to_string();
    }

    // by-id "*-video-index0" symlinks point at the capture interface of each
    // USB video device and are stable across re-enumeration.
    if let Ok(entries) = std::fs::read_dir("/dev/v4l/by-id") {
        let mut capture_nodes: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.ends_with("-video-index0"))
                    .unwrap_or(false)
            })
            .collect();
        capture_nodes.sort();
        if let Some(dev) = capture_nodes.first() {
            let dev = dev.to_string_lossy().into_owned();
            tracing::warn!(
                configured,
                chosen = %dev,
                "configured camera device is missing; using by-id capture node"
            );
            return dev;
        }
    }

    tracing::warn!(
        configured,
        "camera device missing and no by-id capture node found; will retry on next viewer"
    );
    configured.to_string()
}

fn create_offer(
    webrtc: &gst::Element,
    out_tx: UnboundedSender<Outbound>,
    viewer: String,
    generation: u64,
) {
    // One clone is moved into the promise callback; the reference is used for
    // the create-offer call itself.
    let webrtc_local = webrtc.clone();
    let promise = gst::Promise::with_change_func(move |reply| {
        let Ok(Some(reply)) = reply else {
            tracing::warn!("create-offer produced no reply");
            return;
        };
        let Ok(offer) = reply.get::<gst_webrtc::WebRTCSessionDescription>("offer") else {
            tracing::warn!("create-offer reply had no offer");
            return;
        };
        webrtc_local.emit_by_name::<()>("set-local-description", &[&offer, &None::<gst::Promise>]);
        if let Ok(sdp) = offer.sdp().as_text() {
            let _ = out_tx.send(Outbound {
                generation,
                to: viewer.clone(),
                message: SignalMessage::Offer {
                    from: String::new(),
                    sdp,
                },
            });
        }
    });
    webrtc.emit_by_name::<()>("create-offer", &[&None::<gst::Structure>, &promise]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn watching(names: &[&str]) -> BTreeMap<String, Instant> {
        let now = Instant::now();
        names.iter().map(|n| (n.to_string(), now)).collect()
    }

    fn served(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn serving_exactly_the_current_audience_needs_no_rebuild() {
        assert!(!has_new_viewer(
            &served(&["alice", "bob"]),
            &watching(&["alice", "bob"])
        ));
    }

    #[test]
    fn a_viewer_leaving_does_not_rebuild() {
        // The departed viewer's branch stays until the next join, which costs a
        // payloader and nothing else. Rebuilding here would black out the
        // people still watching for no gain.
        assert!(!has_new_viewer(
            &served(&["alice", "bob"]),
            &watching(&["alice"])
        ));
    }

    #[test]
    fn a_viewer_joining_rebuilds() {
        assert!(has_new_viewer(
            &served(&["alice"]),
            &watching(&["alice", "bob"])
        ));
    }

    #[test]
    fn a_swap_within_the_same_count_rebuilds() {
        // Same size, different people: comparing counts would miss this.
        assert!(has_new_viewer(&served(&["alice"]), &watching(&["bob"])));
    }

    #[test]
    fn a_viewer_that_comes_back_after_dropping_out_rebuilds() {
        // A blipped connection drops the viewer from the live set, and
        // Broadcast::forget takes it out of the served list too. Without that
        // second half its next hello would not look new, nothing would
        // rebuild, and it would never be offered video again.
        let mut still_served = served(&["alice", "bob"]);
        still_served.retain(|v| v != "bob"); // what forget() does
        assert!(has_new_viewer(&still_served, &watching(&["alice", "bob"])));
    }
}
