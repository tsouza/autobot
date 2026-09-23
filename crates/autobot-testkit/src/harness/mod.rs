//! The fixture harness: the [`Driver`] a store-agnostic scenario runs against, fault injection
//! at any write position, lossy deliveries and the liveness [`Bounds`] of one fixture.
//!
//! - A scenario takes `&mut dyn Driver` and never names a concrete store, so the same script
//!   runs on the in-memory [`MemStore`] and on any other store that implements [`Driver`].
//! - [`run`] drives one sans-I/O [`Protocol`] against a driver within a step bound and reports
//!   a crash, a refused result or a stall as a [`Run`] instead of failing.
//! - [`AtWrite`] arms one kernel conformance [`Fault`] as the `n`-th write is sent, and
//!   [`sweep`] runs a scenario once cleanly and then once per write position with the fault
//!   armed there: with [`Fault::Crash`] the process dies between any two writes, with
//!   [`Fault::WriteTimeout`] any write times out. Faults of the store itself (lost create
//!   acknowledgements, dropped, duplicated and reordered watch events, expired watches) are the
//!   ones [`MemStore`] already injects; the harness only positions them.
//! - [`mishaps`] and [`deliver`] drop, duplicate or swap one delivery of a message sequence,
//!   and [`orders`] gives every order of it, for consumers fed outside the store, such as a
//!   projection fed audit events or a broker fed requests.
//!
//! Choices this module makes where the design is open:
//!
//! - The liveness bounds of FORMAL §6 are fixture constants chosen per fixture (M0 §2), so
//!   [`Bounds`] has no default: every fixture states its own step and time bound, and none is
//!   read from the profile.
//! - A fault positioned by [`AtWrite`] is armed on the store as the write is sent, so the
//!   store decides whether it fires: a crash fires at the first write from that position on
//!   that applies, since [`MemStore`] counts only applied writes toward a crash.

mod deliveries;

pub use deliveries::{Mishap, deliver, mishaps, orders};

use autobot_fakes::store::{Execution, MemStore};
use autobot_kernel::store::conformance::Fault;
use autobot_kernel::store::{Protocol, ProtocolError, Step, StoreOp};
use std::fmt;
use std::num::NonZeroU32;

/// A store a scenario runs against.
pub trait Driver {
    /// Performs `op`.
    fn perform(&mut self, op: StoreOp) -> Execution;

    /// Arms `fault`; it fires once, on the next operation it applies to.
    fn arm(&mut self, fault: Fault);
}

impl Driver for MemStore {
    fn perform(&mut self, op: StoreOp) -> Execution {
        self.execute(op)
    }

    fn arm(&mut self, fault: Fault) {
        MemStore::arm(self, fault);
    }
}

/// The liveness bounds of one fixture: how many steps a run may take and how many ticks of
/// the fixture's clock a condition may take to hold. Both are test-only parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// The most steps one run takes before it counts as stalled: store operations for [`run`],
    /// actor steps for the interleaving explorer.
    pub steps: usize,
    /// The most ticks [`within`] polls a condition for.
    pub ticks: u64,
}

/// A liveness bound a run or a condition exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exceeded {
    /// The run took this many steps without finishing.
    Steps(usize),
    /// The condition did not hold within this many ticks.
    Ticks(u64),
}

impl fmt::Display for Exceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Steps(n) => write!(f, "the run did not finish within {n} steps"),
            Self::Ticks(n) => write!(f, "the condition did not hold within {n} ticks"),
        }
    }
}

impl std::error::Error for Exceeded {}

/// Polls `poll` with the ticks `0..=bounds.ticks` of the fixture's clock and returns the first
/// value it gives.
///
/// # Errors
///
/// [`Exceeded::Ticks`] if `poll` gave nothing within the bound.
pub fn within<T>(bounds: &Bounds, mut poll: impl FnMut(u64) -> Option<T>) -> Result<T, Exceeded> {
    (0..=bounds.ticks)
        .find_map(&mut poll)
        .ok_or(Exceeded::Ticks(bounds.ticks))
}

