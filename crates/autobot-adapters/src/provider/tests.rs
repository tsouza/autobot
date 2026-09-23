use super::*;
use crate::backoff::BackOff;
use crate::contract::testing::assert_breaks;
use std::collections::{BTreeMap, BTreeSet};

/// One way a broken double departs from the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Break {
    SendsUnqualified,
    IgnoresHeads,
    ObservesPending,
    DropsMarker,
    AnswersLookupWithoutCapability,
    LookupForgets,
    LookupInvents,
    AnswersDryRunWithoutCapability,
    DryRunApplies,
    AppliesResend,
    ClaimsDedup,
    HidesLostAck,
    RateLimitAsTimeout,
    AppliesWhenRateLimited,
    ClaimsNonApplicationUnauthoritatively,
    WithholdsAuthoritativeNonApplication,
    OmitsAuthoritativeBackOff,
    InventsBackOff,
}

fn name<T: std::str::FromStr>(s: &str) -> T
where
    T::Err: std::fmt::Debug,
{
    s.parse().unwrap()
}

fn cap(operation: &str, flags: [bool; 5], method: ReconciliationMethod) -> ProviderCapability {
    let [idempotency, lookup, marker, head_base, dry_run] = flags;
    ProviderCapability {
        provider: name("double"),
        operation: name(operation),
        supports_idempotency: idempotency,
        supports_lookup: lookup,
        supports_remote_marker: marker,
        requires_head_base: head_base,
        supports_dry_run: dry_run,
        rate_limit_authoritative: false,
        reconciliation_method: method,
        qualified: true,
    }
}

/// Capabilities that together declare and omit every semantic.
fn mixed() -> Vec<ProviderCapability> {
    let mut unqualified = cap("merge", [false; 5], ReconciliationMethod::HumanAdjudication);
    unqualified.qualified = false;
    let mut authoritative = cap("comment", [true; 5], ReconciliationMethod::ProviderLookup);
    authoritative.rate_limit_authoritative = true;
    vec![
        authoritative,
        cap("label", [false; 5], ReconciliationMethod::HumanAdjudication),
        cap(
            "push",
            [true, false, false, false, false],
            ReconciliationMethod::ProviderDeduplication,
        ),
        unqualified,
    ]
}

struct Double {
    caps: Vec<ProviderCapability>,
    broken: Option<Break>,
    applied: BTreeMap<(OperationName, OperationKey), (RemoteIdentity, u32)>,
    fault: Option<ProviderFault>,
    remotes: u32,
}

impl Double {
    fn is(&self, b: Break) -> bool {
        self.broken == Some(b)
    }

    fn cap(&self, op: &OperationName) -> Option<&ProviderCapability> {
        self.caps.iter().find(|c| &c.operation == op)
    }

    fn apply(&mut self, req: &SendRequest) -> SendAck {
        let dedup = self
            .cap(&req.operation)
            .is_some_and(|c| c.supports_idempotency)
            || self.is(Break::ClaimsDedup);
        let dedup = dedup && !self.is(Break::AppliesResend);
        self.remotes += 1;
        let fresh = name(&format!("remote-{}", self.remotes));
        let entry = self
            .applied
            .entry((req.operation.clone(), req.operation_key))
            .or_insert((fresh, 0));
        if dedup && entry.1 > 0 {
            return SendAck::Deduplicated(entry.0.clone());
        }
        entry.1 += 1;
        SendAck::Accepted(entry.0.clone())
    }
}

impl ProviderAdapter for Double {
    fn provider(&self) -> ProviderName {
        name("double")
    }

    fn capabilities(&self) -> Vec<ProviderCapability> {
        self.caps.clone()
    }

    fn send(&mut self, req: &SendRequest) -> Result<SendAck, SendError> {
        let cap = self.cap(&req.operation).cloned();
        let qualified = cap.as_ref().is_some_and(|c| c.qualified);
        if !qualified && !self.is(Break::SendsUnqualified) {
            return Err(SendError::Unqualified);
        }
        let heads = req.source_head.is_some() && req.base_head.is_some();
        if cap.as_ref().is_some_and(|c| c.requires_head_base)
            && !heads
            && !self.is(Break::IgnoresHeads)
        {
            return Err(SendError::MissingHeadBase);
        }
        match self.fault.take() {
            Some(ProviderFault::DroppedRequest) => {
                Err(SendError::Transport(TransportFault::Timeout))
            }
            Some(ProviderFault::LostAcknowledgement) => {
                let ack = self.apply(req);
                if self.is(Break::HidesLostAck) {
                    Ok(ack)
                } else {
                    Err(SendError::Transport(TransportFault::Disconnect))
                }
            }
            Some(ProviderFault::RateLimited(_)) if self.is(Break::RateLimitAsTimeout) => {
                Err(SendError::Transport(TransportFault::Timeout))
            }
            Some(ProviderFault::RateLimited(stated)) => {
                if self.is(Break::AppliesWhenRateLimited) {
                    self.apply(req);
                }
                let authoritative = cap.is_some_and(|c| c.rate_limit_authoritative);
                let proves_non_application = if authoritative {
                    !self.is(Break::WithholdsAuthoritativeNonApplication)
                } else {
                    self.is(Break::ClaimsNonApplicationUnauthoritatively)
                };
                let back_off = if self.is(Break::InventsBackOff) {
                    Some(BackOff { seconds: 1 })
                } else {
                    (!self.is(Break::OmitsAuthoritativeBackOff)).then_some(stated)
                };
                Err(SendError::RateLimited {
                    proves_non_application,
                    back_off,
                })
            }
            None => Ok(self.apply(req)),
        }
    }

