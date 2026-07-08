//! Motion plane. This is the only module that touches the I2C bus and the
//! motors. Everything else asks it to move by sending [`MotionCommand`] values
//! over a channel; the concrete motor hardware never leaks out of here.
//!
//! Safety lives here, not in the network layer: a watchdog coasts the motors if
//! no drive command arrives within the configured window, and a latched e-stop
//! overrides all motion until it is explicitly cleared. Loss of the relay
//! connection therefore stops the robot rather than freezing its last command.

mod drive;
#[cfg(not(feature = "motor-hat"))]
mod mock;
#[cfg(feature = "motor-hat")]
mod pca9685;

pub use drive::{spawn, MotionCommand};

use crate::config::Config;

/// Normalized wheel demand. Each value is a throttle in -1.0 (full reverse) to
/// 1.0 (full forward).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WheelSpeeds {
    pub left: f64,
    pub right: f64,
}

/// A hardware-agnostic two-wheel motor backend. The drive controller owns one
/// and never cares which concrete type it is.
pub trait MotorBackend: Send {
    /// Apply left/right wheel throttles. Implementations clamp to -1.0..1.0.
    fn set_wheels(&mut self, speeds: WheelSpeeds) -> anyhow::Result<()>;
    /// Release the motors so they spin down freely. Used by the watchdog and
    /// e-stop paths, so it must be cheap and must not fail spuriously.
    fn coast(&mut self) -> anyhow::Result<()>;
}

/// Construct the motor backend for this build. With the `motor-hat` feature the
/// real HAT is required and a failure to reach it is fatal, because motion is
/// not an optional subsystem. Without the feature (off-Pi development), a mock
/// backend that only logs is used.
#[cfg(feature = "motor-hat")]
fn build_backend(cfg: &Config) -> anyhow::Result<Box<dyn MotorBackend>> {
    let backend = pca9685::HatBackend::new(&cfg.i2c_bus, cfg.i2c_address)?;
    tracing::info!(bus = %cfg.i2c_bus, address = format!("{:#x}", cfg.i2c_address), "motor HAT ready");
    Ok(Box::new(backend))
}

#[cfg(not(feature = "motor-hat"))]
fn build_backend(_cfg: &Config) -> anyhow::Result<Box<dyn MotorBackend>> {
    tracing::warn!("motor-hat feature disabled: using mock motor backend (no motion)");
    Ok(Box::new(mock::MockBackend::default()))
}
