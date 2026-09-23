//! What every contract suite reports.

use std::fmt;

/// One rule an adapter broke, as a contract suite found it.
///
/// `R` is the suite's rule enum, such as [`ProviderRule`](crate::provider::ProviderRule).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation<R> {
    /// The rule.
    pub rule: R,
    /// What the suite observed.
    pub detail: String,
}

impl<R: fmt::Debug> fmt::Display for Violation<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.rule, self.detail)
    }
}

/// The outcome of a contract suite: `Ok(())`, or every violation it found.
pub type SuiteResult<R> = Result<(), Vec<Violation<R>>>;

/// Collects the violations of one suite run.
#[derive(Debug)]
pub(crate) struct Checker<R> {
    violations: Vec<Violation<R>>,
}

impl<R> Checker<R> {
    /// An empty checker.
    pub(crate) fn new() -> Self {
        Self {
            violations: Vec::new(),
        }
    }

    /// Records a violation of `rule` unless `holds`; `detail` describes what was observed.
    pub(crate) fn check(&mut self, holds: bool, rule: R, detail: impl FnOnce() -> String) {
        if !holds {
            self.violations.push(Violation {
                rule,
                detail: detail(),
            });
        }
    }

    /// Records a violation of `rule`.
    pub(crate) fn fail(&mut self, rule: R, detail: String) {
        self.violations.push(Violation { rule, detail });
    }

    /// The suite's result.
    pub(crate) fn finish(self) -> SuiteResult<R> {
        if self.violations.is_empty() {
            Ok(())
        } else {
            Err(self.violations)
        }
    }
}

/// Test helpers shared by the suites' own tests.
#[cfg(test)]
pub(crate) mod testing {
    use super::SuiteResult;
    use std::fmt::Debug;

    /// The rules `result` reports broken, deduplicated and in first-seen order.
    pub(crate) fn broken<R: PartialEq + Clone>(result: &SuiteResult<R>) -> Vec<R> {
        let mut rules: Vec<R> = Vec::new();
        if let Err(violations) = result {
            for v in violations {
                if !rules.contains(&v.rule) {
                    rules.push(v.rule.clone());
                }
            }
        }
        rules
    }

    /// Asserts that `result` reports `rule` broken.
    pub(crate) fn assert_breaks<R: PartialEq + Clone + Debug>(result: &SuiteResult<R>, rule: &R) {
        let rules = broken(result);
        assert!(
            rules.contains(rule),
            "expected {rule:?} to be broken, the suite reported {rules:?}"
        );
    }
}
