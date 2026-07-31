//! Who is allowed to drive.
//!
//! Any number of operators can watch the robot at once, but only one drives it.
//! The robot owns that decision rather than the consoles: a console that
//! crashes, loses its network, or simply closes its tab must not be able to
//! leave the wheel locked for everyone else.
//!
//! The rules, all in one place:
//!
//!   - Driving while the wheel is free claims it implicitly, so nobody ever
//!     asks for permission.
//!   - The lease lapses after `LEASE_TIMEOUT` without a drive command, which is
//!     about as long as a pause between deliberate movements. In practice that
//!     means the wheel belongs to whoever is currently driving, and the moment
//!     they stop, the next person can simply start. Taking turns needs no
//!     buttons at all.
//!   - An explicit claim still wins immediately, for grabbing the wheel from
//!     someone mid-drive. Taking over is a social problem, not a protocol one,
//!     and a lease nobody can break is worse on a shared robot than an
//!     occasional rude handoff.
//!   - The e-stop is deliberately *not* arbitrated. Anyone watching can stop
//!     the robot at any time, whether or not they hold the wheel.
//!
//! The arbiter itself is pure state: every method takes the current time, so
//! the rules are unit-tested without sleeping or hardware. [`spawn`] wraps it
//! in the task that expires stale leases and publishes `status/driver`.

use crate::link::to_value;
use crate::protocol::{Addresses, Driver};
use clasp_client::prelude::Value;
use clasp_client::Clasp;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// How long the wheel stays held after the driver's last command.
///
/// This is deliberately about the length of a pause between two deliberate
/// movements, not the length of a session. The console repeats the current
/// demand every 150 ms while the robot is moving, so an active driver renews
/// this many times over; letting go of the controls frees the wheel almost at
/// once, and the next person takes their turn by simply driving. Longer, and
/// somebody has to ask for a wheel nobody is using.
pub const LEASE_TIMEOUT: Duration = Duration::from_millis(1500);

/// Name recorded for an operator that never introduced itself. Consoles
/// predating multi-operator support send neither a name nor a session.
const ANONYMOUS: &str = "operator";

struct State {
    driver: Option<Driver>,
    /// When the current driver last claimed or drove.
    touched: Instant,
}

pub struct Arbiter {
    state: Mutex<State>,
    /// Publishes the current holder so the status task can mirror it to the
    /// relay without polling.
    tx: watch::Sender<Option<Driver>>,
}

impl Arbiter {
    pub fn new() -> (Arc<Self>, watch::Receiver<Option<Driver>>) {
        let (tx, rx) = watch::channel(None);
        let arbiter = Arc::new(Self {
            state: Mutex::new(State {
                driver: None,
                touched: Instant::now(),
            }),
            tx,
        });
        (arbiter, rx)
    }

    /// Take the wheel. Returns the operator that was displaced, if any.
    pub fn claim(&self, session: &str, name: &str) -> Option<Driver> {
        self.claim_at(session, name, Instant::now())
    }

    /// Give the wheel up. Ignored unless `session` currently holds it, so a
    /// stale release from a previous holder cannot cut short the new one.
    pub fn release(&self, session: &str) -> bool {
        self.release_at(session, Instant::now())
    }

    /// Should a drive command from `session` be obeyed? Renews the lease when
    /// it should, and takes a free wheel on the operator's behalf.
    pub fn accepts(&self, session: &str, name: &str) -> bool {
        self.accepts_at(session, name, Instant::now())
    }

    /// Drop the lease if the holder has gone quiet. Returns true if it lapsed.
    pub fn expire(&self) -> bool {
        self.expire_at(Instant::now())
    }

    /// The current holder. Test-facing only: production code watches the
    /// channel so it reacts to a change instead of polling for one.
    #[cfg(test)]
    pub fn current(&self) -> Option<Driver> {
        self.state().driver.clone()
    }

    pub fn claim_at(&self, session: &str, name: &str, now: Instant) -> Option<Driver> {
        let driver = Driver {
            session: session.to_string(),
            name: display_name(name),
        };
        let previous = {
            let mut state = self.state();
            let previous = state.driver.clone();
            state.driver = Some(driver.clone());
            state.touched = now;
            previous
        };
        self.publish(Some(driver));
        previous.filter(|p| p.session != session)
    }

