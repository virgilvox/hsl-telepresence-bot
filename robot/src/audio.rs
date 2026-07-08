//! Optional audio plane.
//!
//! No microphone or speaker is fitted to the robot yet. This module exists so
//! that adding one later is a contained change and never a prerequisite for the
//! robot starting. The task is spawned best-effort: any failure is logged and
//! swallowed, and the rest of the system is unaffected.

/// Spawn the audio task. Returns immediately. When audio hardware is added,
/// wire capture and playback here and gate it on a device being present.
pub fn spawn_best_effort() {
    tokio::spawn(async {
        if let Err(err) = run().await {
            tracing::warn!(%err, "audio plane failed to start; continuing without audio");
        }
    });
}

async fn run() -> anyhow::Result<()> {
    // Placeholder. No capture or playback device is configured. When a mic and
    // speaker exist, negotiate a second WebRTC track here and mix it into the
    // existing peer connection rather than opening a separate transport.
    tracing::info!("audio: no capture or playback device configured; audio disabled");
    Ok(())
}
