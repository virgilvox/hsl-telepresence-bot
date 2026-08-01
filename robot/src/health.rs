//! Proof that the relay link actually carries traffic, and a loud death when it
//! stops.
//!
//! The failure this exists for is not a disconnect, which is easy to spot. It
//! is a link that still looks connected: the socket is open, publishes leave,
//! the console renders status and telemetry, and yet nothing inbound ever
//! arrives because the subscriptions were never registered on the relay.
//! `subscribe` is fire-and-forget, so nothing returns an error and nothing is
//! logged. The process looks perfectly healthy to systemd while ignoring every
//! command, including the e-stop, until somebody power-cycles the robot. That
//! has happened here, and from the outside it is indistinguishable from a hang.
//!
//! The check is a round trip through the very path the commands take: write a
//! token to an address only this robot uses, and wait to see it come back. A
//! relay that echoes our own writes is a relay that is delivering to our
//! subscriptions. Every way the link can be useless fails this one check: a
//! closed socket, a session the relay has forgotten, a subscription that never
//! registered, a reconnect that quietly dropped them. The robot does not need
//! to know which one happened in order to know it has to start over.
//!
//! Failing means exiting rather than retrying forever. A link is rebuilt
//! correctly by connecting again from scratch, systemd does that in three
//! seconds, and a robot that is briefly gone is far better than one that is
//! present and deaf.

use crate::protocol::Addresses;
use clasp_client::Clasp;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;
use tokio::time::{Duration, Instant, MissedTickBehavior};

/// How long the first round trip may take before the agent refuses to come up.
/// Generous, because it covers a cold connection over a slow link; it only has
/// to be shorter than an operator's patience.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

/// Gap between round trips once running.
const PROBE_INTERVAL: Duration = Duration::from_secs(15);

/// How long one round trip may take. This also bounds the publish itself: a
/// send into a channel whose reader has died blocks forever, and this is
/// exactly the case where an unguarded await would wedge the very task whose
/// job is to notice.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Consecutive failed round trips before the link is declared dead. Four at a
/// fifteen second cadence is about a minute of a robot nobody can steer, which
/// is long enough to ride out a relay hiccup or a reconnect, and short enough
/// that somebody reaching for the e-stop is not left waiting on it.
const FAILURES_BEFORE_DEAD: u32 = 4;

/// Issues the tokens a round trip is recognized by.
///
/// The probe address is a Param, so subscribing to it hands us whatever the
/// previous run left latched there. A token from that run matching one of ours
/// would be a dead link reporting itself healthy, so every run stamps its
/// tokens with an id of its own.
struct Tokens {
    run: String,
    next: AtomicU64,
}

/// Counts `Tokens` built in this process. See [`Tokens::new`].
static INSTANCES: AtomicU64 = AtomicU64::new(0);

impl Tokens {
    fn new() -> Self {
        // The clock supplies the part that changes across restarts, truncated
        // to stay readable in a log line. The counter covers what the clock
        // cannot: two instances built inside one process, which in practice is
        // only the tests, but a uniqueness property with an exception in it is
        // not one worth relying on.
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| (d.as_nanos() as u64) % 1_000_000_000)
            .unwrap_or(0);
        Self {
            run: format!("{nanos}.{}", INSTANCES.fetch_add(1, Ordering::Relaxed)),
            next: AtomicU64::new(0),
        }
    }

    fn issue(&self) -> String {
        format!("{}:{}", self.run, self.next.fetch_add(1, Ordering::Relaxed))
    }
}

/// Round-trip check on one robot's relay link.
pub struct Health {
    client: Arc<Clasp>,
    address: String,
    tokens: Tokens,
    /// The last token seen coming back. An `Arc` rather than a bare sender so
    /// the subscription callback can hold it without holding the client that
    /// owns the callback.
    echoed: Arc<watch::Sender<String>>,
}

impl Health {
    /// Subscribe to the probe address.
    ///
    /// Call this after the command subscriptions and before [`Health::verify`].
    /// The relay answers subscriptions in order, so an echo of a token sent
    /// after them also proves that every retained command value ahead of it has
    /// already been delivered. That makes the first successful round trip a
    /// barrier: once it returns, a latched e-stop has already reached the
    /// motors.
    pub async fn attach(client: Arc<Clasp>, addr: &Addresses) -> anyhow::Result<Arc<Self>> {
        let echoed = Arc::new(watch::channel(String::new()).0);
        let address = addr.health();

        let sink = echoed.clone();
        client
            .subscribe(&address, move |value, _address| {
                if let Some(token) = crate::link::as_string(&value) {
                    sink.send_replace(token);
                }
            })
            .await?;

        Ok(Arc::new(Self {
            client,
            address,
            tokens: Tokens::new(),
            echoed,
        }))
    }

    /// Prove the link works before the robot advertises itself.
    ///
    /// A robot that publishes `status/online` over a link that cannot deliver
    /// commands is worse than one that never came up at all: every console
    /// shows it as ready, and it ignores all of them.
    pub async fn verify(&self) -> anyhow::Result<()> {
        self.round_trip(STARTUP_TIMEOUT).await
    }

    /// Probe until the link stops answering. Returns when starting over is the
    /// right answer; the caller decides what to do about it.
    pub async fn watch_until_dead(self: Arc<Self>) {
        let mut ticker = tokio::time::interval_at(Instant::now() + PROBE_INTERVAL, PROBE_INTERVAL);
        // A slow round trip must not be followed by a burst of catch-up ticks.
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        let mut failures: u32 = 0;
        loop {
            ticker.tick().await;
            match self.round_trip(PROBE_TIMEOUT).await {
                Ok(()) => {
                    if failures > 0 {
                        tracing::info!(missed = failures, "relay link answering again");
                        failures = 0;
                    }
                }
                Err(err) => {
                    failures += 1;
                    tracing::warn!(
                        %err,
                        failures,
                        limit = FAILURES_BEFORE_DEAD,
                        "relay link did not answer"
                    );
                    if failures >= FAILURES_BEFORE_DEAD {
                        return;
                    }
                }
            }
        }
    }

    /// One round trip. An `Err` means the relay did not carry our own write
    /// back to us in time, whatever the reason for that.
    async fn round_trip(&self, budget: Duration) -> anyhow::Result<()> {
        let token = self.tokens.issue();
        // Watch before publishing, so an echo that beats us back is still seen
        // rather than lost in the gap between the two calls.
        let mut echoed = self.echoed.subscribe();

        let outcome = tokio::time::timeout(budget, async {
            self.client
                .set(self.address.as_str(), token.as_str())
                .await?;
            echoed
                .wait_for(|seen| *seen == token)
                .await
                .map_err(|_| anyhow::anyhow!("probe channel closed"))?;
            Ok::<(), anyhow::Error>(())
        })
        .await;

        match outcome {
            Ok(result) => result,
            Err(_) => Err(anyhow::anyhow!("no echo within {budget:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique_within_a_run() {
        let tokens = Tokens::new();
        let issued: Vec<String> = (0..100).map(|_| tokens.issue()).collect();
        let mut unique = issued.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), issued.len());
    }

    #[test]
    fn a_later_run_never_reissues_an_earlier_runs_token() {
        // The whole point of the run stamp: a token latched on the relay by the
        // previous run must not satisfy this run's probe.
        let first = Tokens::new();
        let second = Tokens::new();
        assert_ne!(first.run, second.run, "run ids collided");
        assert_ne!(first.issue(), second.issue());
    }
}
