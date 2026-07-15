//! Video plane. Captures the stereoscopic USB camera and streams it to the
//! operator over a native WebRTC media track. CLASP is used only to exchange
//! presence and SDP/ICE signaling; the media itself never touches the relay,
//! because CLASP is a control transport, not a media transport.
//!
//! The robot is the offerer: when a viewer announces presence, the robot builds
//! a GStreamer pipeline whose `webrtcbin` produces an offer. Answers and ICE
//! candidates come back through the link layer as [`VideoEvent::Signal`].
//!
//! Pipeline: v4l2src (MJPEG) -> jpegdec -> H264 (hardware v4l2h264enc) -> RTP
//! -> webrtcbin. On the Pi 3B+ the VideoCore H264 encoder keeps CPU low.
//!
//! One viewer is served at a time. Once a viewer's peer connection is
//! established it holds the camera; another viewer's hello is ignored until the
//! current one disconnects or its session fails. A session that never
//! establishes within a short grace window can be handed to a waiting viewer, so
//! a stale viewer does not block the camera forever. This avoids two viewers
//! stealing the stream from each other in a loop.
//!
//! The pipeline bus and the WebRTC connection state are both watched: a camera
//! drop, encoder fault, or peer disconnect tears the session down so it can
//! restart cleanly on the next hello, rather than dying silently.
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
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

const STUN_SERVER: &str = "stun://stun.l.google.com:19302";

/// How long a just-started session is protected from being preempted by another
/// viewer's hello. A viewer keeps saying hello until its video is live, so with
/// two viewers online this window lets the current one finish negotiating
/// instead of the two of them stealing the camera from each other forever.
const SESSION_GRACE: Duration = Duration::from_secs(10);

pub fn spawn(
    client: Arc<Clasp>,
    addr: Addresses,
    cfg: Config,
    session: String,
    mut rx: UnboundedReceiver<VideoEvent>,
) {
    tokio::spawn(async move {
        if let Err(err) = gst::init() {
            tracing::error!(%err, "failed to initialize GStreamer; video disabled");
            return;
        }

        // Signaling produced inside GStreamer callbacks is funneled here, then
        // emitted over CLASP by this task.
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<SignalMessage>();
        // A session whose pipeline errors (most often the USB camera dropping
        // off the bus) reports its viewer id here so we tear it down. The
        // viewer keeps saying hello every couple of seconds, so a clean session
        // starts again on its own once the camera is back.
        let (fail_tx, mut fail_rx) = mpsc::unbounded_channel::<String>();
        let mut current: Option<Session> = None;

        loop {
            tokio::select! {
                event = rx.recv() => {
                    let Some(event) = event else { break };
                    match event {
                        VideoEvent::ViewerPresent(presence) => {
                            match current.as_ref() {
                                // Our current viewer is still saying hello because
                                // its video is not live yet. Keep its session.
                                Some(s) if s.viewer == presence.session => {}
                                // A different viewer wants in. Keep serving the
                                // current one while a peer is actually connected,
                                // or while it is still inside its negotiation grace
                                // window. Only hand off once the current session
                                // has had its chance and nobody is watching it,
                                // which is also how we recover from a stale viewer.
                                Some(s) => {
                                    let established = s.established.load(Ordering::Relaxed);
                                    let within_grace = s.started.elapsed() < SESSION_GRACE;
                                    let holder = s.viewer.clone();
                                    if established || within_grace {
                                        tracing::debug!(
                                            holding = %holder,
                                            waiting = %presence.session,
                                            established,
                                            "ignoring new viewer; current session holds the camera"
                                        );
                                    } else {
                                        tracing::info!(
                                            old = %holder,
                                            new = %presence.session,
                                            "handing video over to a new viewer"
                                        );
                                        if let Some(old) = current.take() {
                                            old.stop();
                                        }
                                        current = spawn_session(
                                            &cfg, &session, &presence.session, &out_tx, &fail_tx,
                                        );
                                    }
                                }
                                None => {
                                    current = spawn_session(
                                        &cfg, &session, &presence.session, &out_tx, &fail_tx,
                                    );
                                }
                            }
                        }
                        VideoEvent::Signal(message) => {
                            if let Some(session) = current.as_ref() {
                                session.handle_signal(message);
                            }
                        }
                    }
                }
                outbound = out_rx.recv() => {
                    let Some(message) = outbound else { continue };
                    if let Some(session) = current.as_ref() {
                        let payload = crate::link::to_value(
                            serde_json::to_value(&message).unwrap_or(serde_json::Value::Null),
                        );
                        if let Err(err) = client
                            .emit(addr.video_signal(&session.viewer).as_str(), payload)
                            .await
                        {
                            tracing::debug!(%err, "signal emit failed");
                        }
                    }
                }
                failed = fail_rx.recv() => {
                    let Some(viewer) = failed else { continue };
                    // Ignore a report from a session we already replaced.
                    if current.as_ref().map(|s| s.viewer == viewer).unwrap_or(false) {
                        tracing::warn!(%viewer, "video pipeline failed; tearing down session, will restart on next hello");
                        if let Some(old) = current.take() {
                            old.stop();
                        }
                    }
                }
            }
        }

        if let Some(session) = current.take() {
            session.stop();
        }
    });
}

