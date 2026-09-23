//! The reducer contract and the durable transition receipt: the Rust side of
//! `docs/design/AUTOBOT-FORMAL-SURFACE.md` §8, which has the implementation expose
//! deterministic reducer decisions and durable transition receipts, over the commit lanes of
//! `docs/design/AUTOBOT-KERNEL.md` §1.
//!
//! - A [`Reducer`] decides one command against one aggregate's state under a [`Guards`]
//!   value: `(state, command, guards) -> Decision`. It is a pure function with no I/O and no
//!   `self`, so it carries nothing from one decision to the next.
//! - A [`Decision`] either commits a [`Transition`] on one lane or refuses with a
//!   [`RefusalGround`].
//! - [`step`] runs a reducer on a [`Versioned`] aggregate: it checks the command's expected
//!   revision, asks the reducer, enforces the lane's field partition on the state digests,
//!   advances the counters and writes the [`TransitionReceipt`] of a commit, or returns the
//!   [`Refusal`].
//! - [`Guards`] holds one switch per FORMAL §5 guard ([`GuardId`]). Outside tests every guard
//!   is enabled; [`Guards::without`], which disables one so that a negative variant can show
//!   its fixture failing, exists only under `cfg(test)` or the `testing` feature.
//!
//! Choices this module makes where the design is open:
//!
//! - A state gives its domain and control digests through [`ReducerState`]; which fields each
//!   digest covers is the field partition of KERNEL §1, which the state's kind defines. [`step`]
//!   refuses a domain transition that changes the control digest, and, while
//!   [`GuardId::ControlFieldsOnly`] is enabled, a control transition that changes the domain
//!   digest: KERNEL §1 rejects a commit that touches both classes.
//! - A command whose expected revision is not the aggregate's current revision in its lane is
//!   refused with [`RefusalGround::RevisionMismatch`] before the reducer runs; a retry is never
//!   rebased (KERNEL §2). The refusal is a decision, not the rejection proof of KERNEL §2: a
//!   controller still has to show that no matching slot or receipt exists before it records a
//!   rejection.
//! - A [`Refusal`] records its ground and the revision of the command's lane that the decision
//!   read, as the owner's ruling on rejection proofs states for a guard refusal (#328). It
//!   carries no serde form: how a `CommandReceipt` records a rejection is open in #328.
//! - The [`TransitionReceipt`] is the reducer's record of one commit, from which a controller
//!   fills the FORMAL §2 `CommandReceipt` and `PendingCommit` or `ControlReceipt`. It holds the
//!   fields those records share with a transition and the refinement mapping reads:
//!   the reducer version, the FORMAL §3 action, the aggregate, the command, its principal and
//!   input digest, the lane, the counters and both digests before and after, and the effect
//!   intents. It also lists the guards disabled when it was decided, so that a receipt made
//!   by a negative variant is never mistaken for one made under every guard. It holds the
//!   disabled guards as a list rather than a [`Guards`] value, so reading a receipt never
//!   constructs a [`Guards`] with a guard disabled.
//! - A receipt names its action by its FORMAL §3 name; whether the transition is an allowed
//!   model transition is the refinement check of G-FORMAL, not the receipt's.
//! - The effect intents are [`SlotEffectIntent`]s, whose provider-facing fields stay opaque
//!   text until the design types them (#329).
//! - The pending slot, its receipt barrier and the control-receipt ring are written by the
//!   commit that installs them and are not part of a decision; [`step`] performs no write.
//!
//! The receipt's schema is snapshotted in `transition_receipt.schema.json` beside this module;
//! the snapshot test rewrites it when `AUTOBOT_UPDATE_SNAPSHOTS` is set.

mod guards;
mod receipt;

pub use guards::{GuardId, Guards};
pub use receipt::{
    ActionName, Counters, RECEIPT_SCHEMA_VERSION, ReceiptError, StateDigests, TransitionReceipt,
    TransitionReceiptFields,
};

use crate::error::ValueError;
use crate::status::SlotEffectIntent;
use crate::types::{Digest, Lane, LaneRevision, Principal, Uid};
use std::fmt;

