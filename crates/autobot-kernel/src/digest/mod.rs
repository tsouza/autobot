//! The canonical encoding and the digests of `docs/design/AUTOBOT-KERNEL.md` §1–§3.3: the
//! domain, control, input, payload and event digests, `operation_key`, and the deterministic
//! names of receipts, creates, `EffectIntent` and `ExternalOperation`.
//!
//! Every digest is SHA-256 over one version byte followed by the RFC 8949 core deterministic
//! CBOR encoding of a value (RFC 8949 §4.2.1): shortest-form integers and lengths,
//! definite-length items only, and map entries sorted by the bytes of their encoded keys. A
//! value reaches the encoder through serde, so a record encodes as its serde form: a struct as
//! a map from its serialized field names, a unit variant as its name, a [`Digest`] as its
//! `sha256:` text.
//!
//! - [`canonical_encoding`] is that encoding, and [`digest`] the digest of it. A command's
//!   input digest and an effect's payload digest are [`digest`] of the input and the payload.
//! - [`event_digest`] is the `event_digest` of an `AutoBotEvent`: the digest of its
//!   [`AuditEnvelope`], which holds every other field of the event.
//! - [`domain_digest`] and [`control_digest`] are the digests of a status's domain fields
//!   only and control fields only, by the partition its [`FieldClasses`] declares.
//!   Reconciliation fields, the pending slot body, the control-receipt ring and the other
//!   structural fields are in neither.
//! - [`operation_key`] is `hash(installation_lineage, aggregate_uid, committed_revision,
//!   effect_index, payload_digest)` (KERNEL §3.3), and [`intent_operation_key`] derives it
//!   for an intent a pending slot holds.
//! - [`object_name`] and the functions beside it give the deterministic names.
//!
//! # Golden vectors
//!
//! ```
//! use autobot_kernel::digest::{canonical_encoding, digest, operation_key};
//! use autobot_kernel::types::{StateRevision, Uid};
//! use std::collections::BTreeMap;
//!
//! // Map keys sort by their encoded bytes, so the shorter key comes first.
//! let map = BTreeMap::from([("aa", 1u8), ("b", 2)]);
//! assert_eq!(canonical_encoding(&map)?, [0xa2, 0x61, b'b', 0x02, 0x62, b'a', b'a', 0x01]);
//! assert_eq!(
//!     digest(&map)?.to_string(),
//!     "sha256:5ebbde31438120d8d7d23d3d0297ed8765d4b7e9ec13fc9a891d01878cf01a77",
//! );
//!
//! let payload = digest("payload")?;
//! let key = operation_key(
//!     "lineage-1",
//!     &"aggregate-uid".parse::<Uid>()?,
//!     StateRevision::new(7)?,
//!     0,
//!     &payload,
//! );
//! assert_eq!(key.to_string(), "sha256:ee0bb97dca9f59e7753a78a62c971d9d959933acf460bb2e8b56aeb89fa6f653");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ```
//! use autobot_kernel::digest::{
//!     CreateIndex, command_receipt_name, effect_intent_name, external_operation_name,
//! };
//! use autobot_kernel::types::Digest;
//!
//! let key: Digest = "sha256:ee0bb97dca9f59e7753a78a62c971d9d959933acf460bb2e8b56aeb89fa6f653".parse()?;
//! assert_eq!(effect_intent_name(&key)?.as_str(), "effectintent-5yf3s7okt5m6o5j2pctczfy5twkzsm5m");
//! assert_eq!(external_operation_name(&key)?.as_str(), "externaloperation-5yf3s7okt5m6o5j2pctczfy5twkzsm5m");
//! assert_eq!(command_receipt_name("key-1")?.as_str(), "commandreceipt-4dfglol4w5robys5wweqb5q7wsvpuxqo");
//!
//! let create = CreateIndex {
//!     context_uid: "context-uid".parse()?,
//!     principal: "system:serviceaccount:autobot:planner".parse()?,
//!     kind: "TaskRun".to_owned(),
//!     parent_uid: Some("task-uid".parse()?),
//!     client_request_key: "request-1".to_owned(),
//! };
//! assert_eq!(create.target_name()?.as_str(), "taskrun-ytlal4nv7ea6anomindphyrrrvb4ndph");
//! assert_eq!(create.receipt_name()?.as_str(), "commandreceipt-ytlal4nv7ea6anomindphyrrrvb4ndph");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Choices this module makes where the design is open:
//!
//! - The version byte is [`ENCODING_VERSION`], 1. The identities — [`operation_key`],
//!   [`CreateIndex::digest`] and every name — are computed at version 1 whatever
//!   [`ENCODING_VERSION`] becomes, so they stay stable across versions; a later encoding
//!   changes only the digests that are recomputed from current state.
//! - Floating-point numbers are refused ([`EncodeError::Float`]): no kernel record carries
//!   one, and refusing them avoids choosing a float form. Integers are CBOR integers, so a
//!   128-bit integer outside `-2^64..2^64` is refused. `None` and `()` encode as `null`, and a
//!   field serde skips is absent from the map.
//! - A digest carries no tag naming what it digests: the version byte is the only prefix, and
//!   two roles never compare their digests with each other.
//! - The domain and control digests are computed over the serde form of the status projected
//!   onto one class. A serialized field is matched to its declared field by name with
//!   underscores removed and ASCII case folded, which covers serde's snake-case and
//!   camel-case renames (`observedGeneration`); a serialized field that matches no declared
//!   field, or two declared fields that fold to one name, is an error rather than a guess.
//!   A struct keeps the fields of the class, and is dropped when none remain unless its own
//!   presence is of the class; a keyed map keeps the entries whose projection is not empty, and
//!   a list keeps its positions, a member with nothing of the class standing as `null`, unless
//!   no member has anything of the class and membership is of another class. The digest of a
//!   status with nothing of the class is the digest of the empty map.
//! - KERNEL §1 reads an absent control field as its initial value, and creating a keyed entry
//!   leaves the control digest unchanged. The digest reads the serde form, so a kind whose
//!   entries are created with a control field at its initial value serializes that value as
//!   absent for the two to agree, which [`check_initial_entry`] checks for one entry.
//! - [`operation_key`] is the digest of the array `[installation_lineage, aggregate_uid,
//!   committed_revision, effect_index, payload_digest]`, the design's order. The
//!   `installation_lineage` is the one a pending slot's intent holds in place of its key, and
//!   `aggregate_uid` and `committed_revision` are the slot's own commit's (#384); no
//!   `installation_id` enters, so the key is preserved across restore (KERNEL §7).
//! - A name is the kind in lowercase, `-`, and the unpadded lowercase RFC 4648 base32 of the
//!   first 20 bytes (160 bits) of a digest: 32 characters, so the kind has at most
//!   [`MAX_NAME_KIND_LEN`] characters and the name is a DNS-1123 label.
//! - A `CommandReceipt` is named from its idempotency key alone and a create's receipt from its
//!   [`CreateIndex`] alone; the principal and input digest are compared, not incorporated
//!   (KERNEL §2). A create's object is named from the same index under its own kind.
//! - An `EffectIntent` and its `ExternalOperation` are named from the `operation_key` itself,
//!   which is already a digest of the intent's identity, so the two names share a suffix.
//!
//! [`AuditEnvelope`]: crate::status::AuditEnvelope
//! [`FieldClasses`]: crate::fields::FieldClasses

