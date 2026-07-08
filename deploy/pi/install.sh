#!/usr/bin/env bash
# Provision a Raspberry Pi 3 Model B+ (64-bit Raspberry Pi OS, Bookworm) to build
# and run the telepresence robot agent. Run from the repo root on the Pi.
set -euo pipefail

echo "Installing system packages..."
sudo apt-get update
sudo apt-get install -y \
  build-essential pkg-config \
  libssl-dev libudev-dev \
  i2c-tools \
  gstreamer1.0-tools \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-nice

echo "Enabling I2C (if not already enabled)..."
if command -v raspi-config >/dev/null 2>&1; then
  sudo raspi-config nonint do_i2c 0 || true
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "Installing Rust toolchain..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

echo "Building the robot agent (release)..."
cargo build --release --manifest-path robot/Cargo.toml

echo
echo "Build complete: robot/target/release/robot"
echo "Next:"
echo "  sudo cp robot/target/release/robot /usr/local/bin/hsl-robot"
echo "  sudo mkdir -p /etc/hsl-telepresence"
echo "  sudo cp deploy/pi/robot.env.example /etc/hsl-telepresence/robot.env   # then edit"
echo "  sudo cp deploy/pi/robot.service /etc/systemd/system/hsl-robot.service"
echo "  sudo systemctl daemon-reload && sudo systemctl enable --now hsl-robot"
echo
echo "Verify the HAT is on the bus with: i2cdetect -y 1   (expect 0x60)"
