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
mod health;
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

    // Wait for a shutdown signal, or for either plane the robot cannot work
    // without to give up. Neither is supposed to happen, and both have the same
    // shape when they do: the process stays up, systemd sees a healthy service,
    // and the robot sits there looking online while ignoring every command.
    // That is the worst of the available failures, so each one exits instead
    // and lets the supervisor rebuild the whole thing.
    let health = link.health.clone();
    tokio::select! {
        _ = shutdown_signal() => {
            tracing::info!("shutdown requested; stopping motors");
        }
        result = &mut motion.task => {
            tracing::error!(?result, "motion task ended; exiting so the supervisor restarts us");
            // The backend coasts as it is dropped, so the wheels are already
            // stopped by the time we get here.
            go_offline(&link).await;
            std::process::exit(1);
        }
        _ = health.watch_until_dead() => {
            tracing::error!("relay link stopped delivering; exiting so the supervisor restarts us");
            // Nothing can reach us to ask for a stop, so stop first and explain
            // second. The drive watchdog has already coasted the wheels on the
            // silence; this latches them for the moment we have left.
            let _ = motion_tx.send(MotionCommand::EStop(true));
            go_offline(&link).await;
            std::process::exit(1);
        }
    }

    // Fail safe on the way out: stop the motors and drop offline promptly.
    let _ = motion_tx.send(MotionCommand::EStop(true));
    go_offline(&link).await;

    Ok(())
}

/// Clear `status/online` without letting a wedged link hold the exit open.
///
/// Publishing pushes into a bounded channel whose reader is gone in exactly the
/// case that brings us here, so an unguarded await would hang forever at the
/// one moment leaving promptly matters. On a shutdown that in turn means
/// systemd waiting out its stop timeout before resorting to SIGKILL.
async fn go_offline(link: &link::Link) {
    const GRACE: Duration = Duration::from_secs(2);
    match tokio::time::timeout(GRACE, link.set_offline()).await {
        // Let the frame leave the socket before the process does.
        Ok(Ok(())) => tokio::time::sleep(Duration::from_millis(200)).await,
        Ok(Err(err)) => tracing::warn!(%err, "could not clear online status"),
        Err(_) => tracing::warn!("timed out clearing online status"),
    }
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
    // Filters match the *target*, which for this binary is its bin name,
    // `robot`. The package name is `hsl-telepresence-robot` and matches no
    // target at all, so a directive naming it does not raise the level of
    // anything: it silently turns every debug line in the agent off, including
    // the ones that report a link going quiet.
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,robot=debug"));
    fmt().with_env_filter(filter).init();
}