/// Build a session for `viewer`, logging success or failure. Returns None on a
/// build error so the caller simply leaves the slot empty and retries on the
/// next hello.
fn spawn_session(
    cfg: &Config,
    from: &str,
    viewer: &str,
    out_tx: &UnboundedSender<SignalMessage>,
    fail_tx: &UnboundedSender<String>,
) -> Option<Session> {
    match Session::start(cfg, from, viewer, out_tx.clone(), fail_tx.clone()) {
        Ok(session) => {
            tracing::info!(%viewer, "video session started");
            Some(session)
        }
        Err(err) => {
            tracing::error!(%err, %viewer, "failed to start video session");
            None
        }
    }
}

struct Session {
    pipeline: gst::Pipeline,
    webrtc: gst::Element,
    viewer: String,
    /// When the session started, used to protect it from preemption while it
    /// negotiates (see SESSION_GRACE).
    started: Instant,
    /// Set once the WebRTC peer connection reaches Connected: a viewer is really
    /// watching, so we must not hand the camera to anyone else.
    established: Arc<AtomicBool>,
    /// Signals the per-session bus-watch thread to exit on teardown.
    shutdown: Arc<AtomicBool>,
}

impl Session {
    fn start(
        cfg: &Config,
        from: &str,
        viewer: &str,
        out_tx: UnboundedSender<SignalMessage>,
        fail_tx: UnboundedSender<String>,
    ) -> anyhow::Result<Self> {
        // Resolve the camera fresh each session: the USB camera can re-enumerate
        // to a different /dev/videoN after a power glitch, so a fixed node goes
        // stale. See resolve_camera_device.
        let device = resolve_camera_device(&cfg.camera_device);

        // The Pi's VideoCore H264 encoder requires explicit output caps
        // (a level string), otherwise it fails to process frames. It also
        // maxes out at 1920 wide, so capture the camera's 1280x480 side-by-side
        // mode rather than its 2560-wide modes.
        let description = format!(
            "v4l2src device={device} ! image/jpeg,width={width},height={height},framerate={fps}/1 \
             ! jpegdec ! queue ! videoconvert ! video/x-raw,format=I420 \
             ! v4l2h264enc ! video/x-h264,level=(string)4 \
             ! h264parse config-interval=-1 \
             ! rtph264pay pt=96 ! application/x-rtp,media=video,encoding-name=H264,payload=96 \
             ! webrtcbin name=webrtc bundle-policy=max-bundle",
            device = device,
            width = cfg.camera_width,
            height = cfg.camera_height,
            fps = cfg.camera_fps,
        );

        let pipeline = gst::parse::launch(&description)?
            .downcast::<gst::Pipeline>()
            .map_err(|_| anyhow::anyhow!("constructed element is not a pipeline"))?;

        let webrtc = pipeline
            .by_name("webrtc")
            .ok_or_else(|| anyhow::anyhow!("pipeline has no webrtcbin named 'webrtc'"))?;
        webrtc.set_property_from_str("stun-server", STUN_SERVER);

        // Watch the pipeline bus on a dedicated thread. Without this an error
        // (camera unplugged, encoder fault) is silent and the session stays up
        // but dead. On the first error or EOS we report the viewer id so the
        // main loop tears the session down and lets it restart cleanly.
        let shutdown = Arc::new(AtomicBool::new(false));
        if let Some(bus) = pipeline.bus() {
            let shutdown = shutdown.clone();
            let viewer_id = viewer.to_string();
            let fail_tx = fail_tx.clone();
            std::thread::spawn(move || {
                loop {
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
                            viewer = %viewer_id,
                            error = %err.error(),
                            debug = ?err.debug(),
                            "video pipeline error"
                        ),
                        gst::MessageView::Eos(_) => {
                            tracing::warn!(viewer = %viewer_id, "video pipeline reached end of stream")
                        }
                        _ => continue,
                    }
                    let _ = fail_tx.send(viewer_id.clone());
                    break;
                }
            });
        }

        // Track the WebRTC peer connection state. Reaching Connected means a
        // viewer is really watching (so the session must not be preempted);
        // Failed or Closed means the viewer is gone, so tear the session down and
        // free the camera for the next hello.
        let established = Arc::new(AtomicBool::new(false));
        {
            let established = established.clone();
            let fail_tx = fail_tx.clone();
            let viewer_id = viewer.to_string();
            webrtc.connect_notify(Some("connection-state"), move |webrtc, _pspec| {
                let state = webrtc
                    .property::<gst_webrtc::WebRTCPeerConnectionState>("connection-state");
                match state {
                    gst_webrtc::WebRTCPeerConnectionState::Connected => {
                        established.store(true, Ordering::Relaxed);
                        tracing::info!(viewer = %viewer_id, "video peer connected");
                    }
                    gst_webrtc::WebRTCPeerConnectionState::Failed
                    | gst_webrtc::WebRTCPeerConnectionState::Closed => {
                        tracing::warn!(viewer = %viewer_id, ?state, "video peer connection lost");
                        let _ = fail_tx.send(viewer_id.clone());
                    }
                    _ => {}
                }
            });
        }

        // Produce an offer as soon as the pipeline is ready to negotiate.
        {
            let out_tx = out_tx.clone();
            let from = from.to_string();
            webrtc.connect("on-negotiation-needed", false, move |values| {
                let webrtc = values[0].get::<gst::Element>().expect("element argument");
                create_offer(&webrtc, out_tx.clone(), from.clone());
                None
            });
        }

        // Trickle local ICE candidates out through signaling.
        {
            let out_tx = out_tx;
            let from = from.to_string();
            webrtc.connect("on-ice-candidate", false, move |values| {
                let sdp_mline_index = values[1].get::<u32>().expect("mline index");
                let candidate = values[2].get::<String>().expect("candidate string");
                let _ = out_tx.send(SignalMessage::Ice {
                    from: from.clone(),
                    candidate,
                    sdp_mline_index,
                });
                None
            });
        }

        pipeline.set_state(gst::State::Playing)?;

        Ok(Self {
            pipeline,
            webrtc,
            viewer: viewer.to_string(),
            started: Instant::now(),
            established,
            shutdown,
        })
    }

    fn handle_signal(&self, message: SignalMessage) {
        match message {
            SignalMessage::Answer { sdp, .. } => {
                match gst_sdp::SDPMessage::parse_buffer(sdp.as_bytes()) {
                    Ok(sdp) => {
                        let answer = gst_webrtc::WebRTCSessionDescription::new(
                            gst_webrtc::WebRTCSDPType::Answer,
                            sdp,
                        );
                        self.webrtc.emit_by_name::<()>(
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
                self.webrtc
                    .emit_by_name::<()>("add-ice-candidate", &[&sdp_mline_index, &candidate]);
            }
            SignalMessage::Offer { .. } | SignalMessage::Bye { .. } => {}
        }
    }

    fn stop(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = self.pipeline.set_state(gst::State::Null);
        // Block until the pipeline has actually reached NULL so the camera fd is
        // released before the next session opens the same device. Without this,
        // a viewer takeover races the old v4l2src and the new one gets EBUSY.
        let _ = self.pipeline.state(gst::ClockTime::from_seconds(2));
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

fn create_offer(webrtc: &gst::Element, out_tx: UnboundedSender<SignalMessage>, from: String) {
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
            let _ = out_tx.send(SignalMessage::Offer {
                from: from.clone(),
                sdp,
            });
        }
    });
    webrtc.emit_by_name::<()>("create-offer", &[&None::<gst::Structure>, &promise]);
}
