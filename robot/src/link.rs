//! The only module that talks to the CLASP relay. It connects, publishes the
//! robot's latched status, subscribes to inbound commands and video signaling,
//! and forwards decoded messages onto in-process channels. It knows nothing
//! about motors or GStreamer; it moves typed values.

use crate::config::Config;
use crate::motion::MotionCommand;
use crate::protocol::{Addresses, DriveCommand, Presence, SignalMessage, VideoEvent};
use clasp_client::prelude::Value;
use clasp_client::Clasp;
use serde::de::DeserializeOwned;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

/// A live relay connection plus the addresses and session identity the rest of
/// the robot needs to publish telemetry and video signaling.
pub struct Link {
    pub client: Arc<Clasp>,
    pub addr: Addresses,
    pub session: String,
}

impl Link {
    /// Clear the online status. Called on shutdown so operators see the robot
    /// drop off promptly rather than waiting for a stale-timeout.
    pub async fn set_offline(&self) -> anyhow::Result<()> {
        self.client
            .set(self.addr.status("online").as_str(), false)
            .await?;
        Ok(())
    }
}

/// Connect to the relay, publish initial status, and wire up subscriptions.
pub async fn connect(
    cfg: &Config,
    motion_tx: UnboundedSender<MotionCommand>,
    video_tx: UnboundedSender<VideoEvent>,
) -> anyhow::Result<Link> {
    let mut builder = Clasp::builder(&cfg.clasp_url)
        .name(&cfg.robot_name)
        .features(vec!["param".into(), "event".into(), "stream".into()])
        .reconnect(true);
    if let Some(token) = &cfg.clasp_token {
        builder = builder.token(token);
    }

    let client = Arc::new(builder.connect().await?);
    let session = client.session_id().unwrap_or_default();
    let addr = Addresses::new(&cfg.robot_id);

    // Latched status the operator console renders on connect.
    client.set(addr.status("online").as_str(), true).await?;
    client.set(addr.status("mode").as_str(), "manual").await?;
    client.set(addr.status("estop").as_str(), false).await?;

    subscribe_commands(&client, &addr, motion_tx).await?;
    subscribe_video(&client, &addr, &session, video_tx).await?;

    Ok(Link {
        client,
        addr,
        session,
    })
}

async fn subscribe_commands(
    client: &Arc<Clasp>,
    addr: &Addresses,
    motion_tx: UnboundedSender<MotionCommand>,
) -> anyhow::Result<()> {
    // Drive and e-stop live under cmd/**.
    let drive_addr = addr.drive();
    let estop_addr = addr.estop();
    let tx = motion_tx.clone();
    client
        .subscribe(addr.cmd_pattern().as_str(), move |value, address| {
            if address == drive_addr {
                if let Some(cmd) = decode::<DriveCommand>(&value) {
                    let _ = tx.send(MotionCommand::Drive(cmd));
                }
            } else if address == estop_addr {
                if let Some(engaged) = as_bool(&value) {
                    let _ = tx.send(MotionCommand::EStop(engaged));
                }
            }
        })
        .await?;

    // Config setpoints live under cfg/**.
    let max_speed_addr = addr.cfg("max_speed");
    let tx = motion_tx;
    client
        .subscribe(addr.cfg_pattern().as_str(), move |value, address| {
            if address == max_speed_addr {
                if let Some(value) = as_f64(&value) {
                    let _ = tx.send(MotionCommand::SetMaxSpeed(value));
                }
            }
        })
        .await?;

    Ok(())
}

async fn subscribe_video(
    client: &Arc<Clasp>,
    addr: &Addresses,
    session: &str,
    video_tx: UnboundedSender<VideoEvent>,
) -> anyhow::Result<()> {
    // Viewers announce themselves via presence Params.
    let tx = video_tx.clone();
    client
        .subscribe(
            addr.video_presence_pattern().as_str(),
            move |value, _address| {
                if let Some(presence) = decode::<Presence>(&value) {
                    let _ = tx.send(VideoEvent::ViewerPresent(presence));
                }
            },
        )
        .await?;

    // Signaling addressed to us. The address ends with the recipient session;
    // we process only messages addressed to us, and ignore echoes of our own.
    let me = session.to_string();
    let tx = video_tx;
    client
        .subscribe(
            addr.video_signal_pattern().as_str(),
            move |value, address| {
                let recipient = address.rsplit('/').next().unwrap_or_default();
                if recipient != me {
                    return;
                }
                if let Some(message) = decode::<SignalMessage>(&value) {
                    if message.from() == me {
                        return;
                    }
                    let _ = tx.send(VideoEvent::Signal(message));
                }
            },
        )
        .await?;

    Ok(())
}

// CLASP values are a typed `Value` enum with no direct serde_json bridge, so we
// round-trip through serde_json here. This is the single place conversion
// happens, in both directions.
fn to_json(value: &Value) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}

fn decode<T: DeserializeOwned>(value: &Value) -> Option<T> {
    serde_json::from_value(to_json(value)).ok()
}

/// Convert a JSON payload into a CLASP `Value`. Used by the telemetry and video
/// planes to publish structured objects (`Value::Map`) and arrays.
pub(crate) fn to_value(json: serde_json::Value) -> Value {
    serde_json::from_value(json).unwrap_or(Value::Null)
}

fn as_bool(value: &Value) -> Option<bool> {
    to_json(value).as_bool()
}

fn as_f64(value: &Value) -> Option<f64> {
    to_json(value).as_f64()
}