/// How [`run`] ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run<O> {
    /// The protocol finished with this outcome.
    Done(O),
    /// An armed crash fired: the write applied and the protocol's process is dead.
    Crashed,
    /// The protocol refused the result of its own operation.
    Refused(ProtocolError),
    /// The protocol performed the step bound's operations without finishing, as it does while
    /// a store it needs stays down.
    Stalled(Exceeded),
}

/// Drives `protocol` against `driver` until it finishes, crashes, refuses a result or reaches
/// `bounds.steps` operations.
pub fn run<P: Protocol + ?Sized>(
    driver: &mut dyn Driver,
    protocol: &mut P,
    bounds: &Bounds,
) -> Run<P::Outcome> {
    for _ in 0..bounds.steps {
        let op = match protocol.step() {
            Step::Done(outcome) => return Run::Done(outcome),
            Step::Op(op) => op,
        };
        match driver.perform(op) {
            Execution::Result(result) => {
                if let Err(e) = protocol.resume(result) {
                    return Run::Refused(e);
                }
            }
            Execution::Crashed => return Run::Crashed,
        }
    }
    match protocol.step() {
        Step::Done(outcome) => Run::Done(outcome),
        Step::Op(_) => Run::Stalled(Exceeded::Steps(bounds.steps)),
    }
}

/// A driver that arms one fault on its inner driver as the `at`-th write is sent, counting
/// writes from one.
#[derive(Debug, Clone)]
pub struct AtWrite<D> {
    inner: D,
    at: Option<(NonZeroU32, Fault)>,
    writes: u32,
}

impl<D: Driver> AtWrite<D> {
    /// `inner`, with `fault` armed as write `at` is sent.
    pub fn new(inner: D, at: NonZeroU32, fault: Fault) -> Self {
        Self {
            inner,
            at: Some((at, fault)),
            writes: 0,
        }
    }

    /// `inner`, with no fault positioned: it only counts writes.
    pub fn counting(inner: D) -> Self {
        Self {
            inner,
            at: None,
            writes: 0,
        }
    }

    /// The writes sent so far.
    #[must_use]
    pub fn writes(&self) -> u32 {
        self.writes
    }

    /// Whether the positioned fault was armed: whether the run reached its write.
    #[must_use]
    pub fn reached(&self) -> bool {
        self.at.is_none()
    }

    /// The inner driver.
    #[must_use]
    pub fn inner(&self) -> &D {
        &self.inner
    }

    /// The inner driver, mutably.
    pub fn inner_mut(&mut self) -> &mut D {
        &mut self.inner
    }
}

impl<D: Driver> Driver for AtWrite<D> {
    fn perform(&mut self, op: StoreOp) -> Execution {
        if op.kind().is_write() {
            self.writes = self.writes.saturating_add(1);
            if let Some((at, fault)) = self.at
                && at.get() == self.writes
            {
                self.at = None;
                self.inner.arm(fault);
            }
        }
        self.inner.perform(op)
    }

    fn arm(&mut self, fault: Fault) {
        self.inner.arm(fault);
    }
}

/// What [`sweep`] found: the clean run and one run per write position of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sweep<T> {
    /// The writes the clean run sent.
    pub writes: u32,
    /// What the clean run gave.
    pub clean: T,
    /// Per write position of the clean run, from the first: the position and what the run
    /// with the fault armed there gave.
    pub faulted: Vec<(NonZeroU32, T)>,
}

/// Runs `scenario` on a fresh driver from `store` with no fault, then once per write the clean
/// run sent, on a fresh driver with `fault` armed as that write is sent.
///
/// A position the faulted run never reaches, since the scenario took another path, is still
/// run and reported; the scenario tells the two apart with [`AtWrite::reached`].
pub fn sweep<D: Driver, T>(
    mut store: impl FnMut() -> D,
    fault: Fault,
    mut scenario: impl FnMut(&mut AtWrite<D>) -> T,
) -> Sweep<T> {
    let mut clean_driver = AtWrite::counting(store());
    let clean = scenario(&mut clean_driver);
    let writes = clean_driver.writes();
    let faulted = (1..=writes)
        .filter_map(NonZeroU32::new)
        .map(|at| {
            let mut driver = AtWrite::new(store(), at, fault);
            (at, scenario(&mut driver))
        })
        .collect();
    Sweep {
        writes,
        clean,
        faulted,
    }
}

#[cfg(test)]
mod tests;
