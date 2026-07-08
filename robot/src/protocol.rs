//! The CLASP address contract and wire message shapes shared by the robot and
//! the web operator console. Both sides must agree on the paths and payloads
//! defined here. This module is the single source of truth for the protocol;
//! the equivalent for the web app lives in `web/src/protocol.js`.
//!
//! Signal type choices (see docs/protocol.md for the full rationale):
//!   - drive       Stream  (high rate, lossy tolerant, backed by a watchdog)
//!   - estop       Param   (latched, re-synced to late joiners)
//!   - cfg/*       Param   (setpoints that must survive reconnect)
//!   - status/*    Param   (what the UI must render correctly on connect)
//!   - tel/*       Stream   (high rate telemetry)
//!   - video/*     Param presence + Event signaling

use serde::{Deserialize, Serialize};

/// Builds the CLASP addresses for one robot rooted at `/robot/<id>`.
#[derive(Clone, Debug)]
pub struct Addresses {
    base: String,
}

impl Addresses {
    pub fn new(robot_id: &str) -> Self {
        Self {
            base: format!("/robot/{robot_id}"),
        }
    }

    /// Pattern the robot subscribes to for all inbound commands.
    pub fn cmd_pattern(&self) -> String {
        format!("{}/cmd/**", self.base)
    }

    pub fn drive(&self) -> String {
        format!("{}/cmd/drive", self.base)
    }

    pub fn estop(&self) -> String {
        format!("{}/cmd/estop", self.base)
    }

    pub fn cfg(&self, name: &str) -> String {
        format!("{}/cfg/{name}", self.base)
    }

    pub fn cfg_pattern(&self) -> String {
        format!("{}/cfg/**", self.base)
    }

    pub fn status(&self, name: &str) -> String {
        format!("{}/status/{name}", self.base)
    }

    pub fn tel(&self, name: &str) -> String {
        format!("{}/tel/{name}", self.base)
    }

    /// Shared address a viewer emits a `hello` Event to when it wants a stream.
    /// An Event (not a Param) so it is never snapshotted: only live viewers are
    /// seen, and stale entries cannot accumulate. Viewers repeat it until they
    /// have video, which also lets the robot recover across restarts.
    pub fn video_hello(&self) -> String {
        format!("{}/video/hello", self.base)
    }

    /// Address a signaling message is delivered to. Messages are keyed by the
    /// recipient's session id: the robot subscribes to its own session address,
    /// the operator subscribes to theirs.
    pub fn video_signal(&self, recipient_session: &str) -> String {
        format!("{}/video/signal/{recipient_session}", self.base)
    }

    pub fn video_signal_pattern(&self) -> String {
        format!("{}/video/signal/**", self.base)
    }
}

/// A continuous teleoperation command. Sent on `cmd/drive` as a Stream at
/// roughly 10 to 20 Hz. Both fields are normalized; the robot clamps them.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DriveCommand {
    /// Forward/backward demand, -1.0 (full reverse) to 1.0 (full forward).
    pub throttle: f64,
    /// Turn demand, -1.0 (full left) to 1.0 (full right).
    pub steer: f64,
    /// Monotonic sequence number from the operator, for out-of-order detection.
    #[serde(default)]
    pub seq: u64,
    /// Operator send timestamp in milliseconds, for latency measurement.
    #[serde(default)]
    pub ts: u64,
}

/// WebRTC signaling message exchanged over the `video/signal/<session>` Event
/// path. `from` is the sender's CLASP session id so a peer can reply and so a
/// peer can ignore echoes of its own messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SignalMessage {
    Offer {
        from: String,
        sdp: String,
    },
    Answer {
        from: String,
        sdp: String,
    },
    Ice {
        from: String,
        candidate: String,
        #[serde(rename = "sdpMLineIndex")]
        sdp_mline_index: u32,
    },
    Bye {
        from: String,
    },
}

impl SignalMessage {
    pub fn from(&self) -> &str {
        match self {
            SignalMessage::Offer { from, .. }
            | SignalMessage::Answer { from, .. }
            | SignalMessage::Ice { from, .. }
            | SignalMessage::Bye { from } => from,
        }
    }
}

/// Payload of a viewer's `hello` Event, telling the robot who wants a stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Presence {
    pub session: String,
    #[serde(default)]
    pub role: String,
}

/// Video-plane events the link layer forwards to the video task. Defined here,
/// outside the feature-gated video module, so the link layer can produce them
/// regardless of whether the video feature is compiled in.
#[derive(Debug, Clone)]
pub enum VideoEvent {
    /// A viewer announced itself and wants a stream.
    ViewerPresent(Presence),
    /// A signaling message addressed to the robot arrived.
    Signal(SignalMessage),
}
