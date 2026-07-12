//! PCA9685 register logic for the Adafruit Motor HAT, generic over any I2C bus.
//! Kept separate from the Linux `I2cdev` backend so it builds on any host and
//! the register sequencing is unit-tested off-device.
//!
//! Each DC motor uses three PCA9685 channels: a PWM channel carrying the speed
//! duty cycle, and two direction channels (IN1/IN2) driven to steady logic
//! HIGH or LOW. The channel numbers follow Adafruit's own library.
//!
//! Direction pins are the subtle part. The PCA9685 has a full-ON bit and a
//! full-OFF bit per channel, and full-OFF takes precedence: once a channel's
//! full-OFF bit is set, setting full-ON alone will NOT drive it high. The
//! `pwm-pca9685` crate's `set_channel_full_on`/`set_channel_full_off` each touch
//! only one side and never clear the other. So to drive a pin HIGH we set
//! full-ON and clear the full-OFF bit; to drive it LOW we set full-OFF and clear
//! the full-ON bit. Missing this leaves the direction pins stuck low after the
//! first coast, and the motors never turn.

use embedded_hal::i2c::I2c;
use pwm_pca9685::{Channel, Pca9685};

/// The three PCA9685 channels that make up one DC motor output.
pub(super) struct MotorChannels {
    pub pwm: Channel,
    pub in1: Channel,
    pub in2: Channel,
}

// Adafruit channel map. M1 is the left wheel, M2 the right.
pub(super) const M1: MotorChannels = MotorChannels {
    pwm: Channel::C8,
    in2: Channel::C9,
    in1: Channel::C10,
};
pub(super) const M2: MotorChannels = MotorChannels {
    pwm: Channel::C13,
    in2: Channel::C12,
    in1: Channel::C11,
};

const DUTY_MAX: f64 = 4095.0;

