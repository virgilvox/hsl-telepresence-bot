//! Video plane. Captures the stereoscopic USB camera once and streams it to
//! every operator over native WebRTC media tracks. CLASP is used only to
//! exchange presence and SDP/ICE signaling; the media itself never touches the
//! relay, because CLASP is a control transport, not a media transport.
//!
//! The robot is the offerer: when viewers announce presence, the robot builds a
//! GStreamer pipeline whose `webrtcbin`s each produce an offer. Answers and ICE
//! candidates come back through the link layer as [`VideoEvent::Signal`].
//!
//! Pipeline: v4l2src (MJPEG) -> jpegdec -> H264 (hardware v4l2h264enc) -> RTP
//! -> tee -> one queue + webrtcbin per viewer. Capture and encode happen once
//! no matter how many people are watching, which is what makes several viewers
//! affordable on a Pi 3B+: only payloading and encryption scale with the
//! audience.
//!
//! # Why the pipeline is rebuilt when someone joins
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
//! Departures do *not* rebuild. A viewer that leaves keeps a dead branch until
//! the next join, which costs nothing but a payloader, and each branch queue is
//! `leaky=downstream` so a stalled or dead peer drops buffers instead of
//! blocking the tee and freezing everybody else.
//!
//! The pipeline bus and each WebRTC connection state are watched: a camera
//! drop, encoder fault, or peer disconnect tears the affected thing down so it
//! restarts cleanly, rather than dying silently.
//!
//! This module builds and runs on a host with GStreamer 1.20+ and the good,
//! bad, and nice plugin sets installed.

use crate::config::Config;
use crate::protocol::{Addresses, SignalMessage, VideoEvent};
use clasp_client::Clasp;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_sdp as gst_sdp;
use gstreamer_webrtc as gst_webrtc;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

const STUN_SERVER: &str = "stun://stun.l.google.com:19302";

/// Upper bound on simultaneous viewers. Encoding is shared, but each extra peer
/// still costs a payloader, DTLS/SRTP, and outbound bandwidth, and the Pi 3B+
/// has one 100 Mbit NIC and not much CPU left over. Past this, late arrivals
/// wait rather than degrading the picture for everyone.
const MAX_VIEWERS: usize = 4;

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

        // Live viewers and when each was last heard from. Ordered so the
        // pipeline description is stable for a given set.
        let mut viewers: BTreeMap<String, Instant> = BTreeMap::new();
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
                            let known = viewers.contains_key(&presence.session);
                            if !known && viewers.len() >= MAX_VIEWERS {
                                let now = Instant::now();
                                let due = last_full_warning
                                    .map(|t| now.saturating_duration_since(t) >= FULL_WARNING_EVERY)
                                    .unwrap_or(true);
                                if due {
                                    last_full_warning = Some(now);
                                    tracing::warn!(
                                        viewer = %presence.session,
                                        max = MAX_VIEWERS,
                                        "viewer limit reached; turning new viewers away"
                                    );
                                }
                                continue;
                            }
                            viewers.insert(presence.session.clone(), Instant::now());
                            if !known {
                                tracing::info!(viewer = %presence.session, watching = viewers.len(), "viewer joined");
                            }
                        }
                        VideoEvent::Signal(message) => {
                            if let SignalMessage::Bye { from } = &message {
                                if viewers.remove(from).is_some() {
                                    if let Some(b) = broadcast.as_mut() {
                                        b.forget(from);
                                    }
                                    tracing::info!(viewer = %from, watching = viewers.len(), "viewer said goodbye");
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
                            let was_watching = viewers.remove(&viewer).is_some();
                            if was_watching {
                                if let Some(b) = broadcast.as_mut() {
                                    b.forget(&viewer);
                                }
                                tracing::info!(%viewer, watching = viewers.len(), "viewer connection lost");
                            }
                        }
                    }
                }
                _ = sweep.tick() => {
                    let now = Instant::now();
                    let expired: Vec<String> = viewers
                        .iter()
                        .filter(|(_, seen)| now.saturating_duration_since(**seen) >= VIEWER_TIMEOUT)
                        .map(|(viewer, _)| viewer.clone())
                        .collect();
                    for viewer in expired {
                        viewers.remove(&viewer);
                        if let Some(b) = broadcast.as_mut() {
                            b.forget(&viewer);
                        }
                        tracing::info!(%viewer, watching = viewers.len(), "viewer timed out");
                    }

                    if viewers.is_empty() {
                        if let Some(old) = broadcast.take() {
                            tracing::info!("no viewers left; releasing the camera");
                            teardown(old).await;
                        }
                        rebuild_at = None;
                    } else if broadcast
                        .as_ref()
                        .map_or(true, |b| has_new_viewer(&b.viewers, &viewers))
                    {
                        let due = rebuild_at.get_or_insert(now + JOIN_DEBOUNCE);
                        if now >= *due {
                            rebuild_at = None;
                            generation += 1;
                            let old = broadcast.take();
                            let audience: Vec<String> = viewers.keys().cloned().collect();
                            match rebuild(old, &cfg, audience.clone(), generation, &out_tx, &fail_tx)
                            .await
                            {
                                Ok(b) => {
                                    tracing::info!(
                                        viewers = audience.len(),
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
                    if published_count != Some(viewers.len()) {
                        published_count = Some(viewers.len());
                        let payload = crate::link::to_value(serde_json::json!(viewers.len()));
                        if let Err(err) = client.set(addr.status("viewers").as_str(), payload).await {
                            tracing::debug!(%err, "viewer count publish failed");
                        }
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
async fn rebuild(
    old: Option<Broadcast>,
    cfg: &Config,
    audience: Vec<String>,
    generation: u64,
    out_tx: &UnboundedSender<Outbound>,
    fail_tx: &UnboundedSender<Failure>,
) -> anyhow::Result<Broadcast> {
    let cfg = cfg.clone();
    let out_tx = out_tx.clone();
    let fail_tx = fail_tx.clone();
    tokio::task::spawn_blocking(move || {
        if let Some(old) = old {
            old.stop();
        }
        Broadcast::start(&cfg, &audience, generation, &out_tx, &fail_tx)
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
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !viewers.is_empty(),
            "refusing to build a pipeline with no viewers"
        );

        // Resolve the camera fresh each time: the USB camera can re-enumerate
        // to a different /dev/videoN after a power glitch, so a fixed node goes
        // stale. See resolve_camera_device.
        let device = resolve_camera_device(&cfg.camera_device);

        // The Pi's VideoCore H264 encoder requires explicit output caps
        // (a level string), otherwise it fails to process frames. It also
        // maxes out at 1920 wide, so capture the camera's 1280x480 side-by-side
        // mode rather than its 2560-wide modes.
        let mut description = format!(
            "v4l2src device={device} ! image/jpeg,width={width},height={height},framerate={fps}/1 \
             ! jpegdec ! queue ! videoconvert ! video/x-raw,format=I420 \
             ! v4l2h264enc ! video/x-h264,level=(string)4 \
             ! h264parse config-interval=-1 \
             ! rtph264pay pt=96 ! application/x-rtp,media=video,encoding-name=H264,payload=96 \
             ! tee name=fanout",
            device = device,
            width = cfg.camera_width,
            height = cfg.camera_height,
            fps = cfg.camera_fps,
        );
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

    fn stop(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = self.pipeline.set_state(gst::State::Null);
        // Block until the pipeline has actually reached NULL so the camera fd is
        // released before the next one opens the same device. Without this, a
        // rebuild races the old v4l2src and the new one gets EBUSY.
        let _ = self.pipeline.state(gst::ClockTime::from_seconds(2));
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
