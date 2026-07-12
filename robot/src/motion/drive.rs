//! Differential-drive kinematics and the safety watchdog. Owns the motor
//! backend on a dedicated task and is the sole writer to it.

use super::{build_backend, WheelSpeeds};
use crate::config::Config;
use crate::protocol::DriveCommand;
use tokio::sync::{mpsc, watch};
use tokio::time::{Duration, Instant};

/// Messages the motion task accepts. Produced by the link layer from inbound
/// CLASP signals and by the orchestrator on shutdown.
#[derive(Debug, Clone)]
pub enum MotionCommand {
    Drive(DriveCommand),
    /// Latched emergency stop. While true, all drive commands are ignored and
    /// the motors are held coasting.
    EStop(bool),
    /// Update the speed ceiling, 0.0 to 1.0.
    SetMaxSpeed(f64),
}

/// Handle returned to the rest of the system. Exposes a read-only view of the
/// current wheel demand for telemetry.
pub struct MotionHandle {
    pub speeds: watch::Receiver<WheelSpeeds>,
    pub estopped: watch::Receiver<bool>,
}

/// Build the backend and spawn the motion task. The caller keeps the sending
/// half of `rx` to issue commands.
pub fn spawn(
    cfg: &Config,
    rx: mpsc::UnboundedReceiver<MotionCommand>,
) -> anyhow::Result<MotionHandle> {
    let backend = build_backend(cfg)?;
    let (speeds_tx, speeds_rx) = watch::channel(WheelSpeeds::default());
    let (estop_tx, estop_rx) = watch::channel(false);

    let timeout = cfg.drive_timeout;
    let max_speed = cfg.max_speed;

    tokio::spawn(async move {
        run(backend, rx, speeds_tx, estop_tx, timeout, max_speed).await;
    });

    Ok(MotionHandle {
        speeds: speeds_rx,
        estopped: estop_rx,
    })
}

async fn run(
    mut backend: Box<dyn super::MotorBackend>,
    mut rx: mpsc::UnboundedReceiver<MotionCommand>,
    speeds_tx: watch::Sender<WheelSpeeds>,
    estop_tx: watch::Sender<bool>,
    timeout: Duration,
    initial_max_speed: f64,
) {
    let mut estopped = false;
    let mut max_speed = initial_max_speed;
    let mut last_command = Instant::now();
    let mut coasting = true;

    // The watchdog fires on this cadence. It is finer than the timeout so the
    // coast happens close to the deadline rather than a full timeout late.
    let tick = (timeout / 4).max(Duration::from_millis(20));
    let mut watchdog = tokio::time::interval(tick);

    loop {
        tokio::select! {
            message = rx.recv() => {
                let Some(message) = message else {
                    // Senders dropped: shut the task down, coasting first.
                    let _ = backend.coast();
                    break;
                };
                match message {
                    MotionCommand::Drive(cmd) => {
                        last_command = Instant::now();
                        if estopped {
                            continue;
                        }
                        let speeds = mix(cmd.throttle, cmd.steer, max_speed);
                        apply(&mut backend, speeds, &speeds_tx, &mut coasting);
                    }
                    MotionCommand::EStop(value) => {
                        estopped = value;
                        let _ = estop_tx.send(value);
                        if value {
                            coast(&mut backend, &speeds_tx, &mut coasting);
                            tracing::warn!("e-stop engaged");
                        } else {
                            tracing::info!("e-stop cleared");
                        }
                    }
                    MotionCommand::SetMaxSpeed(value) => {
                        max_speed = value.clamp(0.0, 1.0);
                        tracing::info!(max_speed, "max speed updated");
                    }
                }
            }
            _ = watchdog.tick() => {
                let stale = last_command.elapsed() >= timeout;
                if (stale || estopped) && !coasting {
                    coast(&mut backend, &speeds_tx, &mut coasting);
                    if stale && !estopped {
                        tracing::warn!("drive watchdog: no command within timeout, coasting");
                    }
                }
            }
        }
    }
}

