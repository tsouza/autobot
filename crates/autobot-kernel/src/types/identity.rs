//! Identities: UIDs, principals, namespaces, object names and namespaced references.

use crate::error::ValueError;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

/// Defines a checked text newtype: `$check` validates, `$schema` is its JSON schema.
macro_rules! text {
    ($(#[$doc:meta])* $name:ident, $check:expr, $schema:tt) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// The text as a string slice.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = ValueError;

            fn try_from(s: String) -> Result<Self, Self::Error> {
                let check: fn(&str) -> Result<(), ValueError> = $check;
                check(&s)?;
                Ok(Self(s))
            }
        }

        impl FromStr for $name {
            type Err = ValueError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                s.to_owned().try_into()
            }
        }

        impl From<$name> for String {
            fn from(v: $name) -> Self {
                v.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }

            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                json_schema!($schema)
            }
        }
    };
}

text!(
    /// The UID Kubernetes assigns an object: opaque, non-empty text.
    Uid,
    |s| non_empty(s, "a UID"),
    { "type": "string", "minLength": 1 }
);

text!(
    /// The authenticated writer of a command, bound at admission: opaque, non-empty text.
    Principal,
    |s| non_empty(s, "a principal"),
    { "type": "string", "minLength": 1 }
);

text!(
    /// A namespace name: a DNS-1123 label, at most 63 characters of `[a-z0-9-]` that start and
    /// end with a letter or digit.
    Namespace,
    |s| if is_label(s) { Ok(()) } else { Err(ValueError::Namespace(s.to_owned())) },
    {
        "type": "string",
        "minLength": 1,
        "maxLength": 63,
        "pattern": "^[a-z0-9]([-a-z0-9]*[a-z0-9])?$",
    }
);

text!(
    /// An object name: a DNS-1123 subdomain, at most 253 characters of DNS-1123 labels joined
    /// by `.`.
    ObjectName,
    |s| {
        if s.len() <= 253 && s.split('.').all(is_label) {
            Ok(())
        } else {
            Err(ValueError::Name(s.to_owned()))
        }
    },
    {
        "type": "string",
        "minLength": 1,
        "maxLength": 253,
        "pattern": "^[a-z0-9]([-a-z0-9]*[a-z0-9])?(\\.[a-z0-9]([-a-z0-9]*[a-z0-9])?)*$",
    }
);

/// Refuses empty text; `what` names the identifier in the error.
fn non_empty(s: &str, what: &'static str) -> Result<(), ValueError> {
    if s.is_empty() {
        Err(ValueError::Empty(what))
    } else {
        Ok(())
    }
}

/// Whether `s` is a DNS-1123 label.
fn is_label(s: &str) -> bool {
    let alnum = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    let bytes = s.as_bytes();
    match (bytes.first(), bytes.last()) {
        (Some(&first), Some(&last)) => {
            bytes.len() <= 63
                && alnum(first)
                && alnum(last)
                && bytes.iter().all(|&c| alnum(c) || c == b'-')
        }
        _ => false,
    }
}

/// A reference to one object: its namespace, name and UID.
///
/// The UID pins the reference to one incarnation of the name: an object deleted and created
/// again under the same name is a different object.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct ObjectRef {
    /// The object's namespace.
    pub namespace: Namespace,
    /// The object's name.
    pub name: ObjectName,
    /// The object's UID.
    pub uid: Uid,
}

impl fmt::Display for ObjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{} ({})", self.namespace, self.name, self.uid)
    }
}
