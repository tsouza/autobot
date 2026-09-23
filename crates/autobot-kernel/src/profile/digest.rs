//! The profile digest and the canonical form it is computed over.

use crate::error::ValueError;
use crate::types::Digest;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

/// SHA-256 of the canonical form of a profile's values.
///
/// It wraps the [`Digest`] of that form, whose text form it uses for `Display`, `FromStr`,
/// serde and its JSON schema: `sha256:` followed by 64 lowercase hexadecimal digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProfileDigest(Digest);

impl ProfileDigest {
    /// The digest of `canonical`, a canonical form produced by [`canonical_form`].
    pub(super) fn of(canonical: &str) -> Self {
        use sha2::Digest as _;
        Self(Digest::from_bytes(Sha256::digest(canonical.as_bytes()).into()))
    }

    /// The 32 digest bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }
}

impl fmt::Display for ProfileDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for ProfileDigest {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

impl JsonSchema for ProfileDigest {
    fn schema_name() -> Cow<'static, str> {
        "ProfileDigest".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        Digest::json_schema(generator)
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
