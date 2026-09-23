//! Trust classes and labelled text, as `docs/design/AUTOBOT-TRUST-MODEL.md` §Not trusted
//! states them.

/// The trust class every fact entering a decision carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TrustClass {
    /// A committed aggregate status or receipt.
    CanonicalFact,
    /// A fact from a forge, CI, user or infrastructure that was authenticated, deduplicated
    /// and persisted before acknowledgement.
    AuthenticatedObservation,
    /// Everything else: repository content, forge text, CI output, model output.
    UntrustedContent,
}

/// One piece of text with its trust class and a label saying what it is.
///
/// The class travels with the text: an adapter that passes the text on passes the class with
/// it, and nothing but the producer of a canonical fact labels text
/// [`TrustClass::CanonicalFact`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelledText {
    /// The text's trust class.
    pub class: TrustClass,
    /// What the text is, such as `capsule` or `issue body`.
    pub label: String,
    /// The text.
    pub content: String,
}

impl LabelledText {
    /// Text of class `class` labelled `label`.
    #[must_use]
    pub fn new(class: TrustClass, label: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            class,
            label: label.into(),
            content: content.into(),
        }
    }

    /// Untrusted text labelled `label`.
    #[must_use]
    pub fn untrusted(label: impl Into<String>, content: impl Into<String>) -> Self {
        Self::new(TrustClass::UntrustedContent, label, content)
    }
}
