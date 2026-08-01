//! The CLASP address contract and wire message shapes shared by the robot and
//! the web operator console. Both sides must agree on the paths and payloads
//! defined here. This module is the single source of truth for the protocol;
//! the equivalent for the web app lives in `web/src/protocol.js`.
//!
//! Signal type choices (see docs/protocol.md for the full rationale):
//!   - drive       Stream  (high rate, lossy tolerant, backed by a watchdog)
//!   - estop       Param   (latched, re-synced to late joiners)
//!   - control     Event   (one-shot claim/release of the driving lease)
//!   - cfg/*       Param   (setpoints that must survive reconnect)
//!   - status/*    Param   (what the UI must render correctly on connect)
//!   - tel/*       Stream   (high rate telemetry)
//!   - video/*     Param presence + Event signaling

use serde::{Deserialize, Serialize};

/// Wire contract version, published as `status/protocol` on connect.
///
/// A console uses it to tell a multi-operator robot from an older one. Version
/// 1 served a single viewer and had no notion of who was driving; version 2
/// serves several viewers at once and arbitrates the wheel. When this is
/// absent, the console must assume 1 and let anyone drive, because an older
/// robot ignores control messages entirely and would otherwise look permanently
/// locked.
pub const PROTOCOL_VERSION: u32 = 2;

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

    /// Where operators claim and release the driving lease.
    pub fn control(&self) -> String {
        format!("{}/cmd/control", self.base)
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

    /// Where the encoded stream is published for everyone not on a WebRTC
    /// track. One publication, fanned out by the relay, so the robot pays the
    /// same whether one person is watching or fifty. See `broadcast.rs`.
    pub fn video_broadcast(&self) -> String {
        format!("{}/video/broadcast", self.base)
    }

    /// Where the agent writes its own liveness token and reads it back.
    ///
    /// Deliberately outside `status/`, `tel/` and `cmd/`: no console subscribes
    /// here, so the check costs an operator nothing and can never be mistaken
    /// for robot state. See `health.rs` for what it proves.
    pub fn health(&self) -> String {
        format!("{}/health", self.base)
    }
}

/// A continuous teleoperation command. Sent on `cmd/drive` as a Stream at
/// roughly 10 to 20 Hz. Both fields are normalized; the robot clamps them.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// CLASP session of the operator that sent this. The robot obeys drive
    /// commands only from whoever holds the lease (see `control::Arbiter`).
    /// Defaults to empty for consoles predating multi-operator support, which
    /// the arbiter treats as one anonymous operator rather than rejecting.
    #[serde(default)]
    pub session: String,
    /// Display name of the operator, carried here so that taking a free wheel
    /// by simply driving still names the driver correctly. Sending it out of
    /// band would race the drive itself, and the other consoles would show the
    /// new driver as "operator" until the two messages settled.
    #[serde(default)]
    pub name: String,
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

/// A request to take or give up the driving lease, sent as an Event on
/// `cmd/control`. Watching is unrestricted; only driving is arbitrated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum ControlCommand {
    /// Take the wheel, displacing whoever holds it.
    Claim {
        session: String,
        #[serde(default)]
        name: String,
    },
    /// Give the wheel up. Ignored unless this session holds it.
    Release { session: String },
}

/// Who currently holds the driving lease. Published as the `status/driver`
/// Param, or null when the wheel is free.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Driver {
    /// CLASP session of the operator holding the wheel.
    pub session: String,
    /// Display name, for the other operators' consoles.
    pub name: String,
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

    /// Stamp the sender. Done as the message leaves rather than where it is
    /// built, because the relay issues a new session on every reconnect and the
    /// only correct value is the one current at the moment of sending.
    pub fn set_from(&mut self, who: String) {
        match self {
            SignalMessage::Offer { from, .. }
            | SignalMessage::Answer { from, .. }
            | SignalMessage::Ice { from, .. }
            | SignalMessage::Bye { from } => *from = who,
        }
    }
}

/// Role a viewer asks for in its `hello`. A console that wants to drive needs
/// the latency only a peer connection gives; one that is watching does not, and
/// asking for the cheaper path is what lets the audience grow without bound.
pub const ROLE_BROADCAST: &str = "broadcast";

/// Payload of a viewer's `hello` Event, telling the robot who wants a stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Presence {
    pub session: String,
    #[serde(default)]
    pub role: String,
}

impl Presence {
    /// Whether this viewer is asking for a WebRTC track of its own.
    ///
    /// Anything that is not explicitly a broadcast watcher gets a peer, so a
    /// console written before the broadcast path existed, which sends
    /// `role: "viewer"` or no role at all, keeps behaving exactly as it did.
    pub fn wants_peer(&self) -> bool {
        self.role != ROLE_BROADCAST
    }
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