/// The state a reducer decides over: it gives its domain and control digests.
pub trait ReducerState {
    /// The digests of the state's domain fields and of its control fields.
    fn digests(&self) -> StateDigests;
}

/// A deterministic decision function over one aggregate kind.
///
/// `reduce` has no receiver and must be a pure function of its arguments: the same state,
/// command and guards give the same decision, every time and in every process.
pub trait Reducer {
    /// The aggregate state the reducer decides over.
    type State: ReducerState;
    /// The commands the reducer decides.
    type Command;
    /// The reducer's version, recorded in every receipt it produces.
    const VERSION: &'static str;

    /// Decides `command` against `state` under `guards`.
    fn reduce(
        state: &Self::State,
        command: &Self::Command,
        guards: &Guards,
    ) -> Decision<Self::State>;
}

/// What a reducer decides about one command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision<S> {
    /// The command commits this transition.
    Commit(Transition<S>),
    /// The command is refused on this ground.
    Refuse(RefusalGround),
}

/// A transition a reducer commits: the lane, the FORMAL §3 action it refines, the state after
/// it and, for a domain commit, its effect intents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition<S> {
    lane: Lane,
    action: &'static str,
    after: S,
    effect_intents: Vec<SlotEffectIntent>,
}

impl<S> Transition<S> {
    /// A domain-lane transition refining `action` to `after`, with `effect_intents` in
    /// effect-index order.
    #[must_use]
    pub fn domain(action: &'static str, after: S, effect_intents: Vec<SlotEffectIntent>) -> Self {
        Self {
            lane: Lane::Domain,
            action,
            after,
            effect_intents,
        }
    }

    /// A control-lane transition refining `action` to `after`. A control commit has no effect
    /// intents.
    #[must_use]
    pub fn control(action: &'static str, after: S) -> Self {
        Self {
            lane: Lane::Control,
            action,
            after,
            effect_intents: Vec::new(),
        }
    }

    /// The transition's lane.
    #[must_use]
    pub fn lane(&self) -> Lane {
        self.lane
    }
}

/// Why a command was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalGround {
    /// A FORMAL §5 guard refused the command.
    Guard(GuardId),
    /// A precondition of the reducer, named by the reducer, does not hold.
    Precondition(&'static str),
    /// The command's expected revision is not the aggregate's current revision in its lane.
    RevisionMismatch,
    /// A domain transition changed the control digest.
    DomainCommitTouchesControl,
}

/// A refused command: the ground, and the revision of the command's lane the decision read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refusal {
    /// Why the command was refused.
    pub ground: RefusalGround,
    /// The aggregate's revision in the command's lane when the decision was made.
    pub read_revision: LaneRevision,
}

/// An aggregate's state with its UID and counters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Versioned<S> {
    /// The aggregate's UID.
    pub uid: Uid,
    /// The aggregate's counters.
    pub counters: Counters,
    /// The aggregate's state.
    pub state: S,
}

/// What a command pins, besides its payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPins {
    /// The command's UID.
    pub command_uid: Uid,
    /// The authenticated writer of the command.
    pub principal: Principal,
    /// The digest of the command's input.
    pub input_digest: Digest,
    /// The revision the command expects, which also names its lane.
    pub expected_revision: LaneRevision,
}

/// The outcome of [`step`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step<S> {
    /// The command committed: the aggregate after the commit and the commit's receipt.
    Committed {
        /// The aggregate after the commit.
        aggregate: Versioned<S>,
        /// The receipt of the commit.
        receipt: TransitionReceipt,
    },
    /// The command was refused; the aggregate is unchanged.
    Refused(Refusal),
}