    fn observe(
        &mut self,
        op: &OperationName,
        remote: &RemoteIdentity,
    ) -> Result<Observation, ProviderError> {
        let marker =
            self.cap(op).is_some_and(|c| c.supports_remote_marker) && !self.is(Break::DropsMarker);
        let found = self
            .applied
            .iter()
            .find(|((o, _), (r, _))| o == op && r == remote)
            .map(|((_, key), _)| *key);
        let outcome = match found {
            Some(_) if self.is(Break::ObservesPending) => RemoteOutcome::Pending,
            Some(_) => RemoteOutcome::Confirmed,
            None => RemoteOutcome::Failed,
        };
        Ok(Observation {
            outcome,
            marker: found.filter(|_| marker),
        })
    }

    fn lookup(&mut self, op: &OperationName, key: &OperationKey) -> Result<Lookup, ProviderError> {
        let supported = self.cap(op).is_some_and(|c| c.supports_lookup);
        if !supported && !self.is(Break::AnswersLookupWithoutCapability) {
            return Err(ProviderError::Unsupported);
        }
        if self.is(Break::LookupInvents) {
            return Ok(Lookup::Applied(name("invented")));
        }
        match self.applied.get(&(op.clone(), *key)) {
            Some((remote, n)) if *n > 0 && !self.is(Break::LookupForgets) => {
                Ok(Lookup::Applied(remote.clone()))
            }
            _ => Ok(Lookup::NotApplied),
        }
    }

    fn dry_run(&mut self, req: &SendRequest) -> Result<(), ProviderError> {
        let supported = self.cap(&req.operation).is_some_and(|c| c.supports_dry_run);
        if !supported && !self.is(Break::AnswersDryRunWithoutCapability) {
            return Err(ProviderError::Unsupported);
        }
        if self.is(Break::DryRunApplies) {
            self.apply(req);
        }
        Ok(())
    }
}

struct Harness {
    caps: Vec<ProviderCapability>,
    broken: Option<Break>,
}

impl Harness {
    fn new(broken: Option<Break>) -> Self {
        Self {
            caps: mixed(),
            broken,
        }
    }
}

impl ProviderHarness for Harness {
    type Adapter = Double;

    fn adapter(&mut self) -> Double {
        Double {
            caps: self.caps.clone(),
            broken: self.broken,
            applied: BTreeMap::new(),
            fault: None,
            remotes: 0,
        }
    }

    fn fault_next_send(&mut self, adapter: &mut Double, fault: ProviderFault) {
        adapter.fault = Some(fault);
    }

    fn applications(&self, adapter: &Double, op: &OperationName, key: &OperationKey) -> u32 {
        adapter
            .applied
            .get(&(op.clone(), *key))
            .map_or(0, |(_, n)| *n)
    }
}

#[test]
fn a_conforming_double_passes() {
    assert_eq!(run(&mut Harness::new(None)), Ok(()));
}

#[test]
fn each_broken_double_fails_its_rule() {
    let cases = [
        (Break::SendsUnqualified, ProviderRule::UnqualifiedRefused),
        (Break::SendsUnqualified, ProviderRule::UndeclaredRefused),
        (Break::IgnoresHeads, ProviderRule::HeadBaseRequired),
        (Break::ObservesPending, ProviderRule::SendApplies),
        (Break::DropsMarker, ProviderRule::RemoteMarker),
        (
            Break::AnswersLookupWithoutCapability,
            ProviderRule::LookupUnsupported,
        ),
        (Break::LookupForgets, ProviderRule::LookupApplied),
        (Break::LookupInvents, ProviderRule::LookupNotApplied),
        (
            Break::AnswersDryRunWithoutCapability,
            ProviderRule::DryRunUnsupported,
        ),
        (Break::DryRunApplies, ProviderRule::DryRunAppliesNothing),
        (Break::AppliesResend, ProviderRule::DedupOnce),
        (Break::ClaimsDedup, ProviderRule::DedupClaimed),
        (Break::HidesLostAck, ProviderRule::FaultIsUnknown),
        (Break::RateLimitAsTimeout, ProviderRule::RateLimitReported),
        (
            Break::AppliesWhenRateLimited,
            ProviderRule::RateLimitReported,
        ),
        (
            Break::ClaimsNonApplicationUnauthoritatively,
            ProviderRule::RateLimitAuthority,
        ),
        (
            Break::WithholdsAuthoritativeNonApplication,
            ProviderRule::RateLimitAuthority,
        ),
        (
            Break::OmitsAuthoritativeBackOff,
            ProviderRule::RateLimitBackOff,
        ),
        (Break::InventsBackOff, ProviderRule::RateLimitBackOff),
    ];
    for (broken, rule) in cases {
        assert_breaks(&run(&mut Harness::new(Some(broken))), &rule);
    }
}

