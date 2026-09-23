//! The interleaving explorer: two actors over one shared world, run under every schedule of a
//! kind, each schedule from a fresh world, with a check after each.
//!
//! - An [`Actor`] advances by atomic steps on the world. [`ProtocolActor`] makes a kernel
//!   store [`Protocol`] an actor over a [`Driver`]: each of its steps performs at most one
//!   store operation, so every read and every write is a step boundary.
//! - [`every_position`] runs actor `B` whole at every step boundary of actor `A`: before its
//!   first step, between any two steps and after its last. This is one action interleaved
//!   against another at every write position.
//! - [`every_interleaving`] runs every merge of the two actors' steps.
//!
//! Each schedule starts from `setup`, which builds the world and both actors afresh, so a
//! schedule is replayed from nothing and never inherits another's state. After each schedule
//! `check` inspects the world and both actors; every schedule it rejects is reported as a
//! [`Violation`] with the schedule that produced it.
//!
//! Choices this module makes where the design is open:
//!
//! - An actor's step count may depend on the other actor, as when a compare-and-set conflicts
//!   and is retried, so positions are discovered by running rather than counted up front: the
//!   explorer tries position `i` of `A` for `i = 0, 1, …` until `A` finishes within `i` steps.
//! - Every actor takes at least one step, even one that finds nothing to do.

use crate::harness::{Bounds, Driver, Exceeded, Run};
use autobot_fakes::store::Execution;
use autobot_kernel::store::{Protocol, Step};

/// Whether an actor has more steps to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// The actor has more steps.
    Running,
    /// The actor is finished.
    Done,
}

/// A participant in a schedule: it takes one atomic step at a time on the world `W`.
pub trait Actor<W: ?Sized> {
    /// Takes the actor's next step on `world`; whether it has more.
    fn step(&mut self, world: &mut W) -> Progress;
}

/// One of the two actors of a schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Who {
    /// The first actor.
    A,
    /// The second actor.
    B,
}

/// A schedule: which actor took each step, in order.
pub type Schedule = Vec<Who>;

/// A schedule `check` rejected, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The schedule.
    pub schedule: Schedule,
    /// What `check` said.
    pub message: String,
}

/// What an exploration ran and found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Exploration {
    /// Every schedule run, in the order run.
    pub schedules: Vec<Schedule>,
    /// The schedules `check` rejected.
    pub violations: Vec<Violation>,
}

impl Exploration {
    fn record(&mut self, schedule: Schedule, verdict: Result<(), String>) {
        if let Err(message) = verdict {
            self.violations.push(Violation {
                schedule: schedule.clone(),
                message,
            });
        }
        self.schedules.push(schedule);
    }
}

/// One schedule in progress: the world, both actors, the steps taken and which actors are
/// finished.
struct Replay<W, A, B> {
    world: W,
    a: A,
    b: B,
    schedule: Schedule,
    a_done: bool,
    b_done: bool,
}

impl<W, A: Actor<W>, B: Actor<W>> Replay<W, A, B> {
    fn new((world, a, b): (W, A, B)) -> Self {
        Self {
            world,
            a,
            b,
            schedule: Vec::new(),
            a_done: false,
            b_done: false,
        }
    }

    /// Takes one step of `who`, which is not finished.
    fn step(&mut self, who: Who, bounds: &Bounds) -> Result<(), Exceeded> {
        if self.schedule.len() >= bounds.steps {
            return Err(Exceeded::Steps(bounds.steps));
        }
        let progress = match who {
            Who::A => self.a.step(&mut self.world),
            Who::B => self.b.step(&mut self.world),
        };
        self.schedule.push(who);
        let done = progress == Progress::Done;
        match who {
            Who::A => self.a_done = done,
            Who::B => self.b_done = done,
        }
        Ok(())
    }

    fn done(&self, who: Who) -> bool {
        match who {
            Who::A => self.a_done,
            Who::B => self.b_done,
        }
    }

    /// Steps `who` until it is finished.
    fn finish(&mut self, who: Who, bounds: &Bounds) -> Result<(), Exceeded> {
        while !self.done(who) {
            self.step(who, bounds)?;
        }
        Ok(())
    }

