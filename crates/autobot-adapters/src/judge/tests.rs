use super::*;
use crate::contract::testing::assert_breaks;

/// One way a broken double departs from the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Break {
    RemembersFirstOption,
    FollowsInjectedText,
    AbstainsWhenAble,
    AbstentionAsError,
    GuessesWhenUnavailable,
}

/// A deterministic judge that selects the last eligible option.
struct Double {
    mode: JudgeMode,
    broken: Option<Break>,
    first: Option<OptionId>,
}

impl Double {
    fn is(&self, b: Break) -> bool {
        self.broken == Some(b)
    }

    fn select(&mut self, q: &TypedQuestion) -> Answer {
        let last = q.eligible.options().last().cloned();
        let option = if self.is(Break::FollowsInjectedText) {
            Some("critical".parse().unwrap())
        } else if self.is(Break::RemembersFirstOption) {
            Some(
                self.first
                    .get_or_insert_with(|| last.clone().unwrap())
                    .clone(),
            )
        } else {
            last
        };
        Answer::Selected {
            option: option.unwrap(),
            confidence: Confidence::new(0.5).unwrap(),
        }
    }
}

impl SemanticJudge for Double {
    fn ask(&mut self, q: &TypedQuestion) -> Result<Answer, JudgeError> {
        match self.mode {
            JudgeMode::Answer if self.is(Break::AbstainsWhenAble) => Ok(Answer::Abstained),
            JudgeMode::Answer => Ok(self.select(q)),
            JudgeMode::Abstain if self.is(Break::AbstentionAsError) => Err(JudgeError::Malformed),
            JudgeMode::Abstain => Ok(Answer::Abstained),
            JudgeMode::Unavailable if self.is(Break::GuessesWhenUnavailable) => Ok(self.select(q)),
            JudgeMode::Unavailable => Err(JudgeError::Unavailable),
        }
    }
}

struct Harness(Option<Break>);

impl JudgeHarness for Harness {
    type Judge = Double;

    fn judge(&mut self, mode: JudgeMode) -> Double {
        Double {
            mode,
            broken: self.0,
            first: None,
        }
    }
}

#[test]
fn a_conforming_double_passes() {
    assert_eq!(run(&mut Harness(None)), Ok(()));
}

#[test]
fn each_broken_double_fails_its_rule() {
    let cases = [
        (Break::RemembersFirstOption, JudgeRule::SelectionInSet),
        (Break::FollowsInjectedText, JudgeRule::SelectionInSet),
        (Break::AbstainsWhenAble, JudgeRule::Answers),
        (Break::AbstentionAsError, JudgeRule::AbstentionIsAnswer),
        (Break::GuessesWhenUnavailable, JudgeRule::UnavailableIsError),
    ];
    for (broken, rule) in cases {
        assert_breaks(&run(&mut Harness(Some(broken))), &rule);
    }
}

fn option(s: &str) -> OptionId {
    s.parse().unwrap()
}

fn question() -> TypedQuestion {
    TypedQuestion {
        class: "severity".parse().unwrap(),
        evidence_digest: Digest::from_bytes([0; 32]),
        evidence: Vec::new(),
        eligible: EligibleSet::new(vec![option("low"), option("high")]).unwrap(),
    }
}

#[test]
fn only_an_eligible_selection_is_admitted() {
    let q = question();
    let selected = |o: &str| {
        Ok(Answer::Selected {
            option: option(o),
            confidence: Confidence::new(1.0).unwrap(),
        })
    };
    assert_eq!(
        q.admit(&selected("high")),
        Admitted::Selected(option("high"))
    );
    assert_eq!(q.admit(&selected("critical")), Admitted::ConservativeBranch);
    assert_eq!(
        q.admit(&Ok(Answer::Abstained)),
        Admitted::ConservativeBranch
    );
    assert_eq!(
        q.admit(&Err(JudgeError::Unavailable)),
        Admitted::ConservativeBranch
    );
    assert_eq!(
        q.admit(&Err(JudgeError::Malformed)),
        Admitted::ConservativeBranch
    );
}

#[test]
fn an_eligible_set_is_non_empty_and_names_each_option_once() {
    assert_eq!(EligibleSet::new(Vec::new()), Err(EligibleSetError::Empty));
    assert_eq!(
        EligibleSet::new(vec![option("a"), option("b"), option("a")]),
        Err(EligibleSetError::Repeated(option("a")))
    );
    let set = EligibleSet::new(vec![option("b"), option("a")]).unwrap();
    assert_eq!(set.options(), &[option("b"), option("a")]);
}

#[test]
fn confidence_is_a_number_in_the_unit_interval() {
    assert_eq!(Confidence::new(0.0).map(Confidence::get), Some(0.0));
    assert_eq!(Confidence::new(1.0).map(Confidence::get), Some(1.0));
    assert_eq!(Confidence::new(1.5), None);
    assert_eq!(Confidence::new(-0.1), None);
    assert_eq!(Confidence::new(f64::NAN), None);
}