#[test]
fn declared_capabilities_are_checked() {
    let mut other = Harness::new(None);
    other.caps[0].provider = name("someone-else");
    assert_breaks(&run(&mut other), &ProviderRule::CapabilityProvider);

    let mut twice = Harness::new(None);
    twice.caps.push(twice.caps[1].clone());
    assert_breaks(&run(&mut twice), &ProviderRule::CapabilityUnique);

    let mut contradicts = Harness::new(None);
    contradicts.caps[1].reconciliation_method = ReconciliationMethod::ProviderLookup;
    assert_breaks(&run(&mut contradicts), &ProviderRule::CapabilityConsistent);
}

#[test]
fn a_capability_reconciles_only_by_what_it_supports() {
    let lookup = cap(
        "a",
        [false, true, false, false, false],
        ReconciliationMethod::ProviderLookup,
    );
    assert_eq!(lookup.check(), Ok(()));
    let dedup = cap(
        "a",
        [true, false, false, false, false],
        ReconciliationMethod::ProviderDeduplication,
    );
    assert_eq!(dedup.check(), Ok(()));
    let none = cap("a", [false; 5], ReconciliationMethod::HumanAdjudication);
    assert_eq!(none.check(), Ok(()));

    let no_lookup = cap(
        "a",
        [true, false, false, false, false],
        ReconciliationMethod::ProviderLookup,
    );
    assert_eq!(no_lookup.check(), Err(CapabilityError::LookupUnsupported));
    let no_dedup = cap(
        "a",
        [false, true, false, false, false],
        ReconciliationMethod::ProviderDeduplication,
    );
    assert_eq!(
        no_dedup.check(),
        Err(CapabilityError::IdempotencyUnsupported)
    );
    let adjudicated = cap(
        "a",
        [false, true, false, false, false],
        ReconciliationMethod::HumanAdjudication,
    );
    assert_eq!(
        adjudicated.check(),
        Err(CapabilityError::AdjudicationWithProof)
    );
}

#[test]
fn only_refusals_are_before_send() {
    assert!(SendError::Unqualified.before_send());
    assert!(SendError::MissingHeadBase.before_send());
    for proves_non_application in [false, true] {
        assert!(
            !SendError::RateLimited {
                proves_non_application,
                back_off: Some(BackOff { seconds: 1 }),
            }
            .before_send()
        );
    }
    assert!(!SendError::Transport(TransportFault::Timeout).before_send());
    assert!(!SendError::Transport(TransportFault::Disconnect).before_send());
}

const FORMAL: &str = include_str!("../../../../docs/design/AUTOBOT-FORMAL-SURFACE.md");

/// The fields of the FORMAL §2 `ProviderCapability` record, comments stripped.
fn formal_capability_fields() -> BTreeSet<String> {
    let mut body = String::new();
    for line in FORMAL
        .lines()
        .skip_while(|l| !l.starts_with("ProviderCapability "))
    {
        let line = line.split("\\*").next().unwrap_or_default();
        body.push_str(line);
        body.push(' ');
        if line.contains(']') {
            break;
        }
    }
    let open = body
        .find('[')
        .expect("FORMAL §2 has a ProviderCapability record");
    let close = body.rfind(']').expect("the record closes");
    body[open + 1..close]
        .split(',')
        .map(|f| f.trim().to_owned())
        .collect()
}

/// The names of every field of [`ProviderCapability`]. The destructuring has no `..`, so it
/// stops compiling when a field is added, removed or renamed without this list following.
macro_rules! capability_fields {
    ($($field:ident),* $(,)?) => {{
        #[allow(dead_code)]
        fn exhaustive(capability: ProviderCapability) {
            let ProviderCapability { $($field: _),* } = capability;
        }
        [$(stringify!($field)),*]
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<String>>()
    }};
}

#[test]
fn provider_capability_has_exactly_the_formal_fields() {
    let formal = formal_capability_fields();
    assert!(formal.contains("rate_limit_authoritative"), "{formal:?}");
    let fields = capability_fields!(
        provider,
        operation,
        supports_idempotency,
        supports_lookup,
        supports_remote_marker,
        requires_head_base,
        supports_dry_run,
        rate_limit_authoritative,
        reconciliation_method,
        qualified,
    );
    assert_eq!(fields, formal);
}