mod cbor;
mod name;
mod project;

pub use name::{
    CreateIndex, MAX_NAME_KIND_LEN, command_receipt_name, effect_intent_name,
    external_operation_name, object_name,
};

use crate::fields::{FieldClass, FieldClasses};
use crate::status::{AuditEnvelope, SlotEffectIntent};
use crate::types::{Digest, StateRevision, Uid};
use cbor::{Identity, Value};
use serde::Serialize;
use sha2::Sha256;
use std::fmt;

/// The version byte that prefixes the encoding every digest is computed over.
pub const ENCODING_VERSION: u8 = 1;

/// The version byte of the identities: `operation_key`, create indexes and names.
const IDENTITY_VERSION: u8 = 1;

/// Why a value has no canonical encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EncodeError {
    /// The value holds a floating-point number.
    Float,
    /// The value holds a 128-bit integer outside the CBOR integer range.
    IntegerRange,
    /// A map holds two keys with the same encoding.
    DuplicateKey,
    /// The serialized status holds a field its type does not declare; the payload is its path.
    UnknownField(String),
    /// Two declared fields fold to the same name; the payload is the second one's path.
    AmbiguousField(String),
    /// A serialized value does not have the shape its declaration implies; the payload is its
    /// path.
    Shape(String),
    /// A keyed entry at its initial values serializes a control field; the payload is the
    /// field's serialized name.
    InitialControl(String),
    /// A `Serialize` implementation failed.
    Custom(String),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Float => f.write_str("floating-point numbers have no canonical encoding"),
            Self::IntegerRange => f.write_str("an integer is outside the CBOR integer range"),
            Self::DuplicateKey => f.write_str("a map holds two keys with the same encoding"),
            Self::UnknownField(p) => write!(f, "`{p}` is not a declared field"),
            Self::AmbiguousField(p) => {
                write!(f, "`{p}` folds to the name of another declared field")
            }
            Self::Shape(p) => write!(f, "`{p}` does not have its declared shape"),
            Self::InitialControl(n) => {
                write!(
                    f,
                    "`{n}` is a control field serialized at its initial value"
                )
            }
            Self::Custom(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for EncodeError {}

impl serde::ser::Error for EncodeError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self::Custom(msg.to_string())
    }
}

/// SHA-256 of `version` followed by `encoding`.
fn sha256(version: u8, encoding: &[u8]) -> Digest {
    use sha2::Digest as _;
    let mut hasher = Sha256::new();
    hasher.update([version]);
    hasher.update(encoding);
    Digest::from_bytes(hasher.finalize().into())
}

