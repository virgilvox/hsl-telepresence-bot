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
//! One viewer is served at a time; a new viewer replaces the current session.
//! This module builds and runs on a host with GStreamer 1.20+ and the good,
//! bad, and nice plugin sets installed. The signaling wiring against
//! gstreamer-rs should be validated on-device when first flashed.

use crate::config::Config;
use crate::protocol::{Addresses, SignalMessage, VideoEvent};
use clasp_client::Clasp;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_sdp as gst_sdp;
use gstreamer_webrtc as gst_webrtc;
use std::sync::Arc;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

const STUN_SERVER: &str = "stun://stun.l.google.com:19302";

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
        let mut current: Option<Session> = None;

        loop {
            tokio::select! {
                event = rx.recv() => {
                    let Some(event) = event else { break };
                    match event {
                        VideoEvent::ViewerPresent(presence) => {
                            let is_new = current
                                .as_ref()
                                .map(|s| s.viewer != presence.session)
                                .unwrap_or(true);
                            if !is_new {
                                continue;
                            }
                            if let Some(old) = current.take() {
                                old.stop();
                            }
                            match Session::start(&cfg, &session, &presence.session, out_tx.clone()) {
                                Ok(session) => {
                                    tracing::info!(viewer = %presence.session, "video session started");
                                    current = Some(session);
                                }
                                Err(err) => tracing::error!(%err, "failed to start video session"),
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
            }
        }

        if let Some(session) = current.take() {
            session.stop();
        }
    });
}

struct Session {
    pipeline: gst::Pipeline,
    webrtc: gst::Element,
    viewer: String,
}

impl Session {
    fn start(
        cfg: &Config,
        from: &str,
        viewer: &str,
        out_tx: UnboundedSender<SignalMessage>,
    ) -> anyhow::Result<Self> {
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
            device = cfg.camera_device,
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
        let _ = self.pipeline.set_state(gst::State::Null);
    }
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
