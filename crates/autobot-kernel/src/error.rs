//! Errors of the kernel's value and status types.

use crate::types::{CommitSequence, LaneRevision};
use std::fmt;

/// A value that one of the kernel types refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValueError {
    /// An identifier that must not be empty is empty; the payload names the identifier.
    Empty(&'static str),
    /// Text that is not a DNS-1123 label, which a namespace must be.
    Namespace(String),
    /// Text that is not a DNS-1123 subdomain, which an object name must be.
    Name(String),
    /// Text that is not `sha256:` followed by 64 lowercase hexadecimal digits.
    Digest(String),
    /// A revision or commit sequence above `i64::MAX`, the largest integer Kubernetes stores.
    OutOfRange(u64),
    /// A rejection proof whose expected and observed revisions belong to different lanes.
    ProofLanes {
        /// The revision the command pinned.
        expected: LaneRevision,
        /// The revision read from the target.
        observed: LaneRevision,
    },
    /// A rejection proof whose observed revision is not past the expected one.
    ProofNotPassed {
        /// The revision the command pinned.
        expected: LaneRevision,
        /// The revision read from the target.
        observed: LaneRevision,
    },
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty(what) => write!(f, "{what} is empty"),
            Self::Namespace(s) => write!(f, "`{s}` is not a DNS-1123 label"),
            Self::Name(s) => write!(f, "`{s}` is not a DNS-1123 subdomain"),
            Self::Digest(s) => write!(
                f,
                "`{s}` is not `sha256:` followed by 64 lowercase hexadecimal digits"
            ),
            Self::OutOfRange(v) => write!(f, "{v} exceeds {}", i64::MAX),
            Self::ProofLanes { expected, observed } => write!(
                f,
                "expected revision {expected} and observed revision {observed} are on different lanes"
            ),
            Self::ProofNotPassed { expected, observed } => write!(
                f,
                "observed revision {observed} is not past expected revision {expected}"
            ),
        }
    }
}

impl std::error::Error for ValueError {}

/// Why a control-receipt ring refused an operation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RingError {
    /// The ring holds as many unpublished receipts as the profile allows; the control
    /// transition is refused and no receipt is dropped.
    Full {
        /// The number of unpublished receipts the ring holds.
        unpublished: usize,
    },
    /// A receipt whose commit sequence or control revision is not past the ring's last entry.
    OutOfOrder {
        /// The commit sequence of the ring's last entry.
        last: CommitSequence,
        /// The commit sequence of the refused receipt.
        refused: CommitSequence,
    },
    /// A receipt appended already marked `PUBLISHED`.
    AlreadyPublished(CommitSequence),
    /// No entry of the ring has this commit sequence.
    NotInRing(CommitSequence),
}

impl fmt::Display for RingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full { unpublished } => write!(
                f,
                "the control-receipt ring is full: {unpublished} receipts are unpublished"
            ),
            Self::OutOfOrder { last, refused } => write!(
                f,
                "receipt at commit sequence {refused} does not follow the ring's last entry at {last}"
            ),
            Self::AlreadyPublished(seq) => {
                write!(f, "receipt at commit sequence {seq} is already published")
            }
            Self::NotInRing(seq) => write!(f, "no ring entry has commit sequence {seq}"),
        }
    }
}

impl std::error::Error for RingError {}
