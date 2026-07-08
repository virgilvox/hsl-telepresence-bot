//! Publishes robot state to the relay. High-rate motor duty goes out as a
//! Stream; the latched e-stop mirror goes out as a status Param so the operator
//! console reflects it even after a reconnect.

use crate::motion::WheelSpeeds;
use crate::protocol::Addresses;
use clasp_client::Clasp;
use std::sync::Arc;
use tokio::sync::watch;
use tokio::time::Duration;

pub fn spawn(
    client: Arc<Clasp>,
    addr: Addresses,
    speeds: watch::Receiver<WheelSpeeds>,
    mut estopped: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_millis(200));
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let s = *speeds.borrow();
                    let payload = crate::link::to_value(
                        serde_json::json!({ "left": s.left, "right": s.right }),
                    );
                    if let Err(err) = client.stream(addr.tel("motors").as_str(), payload).await {
                        tracing::debug!(%err, "telemetry publish failed");
                    }
                }
                changed = estopped.changed() => {
                    if changed.is_err() {
                        break; // motion task gone
                    }
                    let value = *estopped.borrow();
                    if let Err(err) = client.set(addr.status("estop").as_str(), value).await {
                        tracing::debug!(%err, "estop status publish failed");
                    }
                }
            }
        }
    });
}