/// Runs reducer `R` on `aggregate` for `command`, pinned by `pins`, under `guards`.
///
/// The command is refused with [`RefusalGround::RevisionMismatch`] if its expected revision is
/// not the aggregate's, with the reducer's ground if the reducer refuses, with
/// [`RefusalGround::DomainCommitTouchesControl`] if a domain transition changes the control
/// digest, and with [`RefusalGround::Guard`]`(`[`GuardId::ControlFieldsOnly`]`)` if a control
/// transition changes the domain digest while that guard is enabled. Otherwise the counters
/// advance by one commit of the lane and the receipt records the transition.
///
/// # Errors
///
/// [`ReducerError::LaneMismatch`] if the reducer commits on another lane than the command
/// pins, [`ReducerError::Counter`] if a counter would pass `i64::MAX`, and
/// [`ReducerError::Receipt`] if the transition makes an invalid receipt, such as an invalid
/// action name or effect intents out of order.
pub fn step<R: Reducer>(
    aggregate: &Versioned<R::State>,
    pins: &CommandPins,
    command: &R::Command,
    guards: &Guards,
) -> Result<Step<R::State>, ReducerError> {
    let lane = pins.expected_revision.lane();
    let read_revision = aggregate.counters.revision(lane);
    let refuse = |ground| {
        Ok(Step::Refused(Refusal {
            ground,
            read_revision,
        }))
    };
    if read_revision != pins.expected_revision {
        return refuse(RefusalGround::RevisionMismatch);
    }
    let transition = match R::reduce(&aggregate.state, command, guards) {
        Decision::Commit(t) => t,
        Decision::Refuse(ground) => return refuse(ground),
    };
    if transition.lane != lane {
        return Err(ReducerError::LaneMismatch {
            pinned: lane,
            committed: transition.lane,
        });
    }
    let before_digests = aggregate.state.digests();
    let after_digests = transition.after.digests();
    match lane {
        Lane::Domain if before_digests.control != after_digests.control => {
            return refuse(RefusalGround::DomainCommitTouchesControl);
        }
        Lane::Control
            if before_digests.domain != after_digests.domain
                && guards.is_enabled(GuardId::ControlFieldsOnly) =>
        {
            return refuse(RefusalGround::Guard(GuardId::ControlFieldsOnly));
        }
        _ => {}
    }
    let after = aggregate
        .counters
        .advance(lane)
        .map_err(ReducerError::Counter)?;
    let receipt = TransitionReceipt::new(TransitionReceiptFields {
        schema_version: RECEIPT_SCHEMA_VERSION,
        reducer_version: R::VERSION.to_owned(),
        action: transition.action.parse().map_err(ReducerError::Receipt)?,
        aggregate_uid: aggregate.uid.clone(),
        command_uid: pins.command_uid.clone(),
        principal: pins.principal.clone(),
        input_digest: pins.input_digest,
        lane,
        before: aggregate.counters,
        after,
        before_digests,
        after_digests,
        effect_intents: transition.effect_intents,
        disabled_guards: guards.disabled(),
    })
    .map_err(ReducerError::Receipt)?;
    Ok(Step::Committed {
        aggregate: Versioned {
            uid: aggregate.uid.clone(),
            counters: after,
            state: transition.after,
        },
        receipt,
    })
}

/// Why [`step`] could not produce an outcome: a defect of the reducer or an exhausted
/// counter, never a refusal of the command.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReducerError {
    /// The reducer committed on another lane than the command pins.
    LaneMismatch {
        /// The lane of the command's expected revision.
        pinned: Lane,
        /// The lane of the reducer's transition.
        committed: Lane,
    },
    /// A counter would pass `i64::MAX`.
    Counter(ValueError),
    /// The transition does not make a valid receipt.
    Receipt(ReceiptError),
}

impl fmt::Display for ReducerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LaneMismatch { pinned, committed } => write!(
                f,
                "the command pins the {pinned} lane but the reducer committed on {committed}"
            ),
            Self::Counter(e) => write!(f, "counter exhausted: {e}"),
            Self::Receipt(e) => write!(f, "invalid receipt: {e}"),
        }
    }
}

impl std::error::Error for ReducerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::LaneMismatch { .. } => None,
            Self::Counter(e) => Some(e),
            Self::Receipt(e) => Some(e),
        }
    }
}

#[cfg(test)]
mod tests;
