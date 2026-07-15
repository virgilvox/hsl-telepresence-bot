//! Linux `I2cdev` backend for the Adafruit DC and Stepper Motor HAT. The
//! register logic lives in [`super::hat_driver`]; this file only owns the
//! Pi-specific I2C device and the chip setup. M1 drives the left wheel, M2 the
//! right.

use super::hat_driver::{self, M1, M2};
use super::{MotorBackend, WheelSpeeds};
use linux_embedded_hal::I2cdev;
use pwm_pca9685::{Address, Pca9685};

/// ~1526 Hz PWM. prescale = round(25MHz / (4096 * freq)) - 1. Must be set while
/// the oscillator is asleep, i.e. before `enable`.
const PWM_PRESCALE: u8 = 3;

pub struct HatBackend {
    pwm: Pca9685<I2cdev>,
    invert_left: bool,
    invert_right: bool,
}

impl HatBackend {
    pub fn new(
        bus: &str,
        address: u8,
        invert_left: bool,
        invert_right: bool,
    ) -> anyhow::Result<Self> {
        let dev = I2cdev::new(bus)?;
        let mut pwm = Pca9685::new(dev, hat_address(address))
            .map_err(|e| anyhow::anyhow!("pca9685 init: {e:?}"))?;
        pwm.set_prescale(PWM_PRESCALE)
            .map_err(|e| anyhow::anyhow!("pca9685 set_prescale: {e:?}"))?;
        pwm.enable()
            .map_err(|e| anyhow::anyhow!("pca9685 enable: {e:?}"))?;

        let mut backend = Self {
            pwm,
            invert_left,
            invert_right,
        };
        // Start from a known safe state.
        backend.coast()?;
        Ok(backend)
    }
}

impl MotorBackend for HatBackend {
    fn set_wheels(&mut self, speeds: WheelSpeeds) -> anyhow::Result<()> {
        // Apply per-motor inversion for how this chassis is wired (the right
        // motor is usually mounted mirror-imaged from the left).
        let left = if self.invert_left { -speeds.left } else { speeds.left };
        let right = if self.invert_right { -speeds.right } else { speeds.right };
        hat_driver::drive_motor(&mut self.pwm, &M1, left)?;
        hat_driver::drive_motor(&mut self.pwm, &M2, right)?;
        Ok(())
    }

    fn coast(&mut self) -> anyhow::Result<()> {
        hat_driver::release(&mut self.pwm, &M1)?;
        hat_driver::release(&mut self.pwm, &M2)?;
        Ok(())
    }
}

/// Convert an I2C address into the PCA9685 pin-strap tuple the driver expects.
/// The base address is 0x40; the bits above that select the six address pins
/// A5..A0. The Adafruit default 0x60 sets A5 only.
fn hat_address(address: u8) -> Address {
    let pins = address.wrapping_sub(0x40);
    Address::from((
        pins & 0b100000 != 0,
        pins & 0b010000 != 0,
        pins & 0b001000 != 0,
        pins & 0b000100 != 0,
        pins & 0b000010 != 0,
        pins & 0b000001 != 0,
    ))
}
