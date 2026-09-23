//! The profile: the proposed limits of `docs/design/AUTOBOT-M0-AND-GATES.md` §2 as typed
//! configuration with a stable digest.
//!
//! A profile is a versioned TOML document. [`Profile::parse`] reads it from a `&str`: this
//! module reads no file, so the operator and the tests supply the text, for M0 the contents of
//! `profiles/m0.toml`. The document's `version` key is its schema version; this module accepts
//! [`SCHEMA_VERSION`] only, and a document with an unknown key, a missing key or a zero where a
//! [`NonZeroU32`](std::num::NonZeroU32) is expected is refused. Each design area is one TOML
//! table ([`ProfileValues`] lists them) and each key ends in its unit, in the unit the design
//! states it in (`window_days`, `entry_max_kib`, `max_active_work_secs`).
//!
//! [`Profile::digest`] is the SHA-256 of the canonical form of the values: one `<path> =
//! <value>` line per leaf, sorted, described on the digest module's `canonical_form`. The digest
//! therefore changes with any value and with the schema version, and not with key order,
//! layout or comments. Gate evidence binds to it (M0 §4).
//!
//! Choices this module makes where the design is open:
//!
//! - The profile is a versioned TOML document identified by that digest.
//! - The recovery point's uncheckpointed part is [`Checkpoint::max_active_work_secs`], not a
//!   second value.
//! - The rules the design states beside the limits, such as FIFO order within priority, a full
//!   queue stopping admission, oversize input being rejected or referenced, the artifact store's
//!   properties and new writes stopping when the checkpoint age exceeds its bound, are
//!   behaviour, not values, and have no key.
//! - The liveness bounds of `docs/design/AUTOBOT-FORMAL-SURFACE.md` §6 are fixture constants
//!   chosen per fixture, so they are parameters of `autobot-testkit`, not profile values.
//! - [`schemars::JsonSchema`] is derived for the types a custom resource spec carries: the
//!   [`Sandbox`] constraints, which an execution profile pins, and [`ProfileDigest`].

mod digest;
mod values;

pub use digest::{DigestParseError, ProfileDigest};
pub use values::{
    ApiBudget, Artifacts, Checkpoint, ControlRing, ControlWork, DefectMaturity, DispatchLedger,
    Evidence, Objects, ProfileValues, Recovery, Registers, Replay, Sandbox, SandboxEgress,
    SandboxImage, SandboxModelApi, SandboxNetwork, SandboxOs, Scale,
};

use std::fmt;

/// The schema version [`Profile::parse`] accepts.
pub const SCHEMA_VERSION: u32 = 1;

/// A parsed, checked profile and its digest.
///
/// The values are read-only, so the digest always describes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    values: ProfileValues,
    digest: ProfileDigest,
}

impl Profile {
    /// Parses and checks the profile document `toml`.
    ///
    /// # Errors
    ///
    /// [`ProfileError::Toml`] if `toml` is not TOML or does not match [`ProfileValues`],
    /// [`ProfileError::Version`] if its version is not [`SCHEMA_VERSION`], and
    /// [`ProfileError::Invalid`] if its values contradict each other.
    pub fn parse(toml: &str) -> Result<Self, ProfileError> {
        let values: ProfileValues =
            toml::from_str(toml).map_err(|e| ProfileError::Toml(e.to_string()))?;
        if values.version != SCHEMA_VERSION {
            return Err(ProfileError::Version(values.version));
        }
        if values.objects.pending_slot_max_kib > values.objects.status_max_kib {
            return Err(ProfileError::Invalid(
                "objects.pending_slot_max_kib exceeds objects.status_max_kib".to_owned(),
            ));
        }
        let table =
            toml::Table::try_from(&values).map_err(|e| ProfileError::Toml(e.to_string()))?;
        let digest = ProfileDigest::of(&digest::canonical_form(&table));
        Ok(Self { values, digest })
    }

    /// The profile's values.
    #[must_use]
    pub fn values(&self) -> &ProfileValues {
        &self.values
    }

    /// The digest of the profile's values.
    #[must_use]
    pub fn digest(&self) -> ProfileDigest {
        self.digest
    }
}

/// Why a profile document was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProfileError {
    /// The document is not TOML, or its keys or value types do not match [`ProfileValues`].
    Toml(String),
    /// The document's schema version is not [`SCHEMA_VERSION`].
    Version(u32),
    /// Two values contradict each other.
    Invalid(String),
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toml(msg) => write!(f, "profile is not a valid document: {msg}"),
            Self::Version(v) => write!(
                f,
                "profile schema version {v} is not the supported version {SCHEMA_VERSION}"
            ),
            Self::Invalid(msg) => write!(f, "profile values contradict each other: {msg}"),
        }
    }
}

impl std::error::Error for ProfileError {}

#[cfg(test)]
mod tests;