/// The digest of `value` at the current encoding version.
fn value_digest(value: &Value) -> Result<Digest, EncodeError> {
    Ok(sha256(ENCODING_VERSION, &value.encode()?))
}

/// The digest of an identity value, at [`IDENTITY_VERSION`].
fn identity_digest(value: &Identity) -> Digest {
    sha256(IDENTITY_VERSION, &value.encode())
}

/// The RFC 8949 core deterministic CBOR encoding of `value`'s serde form.
///
/// # Errors
///
/// [`EncodeError::Float`], [`EncodeError::IntegerRange`] or [`EncodeError::DuplicateKey`] for a
/// value outside the encoding, and [`EncodeError::Custom`] if serialization fails.
pub fn canonical_encoding<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, EncodeError> {
    cbor::to_value(value)?.encode()
}

/// The digest of `value`: SHA-256 of [`ENCODING_VERSION`] followed by its
/// [`canonical_encoding`]. A command's input digest and an effect's payload digest are this
/// digest of the input and the payload.
///
/// # Errors
///
/// As [`canonical_encoding`].
pub fn digest<T: Serialize + ?Sized>(value: &T) -> Result<Digest, EncodeError> {
    value_digest(&cbor::to_value(value)?)
}

/// The `event_digest` of the `AutoBotEvent` whose other fields are `envelope`.
///
/// # Errors
///
/// As [`canonical_encoding`]; an [`AuditEnvelope`] holds nothing it refuses.
pub fn event_digest(envelope: &AuditEnvelope) -> Result<Digest, EncodeError> {
    digest(envelope)
}

/// The domain digest of `status`: the digest of its domain fields only.
///
/// # Errors
///
/// As [`canonical_encoding`], and [`EncodeError::UnknownField`], [`EncodeError::AmbiguousField`]
/// or [`EncodeError::Shape`] when the serde form of `status` does not follow the fields `S`
/// declares.
pub fn domain_digest<S: Serialize + FieldClasses>(status: &S) -> Result<Digest, EncodeError> {
    class_digest(status, FieldClass::Domain)
}

/// The control digest of `status`: the digest of its control fields only.
///
/// # Errors
///
/// As [`domain_digest`].
pub fn control_digest<S: Serialize + FieldClasses>(status: &S) -> Result<Digest, EncodeError> {
    class_digest(status, FieldClass::Control)
}

/// Checks that `entry`, an entry of a keyed status map with every control field at its
/// initial value, serializes no control field (KERNEL §1, keyed entries).
///
/// A domain commit creates such an entry by writing its domain fields only, and the control
/// digest must not change; since the digest reads the serde form, that holds exactly when the
/// initial entry serializes no control field. A kind calls this on each keyed entry type it
/// declares, with the entry as it is created.
///
/// # Errors
///
/// [`EncodeError::InitialControl`] naming the first control field `entry` serializes, and the
/// errors of [`control_digest`] for an `entry` that does not follow the fields `E` declares.
pub fn check_initial_entry<E: Serialize + FieldClasses>(entry: &E) -> Result<(), EncodeError> {
    match project::project::<E>(&cbor::to_value(entry)?, FieldClass::Control)? {
        Value::Map(fields) => match fields.into_iter().next() {
            None => Ok(()),
            Some((Value::Text(name), _)) => Err(EncodeError::InitialControl(name)),
            Some(_) => Err(EncodeError::Shape(String::new())),
        },
        _ => Err(EncodeError::Shape(String::new())),
    }
}

fn class_digest<S: Serialize + FieldClasses>(
    status: &S,
    class: FieldClass,
) -> Result<Digest, EncodeError> {
    value_digest(&project::project::<S>(&cbor::to_value(status)?, class)?)
}

/// The `operation_key` of KERNEL §3.3: the digest of `[installation_lineage, aggregate_uid,
/// committed_revision, effect_index, payload_digest]`.
///
/// Two keys are equal exactly when their five inputs are (F-21), up to SHA-256 collisions.
#[must_use]
pub fn operation_key(
    installation_lineage: &str,
    aggregate_uid: &Uid,
    committed_revision: StateRevision,
    effect_index: u32,
    payload_digest: &Digest,
) -> Digest {
    identity_digest(&Identity::Array(vec![
        Identity::Text(installation_lineage.to_owned()),
        Identity::Text(aggregate_uid.as_str().to_owned()),
        Identity::Uint(committed_revision.get()),
        Identity::Uint(effect_index.into()),
        Identity::Text(payload_digest.to_string()),
    ]))
}

/// The `operation_key` of `intent`, held in the pending slot of the commit on the aggregate
/// `aggregate_uid` that produced `committed_revision`, the slot's `proposed_revision` (#384).
#[must_use]
pub fn intent_operation_key(
    intent: &SlotEffectIntent,
    aggregate_uid: &Uid,
    committed_revision: StateRevision,
) -> Digest {
    operation_key(
        &intent.installation_lineage,
        aggregate_uid,
        committed_revision,
        intent.effect_index,
        &intent.payload_digest,
    )
}

#[cfg(test)]
mod tests;
