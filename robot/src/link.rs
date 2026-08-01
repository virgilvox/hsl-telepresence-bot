//! The only module that talks to the CLASP relay. It connects, publishes the
//! robot's latched status, subscribes to inbound commands and video signaling,
//! and forwards decoded messages onto in-process channels. It knows nothing
//! about motors or GStreamer; it moves typed values.

use crate::config::Config;
use crate::control::Arbiter;
use crate::health::Health;
use crate::motion::MotionCommand;
use crate::protocol::{
    Addresses, ControlCommand, DriveCommand, Presence, SignalMessage, VideoEvent, PROTOCOL_VERSION,
};
use anyhow::Context;
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
    /// Round-trip check on this link, kept so the supervisor in `main` can wait
    /// on it. See `health.rs` for what it is for.
    pub health: Arc<Health>,
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

/// Connect to the relay, subscribe, prove the link works, then publish status.
///
/// The order is the point. Subscribing first puts the relay's retained command
/// values, a latched e-stop above all, on their way before anything else
/// happens. Verifying next turns the first round trip into a barrier, because
/// it cannot come back ahead of values the relay queued before it. Only then
/// does the robot announce itself, so `status/online` is a statement that
/// commands will be obeyed rather than a hope that they will be.
pub async fn connect(
    cfg: &Config,
    motion_tx: UnboundedSender<MotionCommand>,
    video_tx: UnboundedSender<VideoEvent>,
    arbiter: Arc<Arbiter>,
) -> anyhow::Result<Link> {
    let mut builder = Clasp::builder(&cfg.clasp_url)
        .name(&cfg.robot_name)
        .features(vec!["param".into(), "event".into(), "stream".into()])
        .reconnect(true);
    if let Some(token) = &cfg.clasp_token {
        builder = builder.token(token);
    }

    let client = Arc::new(builder.connect().await?);
    // `reconnect(true)` on the builder only records the intent. The client
    // reconnects when this loop is running and not otherwise: without it the
    // disconnect notification has nobody listening for it, a dropped socket is
    // never rebuilt, and because nothing anywhere returns an error the process
    // stays up looking healthy while the robot is gone.
    client.start_reconnect_loop();

    let session = client.session_id().unwrap_or_default();
    let addr = Addresses::new(&cfg.robot_id);

    subscribe_commands(&client, &addr, motion_tx, arbiter).await?;
    subscribe_video(&client, &addr, video_tx).await?;

    let health = Health::attach(client.clone(), &addr).await?;
    health
        .verify()
        .await
        .context("relay accepted the connection but does not deliver to it")?;

    publish_status(&client, &addr).await?;

    Ok(Link {
        client,
        addr,
        session,
        health,
    })
}

/// The latched status an operator console renders the moment it connects.
async fn publish_status(client: &Arc<Clasp>, addr: &Addresses) -> anyhow::Result<()> {
    client.set(addr.status("online").as_str(), true).await?;
    client.set(addr.status("mode").as_str(), "manual").await?;
    // Tells a console this robot arbitrates the wheel and can serve several
    // viewers. Its absence is what marks an older robot, so it must be
    // published before anything else can act on it.
    client
        .set(
            addr.status("protocol").as_str(),
            to_value(serde_json::json!(PROTOCOL_VERSION)),
        )
        .await?;
    // Start from a known-free wheel and an empty room rather than leaving last
    // run's values latched on the relay.
    client
        .set(addr.status("driver").as_str(), Value::Null)
        .await?;
    client
        .set(
            addr.status("viewers").as_str(),
            to_value(serde_json::json!(0)),
        )
        .await?;
    // `status/estop` is deliberately not published here. The telemetry plane
    // mirrors it from the motors themselves, so a robot that comes up to an
    // e-stop latched on the relay never announces itself as clear first.
    Ok(())
}

async fn subscribe_commands(
    client: &Arc<Clasp>,
    addr: &Addresses,
    motion_tx: UnboundedSender<MotionCommand>,
    arbiter: Arc<Arbiter>,
) -> anyhow::Result<()> {
    // Drive, e-stop, and control live under cmd/**.
    let drive_addr = addr.drive();
    let estop_addr = addr.estop();
    let control_addr = addr.control();
    let tx = motion_tx.clone();
    client
        .subscribe(addr.cmd_pattern().as_str(), move |value, address| {
            if address == drive_addr {
                if let Some(cmd) = decode::<DriveCommand>(&value) {
                    // Only the operator holding the wheel moves the robot.
                    // Everyone else's commands are dropped here, before they
                    // can reach the motors or reset the drive watchdog.
                    if arbiter.accepts(&cmd.session, &cmd.name) {
                        let _ = tx.send(MotionCommand::Drive(cmd));
                    }
                }
            } else if address == estop_addr {
                // Never arbitrated: anyone watching can stop the robot.
                if let Some(engaged) = as_bool(&value) {
                    let _ = tx.send(MotionCommand::EStop(engaged));
                }
            } else if address == control_addr {
                match decode::<ControlCommand>(&value) {
                    Some(ControlCommand::Claim { session, name }) => {
                        if let Some(displaced) = arbiter.claim(&session, &name) {
                            tracing::info!(
                                from = %displaced.name,
                                to = %name,
                                "wheel taken over"
                            );
                        }
                    }
                    Some(ControlCommand::Release { session }) => {
                        arbiter.release(&session);
                    }
                    None => tracing::debug!("undecodable control command"),
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
    video_tx: UnboundedSender<VideoEvent>,
) -> anyhow::Result<()> {
    // Viewers announce themselves with a hello Event.
    let tx = video_tx.clone();
    client
        .subscribe(addr.video_hello().as_str(), move |value, _address| {
            if let Some(presence) = decode::<Presence>(&value) {
                let _ = tx.send(VideoEvent::ViewerPresent(presence));
            }
        })
        .await?;

    // Signaling addressed to us. The address ends with the recipient session;
    // we process only messages addressed to us, and ignore echoes of our own.
    //
    // The session is read live rather than captured, because the relay issues a
    // new one on every reconnect. Comparing against the session we had at
    // startup would silently drop every answer and every ICE candidate from the
    // first reconnect onwards, and video would never negotiate again while the
    // control plane carried on looking perfectly healthy.
    let me = client.clone();
    let tx = video_tx;
    client
        .subscribe(
            addr.video_signal_pattern().as_str(),
            move |value, address| {
                let Some(current) = me.session_id() else {
                    return;
                };
                let recipient = address.rsplit('/').next().unwrap_or_default();
                if recipient != current {
                    return;
                }
                if let Some(message) = decode::<SignalMessage>(&value) {
                    if message.from() == current {
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

/// Read a `Value` back as a plain string, for payloads that are just a token.
pub(crate) fn as_string(value: &Value) -> Option<String> {
    to_json(value).as_str().map(str::to_string)
}

fn as_f64(value: &Value) -> Option<f64> {
    to_json(value).as_f64()
}
