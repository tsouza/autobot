//! The profile digest and the canonical form it is computed over.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

/// The prefix of the text form of a [`ProfileDigest`].
const PREFIX: &str = "sha256:";

/// SHA-256 of the canonical form of a profile's values.
///
/// Its text form, used by `Display`, `FromStr` and serde, is `sha256:` followed by 64
/// lowercase hexadecimal digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProfileDigest([u8; 32]);

impl ProfileDigest {
    /// The digest of `canonical`, a canonical form produced by [`canonical_form`].
    pub(super) fn of(canonical: &str) -> Self {
        Self(Sha256::digest(canonical.as_bytes()).into())
    }

    /// The 32 digest bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for ProfileDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(PREFIX)?;
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

/// Text that is not the text form of a [`ProfileDigest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestParseError(String);

impl fmt::Display for DigestParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not `{PREFIX}` followed by 64 lowercase hexadecimal digits",
            self.0
        )
    }
}

impl std::error::Error for DigestParseError {}

impl FromStr for ProfileDigest {
    type Err = DigestParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || DigestParseError(s.to_owned());
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

impl TryFrom<String> for ProfileDigest {
    type Error = DigestParseError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<ProfileDigest> for String {
    fn from(d: ProfileDigest) -> Self {
        d.to_string()
    }
}

impl JsonSchema for ProfileDigest {
    fn schema_name() -> Cow<'static, str> {
        "ProfileDigest".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": "^sha256:[0-9a-f]{64}$",
        })
    }
}

/// The canonical form of `table`: one line `<path> = <value>` per leaf, sorted by byte order.
///
/// A path joins the keys from the root with `.`. Integers and floats are written in Rust's
/// shortest decimal form, booleans as `true` or `false`, datetimes in RFC 3339, strings as TOML
/// basic strings with every `"`, `\` and control character escaped, arrays as `[` their values
/// joined by `, ` `]`, and a table inside an array as `{` its sorted lines joined by `, ` `}`.
/// The form does not depend on the key order, layout or comments of the document the values
/// were read from.
pub(super) fn canonical_form(table: &toml::Table) -> String {
    let mut lines = Vec::new();
    leaves(table, "", &mut lines);
    lines.sort();
    lines.concat()
}

/// Appends one line per leaf of `table`, whose keys are prefixed by `prefix`.
fn leaves(table: &toml::Table, prefix: &str, out: &mut Vec<String>) {
    for (key, value) in table {
        let path = format!("{prefix}{key}");
        match value {
            toml::Value::Table(t) => leaves(t, &format!("{path}."), out),
            v => out.push(format!("{path} = {}\n", scalar(v))),
        }
    }
}

/// The canonical text of a non-table value.
fn scalar(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => {
            let mut out = String::from('"');
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    c if c.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(c))),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        }
        toml::Value::Array(items) => {
            let items: Vec<String> = items.iter().map(scalar).collect();
            format!("[{}]", items.join(", "))
        }
        toml::Value::Table(t) => {
            let mut lines = Vec::new();
            leaves(t, "", &mut lines);
            lines.sort();
            format!("{{{}}}", lines.concat().trim_end().replace('\n', ", "))
        }
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(x) => x.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Datetime(d) => d.to_string(),
    }
}
