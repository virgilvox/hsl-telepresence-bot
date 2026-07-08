//! Real motor backend for the Adafruit DC and Stepper Motor HAT.
//!
//! The HAT is a PCA9685 16-channel PWM chip at I2C address 0x60 driving
//! TB6612-style H-bridges. Each DC motor uses three channels: one PWM channel
//! carrying the speed duty cycle, and two direction channels driven fully on or
//! fully off. The channel assignments below follow Adafruit's own library.
//!
//! Two motors are wired for differential drive: M1 is the left wheel, M2 the
//! right. The other two motor slots (M3, M4) are mapped for completeness.

use super::{MotorBackend, WheelSpeeds};
use linux_embedded_hal::I2cdev;
use pwm_pca9685::{Address, Channel, Pca9685};

/// The three PCA9685 channels that make up one DC motor output.
struct MotorChannels {
    pwm: Channel,
    in1: Channel,
    in2: Channel,
}

// Adafruit channel map. PWM carries speed; IN1/IN2 set direction.
const M1: MotorChannels = MotorChannels {
    pwm: Channel::C8,
    in2: Channel::C9,
    in1: Channel::C10,
};
const M2: MotorChannels = MotorChannels {
    pwm: Channel::C13,
    in2: Channel::C12,
    in1: Channel::C11,
};

const DUTY_MAX: f64 = 4095.0;

/// ~1526 Hz PWM. prescale = round(25MHz / (4096 * freq)) - 1. The HAT is happy
/// anywhere in the low-kHz range. Must be set while the oscillator is asleep,
/// i.e. before `enable`.
const PWM_PRESCALE: u8 = 3;

pub struct HatBackend {
    pwm: Pca9685<I2cdev>,
}

impl HatBackend {
    pub fn new(bus: &str, address: u8) -> anyhow::Result<Self> {
        let dev = I2cdev::new(bus)?;
        let mut pwm = Pca9685::new(dev, hat_address(address))
            .map_err(|e| anyhow::anyhow!("pca9685 init: {e:?}"))?;
        pwm.set_prescale(PWM_PRESCALE)
            .map_err(|e| anyhow::anyhow!("pca9685 set_prescale: {e:?}"))?;
        pwm.enable()
            .map_err(|e| anyhow::anyhow!("pca9685 enable: {e:?}"))?;

        let mut backend = Self { pwm };
        // Start from a known safe state.
        backend.coast()?;
        Ok(backend)
    }

    fn drive_motor(&mut self, m: &MotorChannels, speed: f64) -> anyhow::Result<()> {
        let speed = speed.clamp(-1.0, 1.0);
        let duty = (speed.abs() * DUTY_MAX).round() as u16;

        self.pwm
            .set_channel_on_off(m.pwm, 0, duty)
            .map_err(|e| anyhow::anyhow!("pca9685 pwm: {e:?}"))?;

        if speed > 0.0 {
            self.set_direction(m.in1, m.in2)?; // forward: IN1 high, IN2 low
        } else if speed < 0.0 {
            self.set_direction(m.in2, m.in1)?; // reverse: IN2 high, IN1 low
        } else {
            self.release(m)?; // coast
        }
        Ok(())
    }

    fn set_direction(&mut self, high: Channel, low: Channel) -> anyhow::Result<()> {
        self.pwm
            .set_channel_full_on(high, 0)
            .map_err(|e| anyhow::anyhow!("pca9685 dir high: {e:?}"))?;
        self.pwm
            .set_channel_full_off(low)
            .map_err(|e| anyhow::anyhow!("pca9685 dir low: {e:?}"))?;
        Ok(())
    }

    fn release(&mut self, m: &MotorChannels) -> anyhow::Result<()> {
        self.pwm
            .set_channel_full_off(m.in1)
            .map_err(|e| anyhow::anyhow!("pca9685 release in1: {e:?}"))?;
        self.pwm
            .set_channel_full_off(m.in2)
            .map_err(|e| anyhow::anyhow!("pca9685 release in2: {e:?}"))?;
        Ok(())
    }
}

impl MotorBackend for HatBackend {
    fn set_wheels(&mut self, speeds: WheelSpeeds) -> anyhow::Result<()> {
        self.drive_motor(&M1, speeds.left)?;
        self.drive_motor(&M2, speeds.right)?;
        Ok(())
    }

    fn coast(&mut self) -> anyhow::Result<()> {
        self.release(&M1)?;
        self.release(&M2)?;
        Ok(())
    }
}

/// Convert an I2C address into the PCA9685 pin-strap tuple the driver expects.
/// The base address is 0x40; the high bits above that select the six address
/// pins A5..A0. The Adafruit default 0x60 sets A5 only.
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
