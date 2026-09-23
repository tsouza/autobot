//! Deterministic object names: the lowercased kind, `-`, and 32 base32 characters of a digest.

use super::cbor::Identity;
use super::identity_digest;
use crate::error::ValueError;
use crate::types::{Digest, Namespace, ObjectName, Principal, Uid};

/// The number of digest bytes a name keeps: 160 bits, 32 base32 characters.
const NAME_DIGEST_BYTES: usize = 20;

/// The longest kind a name can carry: a DNS-1123 label holds 63 characters, and the suffix
/// takes 33.
pub const MAX_NAME_KIND_LEN: usize = 63 - 1 - 32;

/// The RFC 4648 base32 alphabet in lowercase, every character of which a DNS-1123 label
/// admits.
const BASE32: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// The unpadded lowercase base32 text of `bytes`.
pub(super) fn base32(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let (mut acc, mut bits) = (0u16, 0u8);
    for &b in bytes {
        acc = (acc << 8) | u16::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(BASE32[usize::from((acc >> bits) & 31)]));
        }
        acc &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(char::from(BASE32[usize::from((acc << (5 - bits)) & 31)]));
    }
    out
}

/// The deterministic name of an object of kind `kind` whose identity has digest `identity`.
///
/// The name is `kind` in lowercase, `-`, and the unpadded lowercase base32 of the first 20
/// bytes of `identity`: a DNS-1123 label of at most 63 characters.
///
/// # Errors
///
/// [`ValueError::Namespace`] if the name is not a DNS-1123 label, which happens exactly when
/// `kind` is empty, longer than [`MAX_NAME_KIND_LEN`], starts with `-` or holds a character
/// other than an ASCII letter, digit or `-`.
pub fn object_name(kind: &str, identity: &Digest) -> Result<ObjectName, ValueError> {
    let kind = kind.to_ascii_lowercase();
    let suffix = base32(
        identity
            .as_bytes()
            .get(..NAME_DIGEST_BYTES)
            .unwrap_or_default(),
    );
    let name = format!("{kind}-{suffix}");
    let label: Namespace = name.parse()?;
    String::from(label).try_into()
}

/// The deterministic name of the `CommandReceipt` of a command with idempotency key
/// `idempotency_key` (KERNEL §2).
///
/// The principal and input digest of the replay identity are compared against the receipt,
/// not incorporated in its name, so the same key with another principal or payload finds the
/// receipt it conflicts with.
///
/// # Errors
///
/// None in practice: the kind is fixed. The result is checked as an [`ObjectName`].
pub fn command_receipt_name(idempotency_key: &str) -> Result<ObjectName, ValueError> {
    object_name(
        "CommandReceipt",
        &identity_digest(&Identity::Text(idempotency_key.to_owned())),
    )
}

/// The deterministic name of the `EffectIntent` materialized for `operation_key` (KERNEL §3.3).
///
/// # Errors
///
/// None in practice: the kind is fixed. The result is checked as an [`ObjectName`].
pub fn effect_intent_name(operation_key: &Digest) -> Result<ObjectName, ValueError> {
    object_name("EffectIntent", operation_key)
}

/// The deterministic name of the `ExternalOperation` of `operation_key` (KERNEL §3.3); it
/// shares its suffix with the name of the `EffectIntent` it is materialized from.
///
/// # Errors
///
/// None in practice: the kind is fixed. The result is checked as an [`ObjectName`].
pub fn external_operation_name(operation_key: &Digest) -> Result<ObjectName, ValueError> {
    object_name("ExternalOperation", operation_key)
}

/// The index of a create command (KERNEL §2): context, principal, kind, parent and client
/// request key.
///
/// The input digest is compared against the create's receipt, not incorporated in the index,
/// so a changed payload conflicts instead of creating a second object.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CreateIndex {
    /// The UID of the `WorkContext` the create belongs to.
    pub context_uid: Uid,
    /// The authenticated writer of the create.
    pub principal: Principal,
    /// The kind of the object to create, as Kubernetes names it (`TaskRun`).
    pub kind: String,
    /// The UID of the object's parent; absent for an object with none.
    pub parent_uid: Option<Uid>,
    /// The client request key the create carries.
    pub client_request_key: String,
}

impl CreateIndex {
    /// The digest of the index: of the array `[context_uid, principal, kind, parent_uid,
    /// client_request_key]`, an absent parent being `null`.
    #[must_use]
    pub fn digest(&self) -> Digest {
        let text = |s: &str| Identity::Text(s.to_owned());
        identity_digest(&Identity::Array(vec![
            text(self.context_uid.as_str()),
            text(self.principal.as_str()),
            text(&self.kind),
            self.parent_uid
                .as_ref()
                .map_or(Identity::Null, |u| text(u.as_str())),
            text(&self.client_request_key),
        ]))
    }

    /// The deterministic name of the create's `CommandReceipt`.
    ///
    /// # Errors
    ///
    /// None in practice: the kind is fixed. The result is checked as an [`ObjectName`].
    pub fn receipt_name(&self) -> Result<ObjectName, ValueError> {
        object_name("CommandReceipt", &self.digest())
    }

    /// The deterministic name the create reserves for its object, of kind `kind`; it shares
    /// its suffix with [`receipt_name`](Self::receipt_name).
    ///
    /// # Errors
    ///
    /// [`ValueError::Namespace`] under the conditions of [`object_name`] for `kind`.
    pub fn target_name(&self) -> Result<ObjectName, ValueError> {
        object_name(&self.kind, &self.digest())
    }
}
