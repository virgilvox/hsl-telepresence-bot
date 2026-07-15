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
    /// The browser sends JS numbers, which CLASP may carry as either an integer
    /// or a float, so parse leniently: a float here must not fail the whole
    /// command (which would silently drop teleop and freeze the robot).
    #[serde(default, deserialize_with = "de_lenient_u64")]
    pub seq: u64,
    /// Operator send timestamp in milliseconds, for latency measurement.
    #[serde(default, deserialize_with = "de_lenient_u64")]
    pub ts: u64,
}

/// Deserialize a `u64` from any JSON number, integer or float. `Date.now()` and
/// a sequence counter arrive from JavaScript as plain numbers that CLASP can tag
/// as `Float`; serde's default `u64` path rejects a float outright, so we coerce
/// instead of erroring.
fn de_lenient_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::Number(n) => Ok(n
            .as_u64()
            .or_else(|| n.as_i64().map(|i| i.max(0) as u64))
            .or_else(|| n.as_f64().map(|f| f.max(0.0) as u64))
            .unwrap_or(0)),
        _ => Ok(0),
    }
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
