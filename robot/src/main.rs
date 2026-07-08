//! Onboard agent for the HeatSync Labs telepresence robot.
//!
//! Brings up the relay link first, then the motion, telemetry, video, and audio
//! planes. Each plane runs as its own task and communicates over channels, so a
//! failure or restart in one does not take down the others. Motion safety is
//! enforced locally by a watchdog, independent of the network.

// Reduced-feature dev builds (off-Pi, without motor-hat/video) intentionally
// leave some hardware and video fields unconsumed. The default Pi build uses
// them all.
#![cfg_attr(not(all(feature = "motor-hat", feature = "video")), allow(dead_code))]

mod audio;
mod config;
mod link;
mod motion;
mod protocol;
mod telemetry;
#[cfg(feature = "video")]
mod video;

use crate::config::Config;
use crate::motion::MotionCommand;
use tokio::sync::mpsc;
use tokio::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let cfg = Config::from_env()?;
    tracing::info!(robot_id = %cfg.robot_id, relay = %cfg.clasp_url, "starting telepresence robot");

    // Motion first: building the backend proves the motor hardware is reachable
    // before we advertise the robot as online.
    let (motion_tx, motion_rx) = mpsc::unbounded_channel();
    let motion = motion::spawn(&cfg, motion_rx)?;

    // Video events flow from the link layer to the video task. The channel
    // exists even when video is compiled out, so the link layer stays uniform.
    let (video_tx, video_rx) = mpsc::unbounded_channel();

    let link = link::connect(&cfg, motion_tx.clone(), video_tx).await?;
    tracing::info!(session = %link.session, "connected to relay");

    telemetry::spawn(
        link.client.clone(),
        link.addr.clone(),
        motion.speeds.clone(),
        motion.estopped.clone(),
    );

    #[cfg(feature = "video")]
    video::spawn(
        link.client.clone(),
        link.addr.clone(),
        cfg.clone(),
        link.session.clone(),
        video_rx,
    );
    #[cfg(not(feature = "video"))]
    {
        drop(video_rx);
        tracing::info!("video plane disabled (built without the 'video' feature)");
    }

    // Best-effort: never blocks startup, never fatal.
    audio::spawn_best_effort();

    tracing::info!("robot online");
    tokio::signal::ctrl_c().await?;

    // Fail safe on the way out: stop the motors and drop offline promptly.
    tracing::info!("shutdown requested; stopping motors");
    let _ = motion_tx.send(MotionCommand::EStop(true));
    let _ = link.set_offline().await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,hsl_telepresence_robot=debug"));
    fmt().with_env_filter(filter).init();
}