fn apply(
    backend: &mut Box<dyn super::MotorBackend>,
    speeds: WheelSpeeds,
    speeds_tx: &watch::Sender<WheelSpeeds>,
    coasting: &mut bool,
) {
    match backend.set_wheels(speeds) {
        Ok(()) => {
            *coasting = false;
            let _ = speeds_tx.send(speeds);
        }
        Err(err) => {
            // Fail safe: a failed drive write leaves the motors in an unknown
            // state, so try to stop them. coast() only marks us coasting if the
            // stop itself succeeds, so the watchdog keeps retrying otherwise.
            tracing::error!(%err, "failed to drive motors, coasting");
            coast(backend, speeds_tx, coasting);
        }
    }
}

fn coast(
    backend: &mut Box<dyn super::MotorBackend>,
    speeds_tx: &watch::Sender<WheelSpeeds>,
    coasting: &mut bool,
) {
    match backend.coast() {
        Ok(()) => {
            *coasting = true;
            let _ = speeds_tx.send(WheelSpeeds::default());
        }
        Err(err) => tracing::error!(%err, "failed to coast motors"),
    }
}

/// Map throttle and steer onto left/right wheel demand, scaled by the speed
/// ceiling. Positive steer turns the robot to its right.
fn mix(throttle: f64, steer: f64, max_speed: f64) -> WheelSpeeds {
    let throttle = throttle.clamp(-1.0, 1.0);
    let steer = steer.clamp(-1.0, 1.0);
    let left = ((throttle + steer) * max_speed).clamp(-1.0, 1.0);
    let right = ((throttle - steer) * max_speed).clamp(-1.0, 1.0);
    WheelSpeeds { left, right }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_forward_is_even() {
        let s = mix(1.0, 0.0, 1.0);
        assert_eq!(s.left, 1.0);
        assert_eq!(s.right, 1.0);
    }

    #[test]
    fn steer_right_slows_right_wheel() {
        let s = mix(0.5, 0.5, 1.0);
        assert!(s.left > s.right);
    }

    #[test]
    fn max_speed_scales_output() {
        let s = mix(1.0, 0.0, 0.5);
        assert_eq!(s.left, 0.5);
        assert_eq!(s.right, 0.5);
    }

    #[test]
    fn output_is_clamped() {
        let s = mix(1.0, 1.0, 1.0);
        assert!(s.left <= 1.0 && s.right >= -1.0);
    }

    // Exercises the running motion task through the mock backend. Only valid
    // when the real HAT is not compiled in (otherwise spawn tries to open I2C).
    #[cfg(not(feature = "motor-hat"))]
    fn test_config() -> Config {
        Config {
            robot_id: "test".into(),
            robot_name: "test".into(),
            clasp_url: String::new(),
            clasp_token: None,
            i2c_bus: String::new(),
            i2c_address: 0x60,
            drive_timeout: Duration::from_secs(10),
            max_speed: 1.0,
            camera_device: String::new(),
            camera_width: 0,
            camera_height: 0,
            camera_fps: 0,
        }
    }

    #[cfg(not(feature = "motor-hat"))]
    #[tokio::test]
    async fn estop_blocks_and_clears() {
        let (tx, rx) = mpsc::unbounded_channel();
        let handle = spawn(&test_config(), rx).unwrap();
        let mut speeds = handle.speeds.clone();

        // A drive command moves the wheels.
        tx.send(MotionCommand::Drive(DriveCommand {
            throttle: 1.0,
            steer: 0.0,
            seq: 0,
            ts: 0,
        }))
        .unwrap();
        speeds.changed().await.unwrap();
        assert!(speeds.borrow().left > 0.0);

        // Engaging e-stop coasts.
        tx.send(MotionCommand::EStop(true)).unwrap();
        speeds.changed().await.unwrap();
        assert_eq!(speeds.borrow().left, 0.0);

        // Drives are ignored while stopped.
        tx.send(MotionCommand::Drive(DriveCommand {
            throttle: 1.0,
            steer: 0.0,
            seq: 1,
            ts: 0,
        }))
        .unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(speeds.borrow().left, 0.0);

        // Clearing e-stop lets drives through again.
        tx.send(MotionCommand::EStop(false)).unwrap();
        tx.send(MotionCommand::Drive(DriveCommand {
            throttle: 1.0,
            steer: 0.0,
            seq: 2,
            ts: 0,
        }))
        .unwrap();
        speeds.changed().await.unwrap();
        assert!(speeds.borrow().left > 0.0);
    }
}
