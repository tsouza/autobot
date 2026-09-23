//! The opaque identifiers adapters exchange: non-empty text that the party behind the adapter
//! assigns and AutoBot only compares.

use std::fmt;
use std::str::FromStr;

/// Text refused as an identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmptyText(
    /// The name of the identifier that was empty.
    pub &'static str,
);

impl fmt::Display for EmptyText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} is empty", self.0)
    }
}

impl std::error::Error for EmptyText {}

/// Defines an opaque, non-empty text newtype; `$what` names it in [`EmptyText`].
macro_rules! opaque {
    ($(#[$doc:meta])* $name:ident, $what:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// The identifier `text`.
            ///
            /// # Errors
            ///
            /// [`EmptyText`] if `text` is empty.
            pub fn new(text: impl Into<String>) -> Result<Self, EmptyText> {
                let text = text.into();
                if text.is_empty() {
                    Err(EmptyText($what))
                } else {
                    Ok(Self(text))
                }
            }

            /// The text as a string slice.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl FromStr for $name {
            type Err = EmptyText;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::new(s)
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
    /// The name of a provider behind a provider adapter, such as a forge or a CI service.
    ProviderName,
    "a provider name"
);
opaque!(
    /// The name of one operation a provider offers, such as a comment create or a merge.
    OperationName,
    "an operation name"
);
opaque!(
    /// The identity a provider gives the remote object an operation created or changed.
    RemoteIdentity,
    "a remote identity"
);
opaque!(
    /// The `target_identity` of an effect intent: the remote object the effect is about.
    TargetIdentity,
    "a target identity"
);
opaque!(
    /// A source or base head: the commit a branch pointed at when it was read.
    Head,
    "a head"
);
opaque!(
    /// The identifier a provider gives one delivered event.
    EventId,
    "an event id"
);
opaque!(
    /// A provider run: the identity of one CI run.
    RunIdentity,
    "a run identity"
);
opaque!(
    /// The key under which an artifact store keeps the versions of one artifact.
    ArtifactKey,
    "an artifact key"
);
opaque!(
    /// An `installation_id`: one installation of AutoBot, new on every restore.
    InstallationId,
    "an installation id"
);
opaque!(
    /// The class of a typed question.
    QuestionClass,
    "a question class"
);
opaque!(
    /// One option of an eligible set.
    OptionId,
    "an option id"
);
opaque!(
    /// The name of a tool an agent session calls.
    ToolName,
    "a tool name"
);
opaque!(
    /// A forge or CI actor as the provider authenticated it.
    Actor,
    "an actor"
);