/// PWM duty for a signed speed. Non-finite input (which should never arrive over
/// JSON) is treated as a stop rather than an undefined cast.
fn duty_for(speed: f64) -> u16 {
    let s = if speed.is_finite() {
        speed.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    (s.abs() * DUTY_MAX).round() as u16
}

/// Drive one motor at the given signed speed (-1.0..1.0). Sets PWM duty, then
/// the direction pins. Zero (or non-finite) speed releases the motor to coast.
pub(super) fn drive_motor<I2C: I2c>(
    pwm: &mut Pca9685<I2C>,
    m: &MotorChannels,
    speed: f64,
) -> anyhow::Result<()> {
    pwm.set_channel_on_off(m.pwm, 0, duty_for(speed))
        .map_err(|e| anyhow::anyhow!("pca9685 pwm: {e:?}"))?;

    let s = if speed.is_finite() { speed } else { 0.0 };
    if s > 0.0 {
        set_pin_high(pwm, m.in1)?; // forward
        set_pin_low(pwm, m.in2)?;
    } else if s < 0.0 {
        set_pin_high(pwm, m.in2)?; // reverse
        set_pin_low(pwm, m.in1)?;
    } else {
        release(pwm, m)?;
    }
    Ok(())
}

/// Release a motor: both direction pins low, so it coasts.
pub(super) fn release<I2C: I2c>(pwm: &mut Pca9685<I2C>, m: &MotorChannels) -> anyhow::Result<()> {
    set_pin_low(pwm, m.in1)?;
    set_pin_low(pwm, m.in2)?;
    Ok(())
}

/// Drive a channel to steady logic HIGH: set full-ON and clear the full-OFF bit
/// (which otherwise wins). Order matters only for the sub-millisecond transient;
/// the resulting register state is full-ON set, full-OFF clear.
fn set_pin_high<I2C: I2c>(pwm: &mut Pca9685<I2C>, ch: Channel) -> anyhow::Result<()> {
    pwm.set_channel_full_on(ch, 0)
        .map_err(|e| anyhow::anyhow!("pca9685 full_on: {e:?}"))?;
    pwm.set_channel_off(ch, 0)
        .map_err(|e| anyhow::anyhow!("pca9685 clear full_off: {e:?}"))?;
    Ok(())
}

/// Drive a channel to steady logic LOW: set full-OFF and clear the full-ON bit.
fn set_pin_low<I2C: I2c>(pwm: &mut Pca9685<I2C>, ch: Channel) -> anyhow::Result<()> {
    pwm.set_channel_full_off(ch)
        .map_err(|e| anyhow::anyhow!("pca9685 full_off: {e:?}"))?;
    pwm.set_channel_on(ch, 0)
        .map_err(|e| anyhow::anyhow!("pca9685 clear full_on: {e:?}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_hal::i2c::{ErrorType, I2c, Operation, SevenBitAddress};
    use pwm_pca9685::{Address, Pca9685};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Writes = Rc<RefCell<Vec<Vec<u8>>>>;

    /// I2C bus that records every write payload so tests can assert exactly
    /// which registers were written with which bytes.
    #[derive(Clone, Default)]
    struct Recorder {
        writes: Writes,
    }

    impl ErrorType for Recorder {
        type Error = embedded_hal::i2c::ErrorKind;
    }

    impl I2c<SevenBitAddress> for Recorder {
        fn transaction(
            &mut self,
            _address: SevenBitAddress,
            operations: &mut [Operation<'_>],
        ) -> Result<(), Self::Error> {
            for op in operations {
                if let Operation::Write(bytes) = op {
                    self.writes.borrow_mut().push(bytes.to_vec());
                }
            }
            Ok(())
        }
    }

    // Channel N: ON_L = 0x06 + 4N, OFF_L = 0x08 + 4N. Values are little-endian.
    fn on_reg(n: u8) -> u8 {
        0x06 + 4 * n
    }
    fn off_reg(n: u8) -> u8 {
        0x08 + 4 * n
    }
    const FULL_BIT_HI: u8 = 0x10; // bit 12 of a u16, high byte

    fn recorder() -> (Pca9685<Recorder>, Writes) {
        let rec = Recorder::default();
        let writes = rec.writes.clone();
        let pwm = Pca9685::new(rec, Address::default()).expect("construct");
        (pwm, writes)
    }

    // The regression test for the full-OFF-precedence bug: driving a pin HIGH
    // must clear its full-OFF bit, otherwise the motor never turns.
    #[test]
    fn driving_a_pin_high_clears_the_full_off_bit() {
        let (mut pwm, writes) = recorder();
        set_pin_high(&mut pwm, Channel::C10).unwrap();
        let w = writes.borrow();
        assert!(
            w.contains(&vec![on_reg(10), 0x00, FULL_BIT_HI]),
            "expected full-ON write, got {w:?}"
        );
        assert!(
            w.contains(&vec![off_reg(10), 0x00, 0x00]),
            "expected full-OFF cleared, got {w:?}"
        );
    }

    #[test]
    fn forward_sets_in1_high_and_in2_low() {
        let (mut pwm, writes) = recorder();
        drive_motor(&mut pwm, &M1, 0.5).unwrap();
        let w = writes.borrow();
        // IN1 (C10) high: full-on set, full-off cleared.
        assert!(w.contains(&vec![on_reg(10), 0x00, FULL_BIT_HI]));
        assert!(w.contains(&vec![off_reg(10), 0x00, 0x00]));
        // IN2 (C9) low: full-off set, full-on cleared.
        assert!(w.contains(&vec![off_reg(9), 0x00, FULL_BIT_HI]));
        assert!(w.contains(&vec![on_reg(9), 0x00, 0x00]));
        // PWM (C8) duty for 0.5 = round(0.5*4095) = 2048 = 0x0800.
        assert!(w.contains(&vec![on_reg(8), 0x00, 0x00, 0x00, 0x08]));
    }

    #[test]
    fn reverse_sets_in2_high_and_in1_low() {
        let (mut pwm, writes) = recorder();
        drive_motor(&mut pwm, &M1, -0.5).unwrap();
        let w = writes.borrow();
        assert!(w.contains(&vec![on_reg(9), 0x00, FULL_BIT_HI])); // in2 full-on
        assert!(w.contains(&vec![off_reg(10), 0x00, FULL_BIT_HI])); // in1 full-off
    }

    #[test]
    fn release_pulls_both_direction_pins_low() {
        let (mut pwm, writes) = recorder();
        release(&mut pwm, &M1).unwrap();
        let w = writes.borrow();
        assert!(w.contains(&vec![off_reg(10), 0x00, FULL_BIT_HI])); // in1 full-off
        assert!(w.contains(&vec![off_reg(9), 0x00, FULL_BIT_HI])); // in2 full-off
    }

    #[test]
    fn duty_maps_speed_magnitude() {
        assert_eq!(duty_for(0.0), 0);
        assert_eq!(duty_for(1.0), 4095);
        assert_eq!(duty_for(-1.0), 4095);
        assert_eq!(duty_for(f64::NAN), 0);
        assert_eq!(duty_for(2.0), 4095); // clamped
    }
}
