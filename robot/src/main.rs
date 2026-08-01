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
mod control;
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
    let mut motion = motion::spawn(&cfg, motion_rx)?;

    // Video events flow from the link layer to the video task. The channel
    // exists even when video is compiled out, so the link layer stays uniform.
    let (video_tx, video_rx) = mpsc::unbounded_channel();

    // Decides which of several connected operators is allowed to drive. Built
    // before the link because the inbound command path consults it.
    let (arbiter, driver_rx) = control::Arbiter::new();

    let link = link::connect(&cfg, motion_tx.clone(), video_tx, arbiter.clone()).await?;
    tracing::info!(session = %link.session, "connected to relay");

    control::spawn(link.client.clone(), link.addr.clone(), arbiter, driver_rx);

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

    // Wait for either a shutdown signal or the motion task giving up. The
    // second is not supposed to happen, but a robot whose motion plane has
    // quietly died would otherwise sit there looking online and ignoring
    // every command, which is the worst of the available failures.
    tokio::select! {
        _ = shutdown_signal() => {
            tracing::info!("shutdown requested; stopping motors");
        }
        result = &mut motion.task => {
            tracing::error!(?result, "motion task ended; exiting so the supervisor restarts us");
            // The backend coasts as it is dropped, so the wheels are already
            // stopped by the time we get here.
            let _ = link.set_offline().await;
            tokio::time::sleep(Duration::from_millis(200)).await;
            std::process::exit(1);
        }
    }

    // Fail safe on the way out: stop the motors and drop offline promptly.
    let _ = motion_tx.send(MotionCommand::EStop(true));
    let _ = link.set_offline().await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    Ok(())
}

/// Resolve when the service is asked to stop.
///
/// systemd stops units with SIGTERM, so waiting only on SIGINT means the
/// shutdown above never runs on `systemctl restart`: the motors keep whatever
/// they were doing until the process dies, and `status/online` stays latched
/// true on the relay, telling every console the robot is still there.
async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    match signal(SignalKind::terminate()) {
        Ok(mut term) => {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
        }
        Err(err) => {
            tracing::warn!(%err, "cannot listen for SIGTERM; falling back to SIGINT only");
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,hsl_telepresence_robot=debug"));
    fmt().with_env_filter(filter).init();
}
