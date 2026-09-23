//! The digest value a status record carries.

use crate::error::ValueError;
use crate::profile::ProfileDigest;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

/// A SHA-256 digest carried by a record: a before, after, audit or payload digest.
///
/// Its text form, used by `Display`, `FromStr` and serde, is the one [`ProfileDigest`] uses:
/// `sha256:` followed by 64 lowercase hexadecimal digits. This type carries a digest and does
/// not compute one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Digest([u8; 32]);

impl Digest {
    /// The digest whose bytes are `bytes`.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The 32 digest bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("sha256:")?;
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl FromStr for Digest {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<ProfileDigest>()
            .map(|d| Self(*d.as_bytes()))
            .map_err(|_| ValueError::Digest(s.to_owned()))
    }
}

impl TryFrom<String> for Digest {
    type Error = ValueError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<Digest> for String {
    fn from(d: Digest) -> Self {
        d.to_string()
    }
}

impl JsonSchema for Digest {
    fn schema_name() -> Cow<'static, str> {
        "Digest".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        ProfileDigest::json_schema(generator)
    }
}
