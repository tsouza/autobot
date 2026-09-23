//! The semantic judge: answers a typed question over an eligible set computed before it was
//! asked, or abstains (`docs/design/AUTOBOT-TRUST-MODEL.md` §Not trusted; TypeSafe decision
//! policy, Decision 1).
//!
//! Deterministic code computes the facts and the [`EligibleSet`] first, asks through
//! [`SemanticJudge::ask`], and decides with [`TypedQuestion::admit`]: a selection inside the
//! eligible set is admitted, and anything else, an abstention, an unavailable judge, a malformed
//! answer or a selection outside the set, takes the question class's conservative branch. The
//! judge never widens a permission; its answer is untrusted model output and its confidence
//! establishes nothing.
//!
//! Choices this module makes where the design is open:
//!
//! - A question's evidence is a list of [`LabelledText`] items, each with its trust class; the
//!   evidence digest binds them. Which classes a question class accepts is the decision policy's
//!   (G-INTAKE) and is not checked here.
//! - A selection outside the eligible set is treated like an abstention, not as an error of
//!   the caller.
//! - What becomes of an admitted answer, a `Decision` record, follows the owner's ruling on
//!   #310: a `Decision` records a selection and is immutable; an open question is an
//!   `Intervention` or a `Finding`, not a `Decision` awaiting an answer.

mod contract;

pub use contract::{JudgeHarness, JudgeMode, JudgeRule, run};

use crate::text::{OptionId, QuestionClass};
use crate::trust::LabelledText;
use autobot_kernel::types::Digest;
use std::fmt;

/// Why a list of options is not an eligible set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EligibleSetError {
    /// The list is empty: there is nothing to select, so the question is not asked.
    Empty,
    /// The list names this option more than once.
    Repeated(OptionId),
}

impl fmt::Display for EligibleSetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the eligible set is empty"),
            Self::Repeated(o) => write!(f, "the eligible set names {o} more than once"),
        }
    }
}

impl std::error::Error for EligibleSetError {}

/// The options a judge may select from: non-empty, each named once, in the order given.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EligibleSet(Vec<OptionId>);

impl EligibleSet {
    /// The eligible set of `options`.
    ///
    /// # Errors
    ///
    /// [`EligibleSetError`] when `options` is empty or repeats an option.
    pub fn new(options: Vec<OptionId>) -> Result<Self, EligibleSetError> {
        if options.is_empty() {
            return Err(EligibleSetError::Empty);
        }
        for (i, o) in options.iter().enumerate() {
            if options[..i].contains(o) {
                return Err(EligibleSetError::Repeated(o.clone()));
            }
        }
        Ok(Self(options))
    }

    /// The options.
    #[must_use]
    pub fn options(&self) -> &[OptionId] {
        &self.0
    }

    /// Whether `option` is eligible.
    #[must_use]
    pub fn contains(&self, option: &OptionId) -> bool {
        self.0.contains(option)
    }
}

/// A typed question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedQuestion {
    /// The question class.
    pub class: QuestionClass,
    /// The digest of the evidence.
    pub evidence_digest: Digest,
    /// The evidence, each item with its trust class.
    pub evidence: Vec<LabelledText>,
    /// The options the judge may select from.
    pub eligible: EligibleSet,
}

/// A judge's confidence in a selection, in `0.0..=1.0`: a summary of a distribution, not a
/// probability of success.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Confidence(f64);

impl Confidence {
    /// The confidence `value`, if it is a finite number in `0.0..=1.0`.
    #[must_use]
    pub fn new(value: f64) -> Option<Self> {
        (0.0..=1.0).contains(&value).then_some(Self(value))
    }

    /// The value.
    #[must_use]
    pub fn get(self) -> f64 {
        self.0
    }
}

/// A judge's answer.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// The judge selected an option.
    Selected {
        /// The option.
        option: OptionId,
        /// The judge's confidence.
        confidence: Confidence,
    },
    /// The judge declined to select.
    Abstained,
}

/// Why a judge gave no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JudgeError {
    /// The judge is unavailable.
    Unavailable,
    /// The judge's answer could not be read as an [`Answer`].
    Malformed,
}

/// What deterministic code takes from an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admitted {
    /// The eligible option the judge selected.
    Selected(OptionId),
    /// The question class's conservative branch.
    ConservativeBranch,
}

impl TypedQuestion {
    /// What deterministic code takes from `answer`: the selected option when it is eligible,
    /// the conservative branch otherwise.
    #[must_use]
    pub fn admit(&self, answer: &Result<Answer, JudgeError>) -> Admitted {
        match answer {
            Ok(Answer::Selected { option, .. }) if self.eligible.contains(option) => {
                Admitted::Selected(option.clone())
            }
            _ => Admitted::ConservativeBranch,
        }
    }
}

/// A semantic judge client.
pub trait SemanticJudge {
    /// Asks `question`.
    ///
    /// # Errors
    ///
    /// [`JudgeError`] when the judge gave no readable answer.
    fn ask(&mut self, question: &TypedQuestion) -> Result<Answer, JudgeError>;
}

#[cfg(test)]
mod tests;
