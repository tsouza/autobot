//! The objects the store holds: keys, resource versions, origins and status.

use crate as autobot_kernel;
use crate::error::ValueError;
use crate::fields::FieldClasses;
use crate::status::StatusEnvelope;
use crate::types::{Digest, Lane, LaneRevision, Namespace, ObjectName, Uid};
use serde::Serialize;
use std::fmt;
use std::str::FromStr;

/// Defines a non-empty opaque text newtype.
macro_rules! opaque {
    ($(#[$doc:meta])* $name:ident, $what:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// The text as a string slice.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl FromStr for $name {
            type Err = ValueError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                if s.is_empty() {
                    Err(ValueError::Empty($what))
                } else {
                    Ok(Self(s.to_owned()))
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

opaque!(
    /// The kind of a custom resource, such as `WorkContext`: non-empty text.
    Kind,
    "a kind"
);

opaque!(
    /// A `metadata.resourceVersion`: an opaque, non-empty token the store assigns on every
    /// write. The kernel compares two resource versions for equality only; their order is the
    /// store's.
    ResourceVersion,
    "a resource version"
);

/// The key of one object: kind, namespace and name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectKey {
    /// The object's kind.
    pub kind: Kind,
    /// The object's namespace.
    pub namespace: Namespace,
    /// The object's name.
    pub name: ObjectName,
}

impl fmt::Display for ObjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}/{}", self.kind, self.namespace, self.name)
    }
}

/// The immutable origin metadata a create writes beside the spec (KERNEL §2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Origin {
    /// The UID of the create command's receipt, which reserved the name.
    pub create_receipt_uid: Uid,
    /// The digest of the create command's input.
    pub input_digest: Digest,
    /// The UID of the work context the object belongs to.
    pub context_uid: Uid,
}

/// The status of an aggregate: the common envelope and the kind's own fields, split by the
/// field partition of KERNEL §1.
///
/// `domain` and `control` are the kind's encoded domain and control fields. The store moves
/// them as opaque text and never interprets them; a domain commit replaces `domain` only and a
/// control commit `control` only.
///
/// The status declares its field partition like any kind's: the envelope's classes, `domain`
/// as a domain field and `control` as a control field. Its domain and control digests are
/// therefore [`domain_digest`](crate::digest::domain_digest) and
/// [`control_digest`](crate::digest::control_digest) of the status itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, FieldClasses)]
pub struct Status {
    /// The envelope: revisions, commit sequence, pending slot and control-receipt ring.
    #[serde(flatten)]
    #[field(nested)]
    pub envelope: StatusEnvelope,
    /// The encoded domain fields.
    #[field(domain)]
    pub domain: String,
    /// The encoded control fields; empty on an aggregate without a control lane.
    #[field(control)]
    pub control: String,
}

impl Status {
    /// The revision of `lane`.
    #[must_use]
    pub fn revision(&self, lane: Lane) -> LaneRevision {
        match lane {
            Lane::Domain => LaneRevision::State(self.envelope.state_revision),
            Lane::Control => LaneRevision::Control(self.envelope.control_revision),
        }
    }
}

/// One object as the store returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    /// The object's key.
    pub key: ObjectKey,
    /// The UID the store assigned when it created the object.
    pub uid: Uid,
    /// The object's resource version.
    pub resource_version: ResourceVersion,
    /// The origin the create wrote.
    pub origin: Origin,
    /// The encoded spec.
    pub spec: String,
    /// The status; absent until the owning controller initializes it.
    pub status: Option<Status>,
}

impl From<u64> for ResourceVersion {
    /// The decimal text of `counter`, the resource version of a store that numbers its writes.
    fn from(counter: u64) -> Self {
        Self(counter.to_string())
    }
}
