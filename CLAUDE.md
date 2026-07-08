# CLAUDE.md

Project rules and context for anyone (human or agent) working in this repository.

## Hard rules

These are not preferences. Do not violate them.

### Attribution

- Never add Claude, any AI, or any assistant as an author or co-author of a git commit.
- Never add "Generated with", "Co-Authored-By" AI lines, or any similar AI attribution to commit messages, pull request descriptions, code comments, or documentation.
- Commits are authored by the human running the work. Nothing else.

### Writing and language

- Do not use em-dashes. Do not use double hyphens as sentence punctuation. Use a period, a comma, or a rephrase instead. (Command-line flags such as `--release` are code and are fine.)
- Do not use the stock phrasing that signals machine-written text. Avoid "delve", "in today's fast-paced world", "it's worth noting that", "at the end of the day", "unlock", "leverage" as filler, "seamless", "robust" as a reflex, "elevate", "game-changer", and sentence-opening "Moreover"/"Furthermore" chains.
- Write plainly. State the thing. Cut the throat-clearing sentence before the real sentence.
- No emojis in code, commits, UI copy, comments, or documentation. Where the interface needs a symbol, use a real icon (an SVG or an icon set), never an emoji standing in for one.

### Design

- No cliche AI design aesthetics. No dark-neon-on-black, no glowing purple gradients, no generic glassmorphism as a default look.
- Design deliberately. Pick a palette and type system that fit a piece of control hardware: legible, calm, high contrast where it matters (the video feed and the motor controls), restrained everywhere else.
- Iconography over decoration.

### Code

- Smart separation of concerns. Transport, hardware drivers, video, and application logic are separate modules with clear boundaries. A change to how motors are driven must not require touching the video pipeline.
- Well-designed code over clever code. Name things for what they do. Keep functions small enough to hold in your head.
- Fail loud at boundaries, degrade gracefully for optional features (see audio below).

## Project

A telepresence robot. This is a modern rebuild of an older design (two AWS Compute Blog posts by Moheeb Zara) using current tooling.

### Hardware

- Raspberry Pi 3 Model B+ (the robot's onboard computer).
- Adafruit DC and Stepper Motor HAT (I2C, PCA9685 based) for drive motors.
- USB stereoscopic camera that presents two feeds side by side in one video stream.
- Microphone and audio: planned but not present yet. All audio support must be optional. The robot and the web app must start and run fully with no audio hardware attached.

### Software shape

- Everything running on the Pi is Rust. Preferred, not optional.
- Connectivity uses CLASP (clasp.to) over its public relay (relay.clasp.to) for control and, where feasible, the video path. Prefer peer to peer / low latency.
- Hardware control on the Pi: the Adafruit Motor HAT is on the Pi's own I2C bus. Investigate whether Conduyt (conduyt.io, github.com/virgilvox/conduyt) fits this path before assuming it does; Conduyt is a host-to-device protocol aimed at microcontrollers, and the Pi 3 is a Linux host, not a Conduyt firmware target.
- The web app is a Vue.js static site that controls the robot and opens its video feed. It runs elsewhere, not on the Pi, and is deployed to DigitalOcean App Platform.

### Versions

CLASP and Conduyt are both under active local development on branches that may diverge from what is published. Always target the latest published library versions (crates.io, npm), not whatever a local working branch happens to contain. Do not modify the local CLASP or Conduyt repositories; they are references only.
