//! The digest value a status record carries, and the `sha256:` text form every kernel digest
//! uses.

use crate::error::ValueError;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

/// The prefix of the text form of a [`Digest`].
const PREFIX: &str = "sha256:";

/// A SHA-256 digest carried by a record: a before, after, audit, payload or profile digest.
///
/// Its text form, used by `Display`, `FromStr` and serde, is `sha256:` followed by 64
/// lowercase hexadecimal digits. This type carries a digest and does not compute one.
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
        f.write_str(PREFIX)?;
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl FromStr for Digest {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ValueError::Digest(s.to_owned());
        let hex = s.strip_prefix(PREFIX).ok_or_else(err)?.as_bytes();
        if hex.len() != 64 {
            return Err(err());
        }
        let nibble = |c: u8| match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        };
        let mut out = [0u8; 32];
        for (byte, &[hi, lo]) in out.iter_mut().zip(hex.as_chunks::<2>().0) {
            let (hi, lo) = (nibble(hi), nibble(lo));
            *byte = (hi.ok_or_else(err)? << 4) | lo.ok_or_else(err)?;
        }
        Ok(Self(out))
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

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": "^sha256:[0-9a-f]{64}$",
        })
    }
}
