//! Runtime configuration, read entirely from the environment so the binary
//! stays free of network and hardware concerns. Nothing here does I/O beyond
//! reading env vars.

use anyhow::Context;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Config {
    /// Stable identity of this robot. Forms the `/robot/<id>` address root.
    pub robot_id: String,
    /// Human-readable client name reported to the relay.
    pub robot_name: String,
    /// Relay endpoint. Defaults to the public relay over TLS.
    pub clasp_url: String,
    /// Optional CPSK auth token. Anonymous connection is used when unset.
    pub clasp_token: Option<String>,

    /// I2C bus device path the Motor HAT lives on.
    pub i2c_bus: String,
    /// I2C address of the HAT's PCA9685. Adafruit default is 0x60.
    pub i2c_address: u8,
    /// If no drive command arrives within this window, the motors coast.
    pub drive_timeout: Duration,
    /// Upper bound applied to wheel demand, 0.0 to 1.0.
    pub max_speed: f64,
    /// Reverse the left motor's sense. A differential-drive chassis mounts the
    /// two motors mirror-imaged, so one wheel usually needs inverting for the
    /// robot to drive straight. Set to match how this robot is wired.
    pub invert_left: bool,
    /// Reverse the right motor's sense. See `invert_left`.
    pub invert_right: bool,

    /// V4L2 device for the stereoscopic USB camera.
    pub camera_device: String,
    /// Full side-by-side capture width in pixels.
    pub camera_width: u32,
    /// Capture height in pixels.
    pub camera_height: u32,
    /// Capture framerate.
    pub camera_fps: u32,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            robot_id: env_or("ROBOT_ID", "hsl-bot-1"),
            robot_name: env_or("ROBOT_NAME", "hsl-telepresence-robot"),
            clasp_url: env_or("CLASP_URL", "wss://relay.clasp.to"),
            clasp_token: std::env::var("CLASP_TOKEN").ok().filter(|t| !t.is_empty()),

            i2c_bus: env_or("I2C_BUS", "/dev/i2c-1"),
            i2c_address: parse_u8("I2C_ADDRESS", 0x60)?,
            drive_timeout: Duration::from_millis(parse("DRIVE_TIMEOUT_MS", 400)?),
            max_speed: parse("MAX_SPEED", 1.0f64)?.clamp(0.0, 1.0),
            invert_left: parse_bool("INVERT_LEFT", false),
            invert_right: parse_bool("INVERT_RIGHT", false),

            // Default to the 1280x480 side-by-side mode: it fits the Pi's H264
            // encoder (max 1920 wide) and keeps JPEG decode light. The camera
            // also offers 2560-wide modes, but those exceed the encoder.
            camera_device: env_or("CAMERA_DEVICE", "/dev/video0"),
            camera_width: parse("CAMERA_WIDTH", 1280)?,
            camera_height: parse("CAMERA_HEIGHT", 480)?,
            camera_fps: parse("CAMERA_FPS", 30)?,
        })
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn parse<T>(key: &str, default: T) -> anyhow::Result<T>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    match std::env::var(key) {
        Ok(v) => v
            .parse()
            .with_context(|| format!("invalid value for {key}")),
        Err(_) => Ok(default),
    }
}

/// Parse a boolean env flag. Accepts 1/true/yes/on (any case) as true; anything
/// else, including unset, is the given default.
fn parse_bool(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => default,
    }
}

fn parse_u8(key: &str, default: u8) -> anyhow::Result<u8> {
    match std::env::var(key) {
        Ok(v) => {
            let trimmed = v.trim();
            let parsed = if let Some(hex) = trimmed.strip_prefix("0x") {
                u8::from_str_radix(hex, 16)
            } else {
                trimmed.parse()
            };
            parsed.with_context(|| format!("invalid value for {key}"))
        }
        Err(_) => Ok(default),
    }
}
