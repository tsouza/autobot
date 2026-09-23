//! The durable transition receipt and the values it records.

use super::GuardId;
use crate::error::ValueError;
use crate::status::SlotEffectIntent;
use crate::types::{
    CommitSequence, ControlRevision, Digest, Lane, LaneRevision, Principal, StateRevision, Uid,
};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

/// The version of the [`TransitionReceipt`] schema that this crate writes and reads.
pub const RECEIPT_SCHEMA_VERSION: u32 = 1;

/// The three per-aggregate counters of KERNEL §1 at one point.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Counters {
    /// `state_revision`.
    pub state_revision: StateRevision,
    /// `control_revision`.
    pub control_revision: ControlRevision,
    /// `commit_sequence`.
    pub commit_sequence: CommitSequence,
}

impl Counters {
    /// The revision of `lane`.
    #[must_use]
    pub fn revision(&self, lane: Lane) -> LaneRevision {
        match lane {
            Lane::Domain => LaneRevision::State(self.state_revision),
            Lane::Control => LaneRevision::Control(self.control_revision),
        }
    }

    /// The counters after one commit of `lane`: that lane's revision and the commit sequence
    /// advance by one, the other lane's revision is kept.
    ///
    /// # Errors
    ///
    /// [`ValueError::OutOfRange`] if a counter that advances is already `i64::MAX`.
    pub fn advance(self, lane: Lane) -> Result<Self, ValueError> {
        let commit_sequence = self.commit_sequence.next()?;
        Ok(match lane {
            Lane::Domain => Self {
                state_revision: self.state_revision.next()?,
                commit_sequence,
                ..self
            },
            Lane::Control => Self {
                control_revision: self.control_revision.next()?,
                commit_sequence,
                ..self
            },
        })
    }
}

/// The domain and control digests of an aggregate's state at one point (KERNEL §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct StateDigests {
    /// The digest of the domain fields.
    pub domain: Digest,
    /// The digest of the control fields.
    pub control: Digest,
}

/// The name of the FORMAL §3 action a transition refines, such as `CommitControlCAS` or
/// `AcceptDispatch`: an ASCII letter in upper case followed by ASCII letters and digits.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ActionName(String);

impl ActionName {
    /// The name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ActionName {
    type Err = ReceiptError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();
        let head = chars.next().is_some_and(|c| c.is_ascii_uppercase());
        if head && chars.all(|c| c.is_ascii_alphanumeric()) {
            Ok(Self(s.to_owned()))
        } else {
            Err(ReceiptError::ActionName(s.to_owned()))
        }
    }
}

impl TryFrom<String> for ActionName {
    type Error = ReceiptError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<ActionName> for String {
    fn from(a: ActionName) -> Self {
        a.0
    }
}

impl fmt::Display for ActionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl JsonSchema for ActionName {
    fn schema_name() -> Cow<'static, str> {
        "ActionName".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string", "pattern": "^[A-Z][A-Za-z0-9]*$" })
    }
}

/// The durable record of one committed transition.
///
/// A receipt that exists satisfies the checks of [`TransitionReceipt::new`], which
/// deserialization applies too. Its serde form is [`TransitionReceiptFields`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "TransitionReceiptFields", into = "TransitionReceiptFields")]
pub struct TransitionReceipt(Box<TransitionReceiptFields>);

/// The fields of a [`TransitionReceipt`], and its serde form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "TransitionReceipt")]
pub struct TransitionReceiptFields {
    /// The receipt schema version; [`RECEIPT_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The version of the reducer that decided the transition.
    pub reducer_version: String,
    /// The FORMAL §3 action the transition refines.
    pub action: ActionName,
    /// The UID of the aggregate the transition committed on.
    pub aggregate_uid: Uid,
    /// The UID of the command that the transition decided.
    pub command_uid: Uid,
    /// The authenticated writer of the command.
    pub principal: Principal,
    /// The digest of the command's input.
    pub input_digest: Digest,
    /// The commit lane.
    pub lane: Lane,
    /// The aggregate's counters before the commit; the lane's revision is the command's
    /// expected revision.
    pub before: Counters,
    /// The aggregate's counters after the commit.
    pub after: Counters,
    /// The aggregate's state digests before the commit.
    pub before_digests: StateDigests,
    /// The aggregate's state digests after the commit.
    pub after_digests: StateDigests,
    /// The effect intents of a domain commit, in effect-index order; empty for a control
    /// commit.
    pub effect_intents: Vec<SlotEffectIntent>,
    /// The guards disabled when the transition was decided, in FORMAL §5 table order; empty
    /// for every transition decided under [`Guards::all`](super::Guards::all).
    pub disabled_guards: Vec<GuardId>,
}