    fn check(
        self,
        check: &mut impl FnMut(&W, &A, &B) -> Result<(), String>,
    ) -> (Schedule, Result<(), String>) {
        let verdict = check(&self.world, &self.a, &self.b);
        (self.schedule, verdict)
    }
}

/// Runs `B` whole at every step boundary of `A`, from the boundary before `A`'s first step to
/// the one after its last, and checks each schedule.
///
/// # Errors
///
/// [`Exceeded::Steps`] if one schedule takes more than `bounds.steps` steps.
pub fn every_position<W, A: Actor<W>, B: Actor<W>>(
    mut setup: impl FnMut() -> (W, A, B),
    mut check: impl FnMut(&W, &A, &B) -> Result<(), String>,
    bounds: &Bounds,
) -> Result<Exploration, Exceeded> {
    let mut exploration = Exploration::default();
    for position in 0.. {
        let mut replay = Replay::new(setup());
        while replay.schedule.len() < position && !replay.a_done {
            replay.step(Who::A, bounds)?;
        }
        let last = replay.a_done;
        replay.finish(Who::B, bounds)?;
        replay.finish(Who::A, bounds)?;
        let (schedule, verdict) = replay.check(&mut check);
        exploration.record(schedule, verdict);
        if last {
            break;
        }
    }
    Ok(exploration)
}

/// Runs every merge of `A`'s and `B`'s steps and checks each schedule.
///
/// # Errors
///
/// [`Exceeded::Steps`] if one schedule takes more than `bounds.steps` steps.
pub fn every_interleaving<W, A: Actor<W>, B: Actor<W>>(
    mut setup: impl FnMut() -> (W, A, B),
    mut check: impl FnMut(&W, &A, &B) -> Result<(), String>,
    bounds: &Bounds,
) -> Result<Exploration, Exceeded> {
    let mut exploration = Exploration::default();
    let mut prefixes: Vec<Schedule> = vec![Vec::new()];
    while let Some(prefix) = prefixes.pop() {
        let mut replay = Replay::new(setup());
        for &who in &prefix {
            replay.step(who, bounds)?;
        }
        match (replay.a_done, replay.b_done) {
            (false, false) => {
                prefixes.push([prefix.as_slice(), &[Who::B]].concat());
                prefixes.push([prefix.as_slice(), &[Who::A]].concat());
            }
            _ => {
                replay.finish(Who::A, bounds)?;
                replay.finish(Who::B, bounds)?;
                let (schedule, verdict) = replay.check(&mut check);
                exploration.record(schedule, verdict);
            }
        }
    }
    Ok(exploration)
}

/// A kernel store protocol as an actor over a [`Driver`]: each step performs one operation.
#[derive(Debug)]
pub struct ProtocolActor<P: Protocol> {
    protocol: P,
    run: Option<Run<P::Outcome>>,
}

impl<P: Protocol> ProtocolActor<P> {
    /// An actor running `protocol`.
    pub fn new(protocol: P) -> Self {
        Self {
            protocol,
            run: None,
        }
    }

    /// How the protocol ended, once the actor is finished.
    #[must_use]
    pub fn run(&self) -> Option<&Run<P::Outcome>> {
        self.run.as_ref()
    }

    /// Records how the protocol ended.
    fn end(&mut self, run: Run<P::Outcome>) -> Progress {
        self.run = Some(run);
        Progress::Done
    }
}

impl<P: Protocol, D: Driver + ?Sized> Actor<D> for ProtocolActor<P> {
    fn step(&mut self, driver: &mut D) -> Progress {
        if self.run.is_some() {
            return Progress::Done;
        }
        let op = match self.protocol.step() {
            Step::Done(outcome) => return self.end(Run::Done(outcome)),
            Step::Op(op) => op,
        };
        match driver.perform(op) {
            Execution::Result(result) => {
                if let Err(e) = self.protocol.resume(result) {
                    return self.end(Run::Refused(e));
                }
            }
            Execution::Crashed => return self.end(Run::Crashed),
        }
        // A protocol that is finished after this operation ends in this step; otherwise its
        // next operation stays outstanding and is returned again by the next `step`.
        match self.protocol.step() {
            Step::Done(outcome) => self.end(Run::Done(outcome)),
            Step::Op(_) => Progress::Running,
        }
    }
}

#[cfg(test)]
mod tests;
