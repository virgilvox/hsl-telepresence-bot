//! Mock motor backend for development off the Pi. It performs no I/O; it only
//! records and logs the demanded wheel speeds so the control and video planes
//! can be exercised on a laptop.

use super::{MotorBackend, WheelSpeeds};

#[derive(Default)]
pub struct MockBackend {
    last: WheelSpeeds,
}

impl MotorBackend for MockBackend {
    fn set_wheels(&mut self, speeds: WheelSpeeds) -> anyhow::Result<()> {
        if speeds != self.last {
            tracing::debug!(left = speeds.left, right = speeds.right, "mock motors");
            self.last = speeds;
        }
        Ok(())
    }

    fn coast(&mut self) -> anyhow::Result<()> {
        if self.last != WheelSpeeds::default() {
            tracing::debug!("mock motors coasting");
            self.last = WheelSpeeds::default();
        }
        Ok(())
    }
}