    pub fn release_at(&self, session: &str, now: Instant) -> bool {
        let released = {
            let mut state = self.state();
            match &state.driver {
                Some(d) if d.session == session => {
                    state.driver = None;
                    state.touched = now;
                    true
                }
                _ => false,
            }
        };
        if released {
            self.publish(None);
        }
        released
    }

    pub fn accepts_at(&self, session: &str, name: &str, now: Instant) -> bool {
        // Expiring first means a lapsed lease is picked up by this very
        // command rather than a tick later.
        self.expire_at(now);

        let granted = {
            let mut state = self.state();
            match &state.driver {
                Some(d) if d.session == session => {
                    state.touched = now;
                    None
                }
                Some(_) => return false,
                None => {
                    let driver = Driver {
                        session: session.to_string(),
                        name: display_name(name),
                    };
                    state.driver = Some(driver.clone());
                    state.touched = now;
                    Some(driver)
                }
            }
        };
        if let Some(driver) = granted {
            tracing::info!(session = %driver.session, name = %driver.name, "wheel taken by driving");
            self.publish(Some(driver));
        }
        true
    }

    pub fn expire_at(&self, now: Instant) -> bool {
        let lapsed = {
            let mut state = self.state();
            let stale = state.driver.is_some()
                && now.saturating_duration_since(state.touched) >= LEASE_TIMEOUT;
            if stale {
                state.driver = None;
                state.touched = now;
            }
            stale
        };
        if lapsed {
            self.publish(None);
        }
        lapsed
    }

    /// Recover from a poisoned lock rather than panicking. The guarded state is
    /// three plain fields with no invariant that a panic could have broken, and
    /// a robot that stops arbitrating is worse than one holding slightly stale
    /// state.
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn publish(&self, driver: Option<Driver>) {
        // Only wakes the status task when the holder actually changed, so a
        // driver renewing several times a second does not republish a Param
        // several times a second.
        self.tx.send_if_modified(|current| {
            if *current == driver {
                false
            } else {
                *current = driver;
                true
            }
        });
    }
}

fn display_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        ANONYMOUS.to_string()
    } else {
        trimmed.chars().take(32).collect()
    }
}

/// Expire stale leases and mirror the current holder to `status/driver`.
pub fn spawn(
    client: Arc<Clasp>,
    addr: Addresses,
    arbiter: Arc<Arbiter>,
    mut driver_rx: watch::Receiver<Option<Driver>>,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_millis(500));
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if arbiter.expire() {
                        tracing::info!("driving lease lapsed; wheel is free");
                    }
                }
                changed = driver_rx.changed() => {
                    if changed.is_err() {
                        break; // arbiter dropped
                    }
                    // Clone out of the guard before awaiting: it is a sync lock.
                    let driver = driver_rx.borrow_and_update().clone();
                    publish_driver(&client, &addr, driver).await;
                }
            }
        }
    });
}