impl TransitionReceipt {
    /// The receipt holding `fields`.
    ///
    /// # Errors
    ///
    /// - [`ReceiptError::SchemaVersion`] if `schema_version` is not [`RECEIPT_SCHEMA_VERSION`];
    /// - [`ReceiptError::Counters`] if `after` is not `before` advanced by one commit of `lane`;
    /// - [`ReceiptError::ControlEffects`] if a control commit carries effect intents;
    /// - [`ReceiptError::EffectOrder`] if the effect indexes do not strictly increase;
    /// - [`ReceiptError::DisabledGuards`] if `disabled_guards` is not in table order without
    ///   repeats;
    /// - [`ReceiptError::LanePartition`] if a domain commit changes the control digest, or a
    ///   control commit changes the domain digest without
    ///   [`GuardId::ControlFieldsOnly`] in `disabled_guards`.
    pub fn new(fields: TransitionReceiptFields) -> Result<Self, ReceiptError> {
        let f = &fields;
        if f.schema_version != RECEIPT_SCHEMA_VERSION {
            return Err(ReceiptError::SchemaVersion(f.schema_version));
        }
        if f.before.advance(f.lane).ok() != Some(f.after) {
            return Err(ReceiptError::Counters {
                lane: f.lane,
                before: f.before,
                after: f.after,
            });
        }
        if f.lane == Lane::Control && !f.effect_intents.is_empty() {
            return Err(ReceiptError::ControlEffects);
        }
        if f.effect_intents
            .windows(2)
            .any(|w| w[0].effect_index >= w[1].effect_index)
        {
            return Err(ReceiptError::EffectOrder);
        }
        if f.disabled_guards.windows(2).any(|w| w[0] >= w[1]) {
            return Err(ReceiptError::DisabledGuards);
        }
        let crosses = match f.lane {
            Lane::Domain => f.before_digests.control != f.after_digests.control,
            Lane::Control => {
                f.before_digests.domain != f.after_digests.domain
                    && !f.disabled_guards.contains(&GuardId::ControlFieldsOnly)
            }
        };
        if crosses {
            return Err(ReceiptError::LanePartition(f.lane));
        }
        Ok(Self(Box::new(fields)))
    }

    /// The receipt's fields.
    #[must_use]
    pub fn fields(&self) -> &TransitionReceiptFields {
        &self.0
    }

    /// The revision the command pinned: the lane's revision before the commit.
    #[must_use]
    pub fn expected_revision(&self) -> LaneRevision {
        self.0.before.revision(self.0.lane)
    }

    /// The revision the commit produced: the lane's revision after the commit.
    #[must_use]
    pub fn proposed_revision(&self) -> LaneRevision {
        self.0.after.revision(self.0.lane)
    }
}

impl TryFrom<TransitionReceiptFields> for TransitionReceipt {
    type Error = ReceiptError;

    fn try_from(fields: TransitionReceiptFields) -> Result<Self, Self::Error> {
        Self::new(fields)
    }
}

impl From<TransitionReceipt> for TransitionReceiptFields {
    fn from(r: TransitionReceipt) -> Self {
        *r.0
    }
}

/// Why a [`TransitionReceipt`] or an [`ActionName`] was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReceiptError {
    /// A receipt schema version this crate does not read.
    SchemaVersion(u32),
    /// Counters that are not one commit of the lane apart.
    Counters {
        /// The receipt's lane.
        lane: Lane,
        /// The counters before the commit.
        before: Counters,
        /// The counters after the commit.
        after: Counters,
    },
    /// A control commit with effect intents.
    ControlEffects,
    /// Effect intents whose indexes do not strictly increase.
    EffectOrder,
    /// Disabled guards out of table order or repeated.
    DisabledGuards,
    /// A commit on this lane that changed the other lane's digest, which no commit made under
    /// the guards it lists can do.
    LanePartition(Lane),
    /// Text that is not an action name.
    ActionName(String),
}

impl fmt::Display for ReceiptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SchemaVersion(v) => write!(
                f,
                "receipt schema version {v} is not {RECEIPT_SCHEMA_VERSION}"
            ),
            Self::Counters {
                lane,
                before,
                after,
            } => write!(
                f,
                "counters {after:?} are not one {lane} commit after {before:?}"
            ),
            Self::ControlEffects => f.write_str("a control commit carries effect intents"),
            Self::EffectOrder => f.write_str("effect indexes do not strictly increase"),
            Self::DisabledGuards => {
                f.write_str("disabled guards are out of table order or repeated")
            }
            Self::LanePartition(lane) => write!(
                f,
                "a {lane} commit changed the other lane's digest under the guards it lists"
            ),
            Self::ActionName(s) => write!(f, "`{s}` is not an action name"),
        }
    }
}

impl std::error::Error for ReceiptError {}
