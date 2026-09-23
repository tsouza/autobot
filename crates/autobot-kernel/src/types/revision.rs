//! The per-lane revisions, the commit sequence and the commit lanes.

use crate::error::ValueError;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt;

/// The largest value a counter takes: Kubernetes stores integers as `int64`.
const MAX: u64 = i64::MAX.unsigned_abs();

/// Defines a counter newtype over `u64` restricted to `0..=i64::MAX`.
macro_rules! counter {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        ///
        /// Its value lies in `0..=i64::MAX`; zero means no commit has been made. Serde and the
        /// JSON schema use a non-negative `int64` integer.
        #[derive(
            Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize,
            Deserialize,
        )]
        #[serde(try_from = "u64", into = "u64")]
        pub struct $name(u64);

        impl $name {
            /// The value before the first commit.
            pub const ZERO: Self = Self(0);

            /// The counter with value `value`.
            ///
            /// # Errors
            ///
            /// [`ValueError::OutOfRange`] if `value` exceeds `i64::MAX`.
            pub fn new(value: u64) -> Result<Self, ValueError> {
                if value > MAX {
                    Err(ValueError::OutOfRange(value))
                } else {
                    Ok(Self(value))
                }
            }

            /// The counter's value.
            #[must_use]
            pub fn get(self) -> u64 {
                self.0
            }

            /// The counter one past this one.
            ///
            /// # Errors
            ///
            /// [`ValueError::OutOfRange`] if this counter is `i64::MAX`.
            pub fn next(self) -> Result<Self, ValueError> {
                Self::new(self.0.saturating_add(1))
            }
        }

        impl TryFrom<u64> for $name {
            type Error = ValueError;

            fn try_from(value: u64) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for u64 {
            fn from(c: $name) -> Self {
                c.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }

            fn json_schema(generator: &mut SchemaGenerator) -> Schema {
                counter_schema(generator)
            }
        }
    };
}

counter!(
    /// `state_revision`: the domain lane's precondition, incremented by every domain commit.
    StateRevision
);

counter!(
    /// `control_revision`: the control lane's precondition, incremented by every control commit.
    ControlRevision
);

counter!(
    /// `commit_sequence`: incremented by every commit of either lane, so it totally orders the
    /// commits on one aggregate.
    CommitSequence
);

/// A commit lane.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Lane {
    /// The domain lane: precondition `state_revision`, installs the pending commit.
    Domain,
    /// The control lane: precondition `control_revision`, appends a control receipt.
    Control,
}

impl fmt::Display for Lane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Domain => "DOMAIN",
            Self::Control => "CONTROL",
        })
    }
}

/// A revision of one lane: what a command pins as its expected revision.
///
/// Serde and the JSON schema use the object `{lane, revision}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "LaneRevisionWire", into = "LaneRevisionWire")]
pub enum LaneRevision {
    /// A `state_revision`.
    State(StateRevision),
    /// A `control_revision`.
    Control(ControlRevision),
}

impl LaneRevision {
    /// The lane the revision belongs to.
    #[must_use]
    pub fn lane(self) -> Lane {
        match self {
            Self::State(_) => Lane::Domain,
            Self::Control(_) => Lane::Control,
        }
    }

    /// The revision's value.
    #[must_use]
    pub fn get(self) -> u64 {
        match self {
            Self::State(r) => r.get(),
            Self::Control(r) => r.get(),
        }
    }
}

impl fmt::Display for LaneRevision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.lane(), self.get())
    }
}

/// The serde form of a [`LaneRevision`].
#[derive(Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "LaneRevision")]
struct LaneRevisionWire {
    /// The lane.
    lane: Lane,
    /// The revision in that lane.
    #[schemars(schema_with = "counter_schema")]
    revision: u64,
}

/// The JSON schema of a counter: a non-negative `int64` integer.
fn counter_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "integer", "format": "int64", "minimum": 0, "maximum": i64::MAX })
}

impl TryFrom<LaneRevisionWire> for LaneRevision {
    type Error = ValueError;

    fn try_from(w: LaneRevisionWire) -> Result<Self, Self::Error> {
        Ok(match w.lane {
            Lane::Domain => Self::State(StateRevision::new(w.revision)?),
            Lane::Control => Self::Control(ControlRevision::new(w.revision)?),
        })
    }
}

impl From<LaneRevision> for LaneRevisionWire {
    fn from(r: LaneRevision) -> Self {
        Self {
            lane: r.lane(),
            revision: r.get(),
        }
    }
}