async fn publish_driver(client: &Arc<Clasp>, addr: &Addresses, driver: Option<Driver>) {
    let payload = match &driver {
        Some(d) => to_value(serde_json::json!({ "session": d.session, "name": d.name })),
        None => Value::Null,
    };
    match &driver {
        Some(d) => tracing::info!(session = %d.session, name = %d.name, "driver changed"),
        None => tracing::info!("wheel is free"),
    }
    if let Err(err) = client.set(addr.status("driver").as_str(), payload).await {
        tracing::debug!(%err, "driver status publish failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arbiter() -> Arc<Arbiter> {
        Arbiter::new().0
    }

    #[test]
    fn free_wheel_is_taken_by_the_first_driver() {
        let a = arbiter();
        let t = Instant::now();
        assert!(a.accepts_at("alice", "", t));
        assert_eq!(a.current().unwrap().session, "alice");
    }

    #[test]
    fn a_second_operator_is_ignored_while_the_first_drives() {
        let a = arbiter();
        let t = Instant::now();
        assert!(a.accepts_at("alice", "", t));
        assert!(!a.accepts_at("bob", "", t));
        // Alice keeps the wheel by driving, so Bob stays locked out even past
        // the lease window.
        let later = t + LEASE_TIMEOUT * 2;
        assert!(a.accepts_at("alice", "", later));
        assert!(!a.accepts_at("bob", "", later));
    }

    #[test]
    fn the_lease_lapses_when_the_driver_goes_quiet() {
        let a = arbiter();
        let t = Instant::now();
        assert!(a.accepts_at("alice", "", t));
        assert!(!a.expire_at(t + LEASE_TIMEOUT / 2));
        assert!(a.expire_at(t + LEASE_TIMEOUT));
        assert!(a.current().is_none());
        assert!(a.accepts_at("bob", "", t + LEASE_TIMEOUT));
    }

    #[test]
    fn a_lapsed_lease_is_picked_up_by_the_next_command() {
        let a = arbiter();
        let t = Instant::now();
        assert!(a.accepts_at("alice", "", t));
        // No explicit expire call: accepts_at must notice the lapse itself.
        assert!(a.accepts_at("bob", "", t + LEASE_TIMEOUT));
        assert_eq!(a.current().unwrap().session, "bob");
    }

    #[test]
    fn an_explicit_claim_takes_over() {
        let a = arbiter();
        let t = Instant::now();
        assert!(a.accepts_at("alice", "", t));
        let displaced = a.claim_at("bob", "Bob", t);
        assert_eq!(displaced.unwrap().session, "alice");
        assert_eq!(a.current().unwrap().name, "Bob");
        assert!(!a.accepts_at("alice", "", t));
        assert!(a.accepts_at("bob", "", t));
    }

    #[test]
    fn reclaiming_your_own_wheel_displaces_nobody() {
        let a = arbiter();
        let t = Instant::now();
        a.claim_at("alice", "Alice", t);
        assert!(a.claim_at("alice", "Alice", t).is_none());
    }

    #[test]
    fn release_frees_the_wheel_only_for_its_holder() {
        let a = arbiter();
        let t = Instant::now();
        a.claim_at("alice", "Alice", t);
        assert!(!a.release_at("bob", t));
        assert!(a.current().is_some());
        assert!(a.release_at("alice", t));
        assert!(a.current().is_none());
    }

    #[test]
    fn a_stale_release_cannot_cut_short_the_new_driver() {
        let a = arbiter();
        let t = Instant::now();
        a.claim_at("alice", "Alice", t);
        a.claim_at("bob", "Bob", t);
        // Alice's console sends its release after being displaced.
        assert!(!a.release_at("alice", t));
        assert_eq!(a.current().unwrap().session, "bob");
    }

    #[test]
    fn an_unnamed_claim_gets_a_readable_default() {
        let a = arbiter();
        let t = Instant::now();
        a.claim_at("alice", "   ", t);
        assert_eq!(a.current().unwrap().name, ANONYMOUS);
    }

    #[test]
    fn a_long_name_is_bounded() {
        let a = arbiter();
        let t = Instant::now();
        a.claim_at("alice", &"n".repeat(200), t);
        assert_eq!(a.current().unwrap().name.chars().count(), 32);
    }

    #[test]
    fn driving_a_free_wheel_names_the_driver_from_the_command() {
        // The name rides on the drive itself rather than arriving separately,
        // so the other consoles never show a new driver as "operator" while
        // two messages settle.
        let a = arbiter();
        a.accepts_at("alice", "Ada", Instant::now());
        assert_eq!(a.current().unwrap().name, "Ada");
    }

    #[test]
    fn legacy_consoles_share_one_anonymous_lease() {
        // A console that predates the protocol sends an empty session. It must
        // still be able to drive rather than being locked out forever.
        let a = arbiter();
        let t = Instant::now();
        assert!(a.accepts_at("", "", t));
        assert!(a.accepts_at("", "", t));
        assert!(!a.accepts_at("alice", "", t));
    }

    #[test]
    fn the_holder_is_published_on_every_change() {
        let (a, mut rx) = Arbiter::new();
        let t = Instant::now();
        assert!(rx.borrow_and_update().is_none());

        a.claim_at("alice", "Alice", t);
        assert!(rx.has_changed().unwrap());
        assert_eq!(rx.borrow_and_update().clone().unwrap().session, "alice");

        // Renewing the same lease must not republish.
        a.accepts_at("alice", "", t);
        assert!(!rx.has_changed().unwrap());

        a.release_at("alice", t);
        assert!(rx.has_changed().unwrap());
        assert!(rx.borrow_and_update().is_none());
    }
}
