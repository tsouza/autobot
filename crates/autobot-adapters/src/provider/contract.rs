//! The provider contract suite.

use super::{
    Lookup, OperationKey, ProviderAdapter, ProviderCapability, ProviderError, RemoteOutcome,
    SendAck, SendError, SendRequest,
};
use crate::contract::{Checker, SuiteResult};
use crate::text::{Head, OperationName, TargetIdentity};
use autobot_kernel::types::Digest;

/// A fault a [`ProviderHarness`] injects into the next send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderFault {
    /// The request never reaches the provider: nothing is applied and no answer arrives.
    DroppedRequest,
    /// The provider applies the operation and its acknowledgement is lost.
    LostAcknowledgement,
}

/// What the provider suite needs to drive an adapter.
///
/// A send the harness has not faulted, of a qualified operation with every field its
/// capability requires, is applied and confirmed at once: its first observation is
/// [`RemoteOutcome::Confirmed`].
pub trait ProviderHarness {
    /// The adapter under test.
    type Adapter: ProviderAdapter;

    /// A fresh adapter over a fresh provider on which nothing has been applied.
    fn adapter(&mut self) -> Self::Adapter;

    /// Makes the next send through `adapter` end in `fault`.
    fn fault_next_send(&mut self, adapter: &mut Self::Adapter, fault: ProviderFault);

    /// How many times the provider behind `adapter` has applied `key` for `operation`: the
    /// ground truth, not the adapter's claim.
    fn applications(
        &self,
        adapter: &Self::Adapter,
        operation: &OperationName,
        key: &OperationKey,
    ) -> u32;
}

/// The rules of the provider suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderRule {
    /// Every declared capability names the adapter's provider.
    CapabilityProvider,
    /// No operation is declared twice.
    CapabilityUnique,
    /// Every capability's reconciliation method agrees with its semantics
    /// ([`ProviderCapability::check`]).
    CapabilityConsistent,
    /// A send of an undeclared operation is refused as unqualified and applies nothing.
    UndeclaredRefused,
    /// A send of an unqualified operation is refused as unqualified and applies nothing.
    UnqualifiedRefused,
    /// A send lacking heads its capability requires is refused and applies nothing.
    HeadBaseRequired,
    /// A well-formed send is accepted, applied once and observed confirmed.
    SendApplies,
    /// A confirmed observation carries the `operation_key` exactly when the capability
    /// declares a remote marker.
    RemoteMarker,
    /// Without declared lookup, a lookup answers [`ProviderError::Unsupported`].
    LookupUnsupported,
    /// With declared lookup, an operation that was never applied looks up as not applied.
    LookupNotApplied,
    /// With declared lookup, an applied operation looks up as applied under its remote
    /// identity, also after its acknowledgement was lost.
    LookupApplied,
    /// Without a declared dry run, a dry run answers [`ProviderError::Unsupported`].
    DryRunUnsupported,
    /// A declared dry run applies nothing.
    DryRunAppliesNothing,
    /// With declared idempotency, a resend of an applied key is deduplicated under the first
    /// remote identity and applies nothing new.
    DedupOnce,
    /// Without declared idempotency, no send claims deduplication.
    DedupClaimed,
    /// A send whose request was dropped or whose acknowledgement was lost ends in a transport
    /// fault, never in an acknowledgement or a refusal.
    FaultIsUnknown,
}

/// Runs the provider suite against the adapters `harness` makes.
///
/// # Errors
///
/// Every [`ProviderRule`] the adapters broke.
pub fn run<H: ProviderHarness>(harness: &mut H) -> SuiteResult<ProviderRule> {
    let mut c = Checker::new();
    // The literals are not empty, so the else branch is never taken.
    let (Ok(target), Ok(source), Ok(base)) = (
        TargetIdentity::new("contract-target"),
        Head::new("contract-source"),
        Head::new("contract-base"),
    ) else {
        return c.finish();
    };
    let mut reqs = Requests {
        next: 0,
        target,
        source,
        base,
    };
    let adapter = harness.adapter();
    let provider = adapter.provider();
    let capabilities = adapter.capabilities();

    let mut seen: Vec<&OperationName> = Vec::new();
    for cap in &capabilities {
        c.check(
            cap.provider == provider,
            ProviderRule::CapabilityProvider,
            || {
                format!(
                    "{} is declared for provider {}",
                    cap.operation, cap.provider
                )
            },
        );
        c.check(
            !seen.contains(&&cap.operation),
            ProviderRule::CapabilityUnique,
            || format!("{} is declared twice", cap.operation),
        );
        seen.push(&cap.operation);
        if let Err(e) = cap.check() {
            c.fail(
                ProviderRule::CapabilityConsistent,
                format!("{}: {e}", cap.operation),
            );
        }
    }

    undeclared(harness, &mut c, &capabilities, &mut reqs);
    for cap in &capabilities {
        if cap.qualified {
            applied(harness, &mut c, cap, &mut reqs);
            faulted(
                harness,
                &mut c,
                cap,
                ProviderFault::LostAcknowledgement,
                &mut reqs,
            );
            faulted(
                harness,
                &mut c,
                cap,
                ProviderFault::DroppedRequest,
                &mut reqs,
            );
        } else {
            unqualified(harness, &mut c, cap, &mut reqs);
        }
    }
    c.finish()
}

/// The requests of one suite run: distinct operation reqs and fixed field values.
struct Requests {
    next: u64,
    target: TargetIdentity,
    source: Head,
    base: Head,
}

impl Requests {
    /// A key no earlier call returned.
    fn next(&mut self) -> OperationKey {
        self.next += 1;
        let mut bytes = [0xa5; 32];
        bytes[..8].copy_from_slice(&self.next.to_be_bytes());
        OperationKey(Digest::from_bytes(bytes))
    }

    /// A well-formed request for `operation`, with heads when `with_heads`.
    fn request(
        &self,
        operation: &OperationName,
        key: OperationKey,
        with_heads: bool,
    ) -> SendRequest {
        SendRequest {
            operation: operation.clone(),
            operation_key: key,
            attempt_index: 0,
            payload_digest: Digest::from_bytes([0x5a; 32]),
            target_identity: self.target.clone(),
            source_head: with_heads.then(|| self.source.clone()),
            base_head: with_heads.then(|| self.base.clone()),
        }
    }
}

fn undeclared<H: ProviderHarness>(
    harness: &mut H,
    c: &mut Checker<ProviderRule>,
    capabilities: &[ProviderCapability],
    reqs: &mut Requests,
) {
    let mut name = String::from("contract-undeclared");
    while capabilities
        .iter()
        .any(|cap| cap.operation.as_str() == name)
    {
        name.push('-');
    }
    let Ok(operation) = OperationName::new(name) else {
        return;
    };
    let mut adapter = harness.adapter();
    let key = reqs.next();
    let sent = adapter.send(&reqs.request(&operation, key, true));
    c.check(
        sent == Err(SendError::Unqualified),
        ProviderRule::UndeclaredRefused,
        || format!("a send of undeclared {operation} answered {sent:?}"),
    );
    let applied = harness.applications(&adapter, &operation, &key);
    c.check(applied == 0, ProviderRule::UndeclaredRefused, || {
        format!("undeclared {operation} was applied {applied} times")
    });
}

fn unqualified<H: ProviderHarness>(
    harness: &mut H,
    c: &mut Checker<ProviderRule>,
    cap: &ProviderCapability,
    reqs: &mut Requests,
) {
    let mut adapter = harness.adapter();
    let key = reqs.next();
    let sent = adapter.send(&reqs.request(&cap.operation, key, true));
    c.check(
        sent == Err(SendError::Unqualified),
        ProviderRule::UnqualifiedRefused,
        || format!("a send of unqualified {} answered {sent:?}", cap.operation),
    );
    let applied = harness.applications(&adapter, &cap.operation, &key);
    c.check(applied == 0, ProviderRule::UnqualifiedRefused, || {
        format!("unqualified {} was applied {applied} times", cap.operation)
    });
}

/// Checks the lookup answer for an operation that was not applied.
fn check_not_applied(
    c: &mut Checker<ProviderRule>,
    cap: &ProviderCapability,
    found: &Result<Lookup, ProviderError>,
    when: &str,
) {
    if cap.supports_lookup {
        c.check(
            *found == Ok(Lookup::NotApplied),
            ProviderRule::LookupNotApplied,
            || format!("{} {when} looked up as {found:?}", cap.operation),
        );
    } else {
        c.check(
            *found == Err(ProviderError::Unsupported),
            ProviderRule::LookupUnsupported,
            || format!("{} has no lookup and answered {found:?}", cap.operation),
        );
    }
}

/// Checks the lookup answer for an operation that was applied, under `remote` when known.
fn check_applied(
    c: &mut Checker<ProviderRule>,
    cap: &ProviderCapability,
    found: &Result<Lookup, ProviderError>,
    remote: Option<&crate::text::RemoteIdentity>,
    when: &str,
) {
    if cap.supports_lookup {
        let holds = match (found, remote) {
            (Ok(Lookup::Applied(r)), Some(expected)) => r == expected,
            (Ok(Lookup::Applied(_)), None) => true,
            _ => false,
        };
        c.check(holds, ProviderRule::LookupApplied, || {
            format!("{} {when} looked up as {found:?}", cap.operation)
        });
    } else {
        c.check(
            *found == Err(ProviderError::Unsupported),
            ProviderRule::LookupUnsupported,
            || format!("{} has no lookup and answered {found:?}", cap.operation),
        );
    }
}

/// Checks the answer to a resend of an applied key and the application count after it.
fn check_resend<H: ProviderHarness>(
    harness: &H,
    adapter: &mut H::Adapter,
    c: &mut Checker<ProviderRule>,
    cap: &ProviderCapability,
    first: &SendRequest,
    remote: Option<&crate::text::RemoteIdentity>,
) {
    let resend = SendRequest {
        attempt_index: first.attempt_index + 1,
        ..first.clone()
    };
    let again = adapter.send(&resend);
    if cap.supports_idempotency {
        let holds = match (&again, remote) {
            (Ok(SendAck::Deduplicated(r)), Some(expected)) => r == expected,
            (Ok(SendAck::Deduplicated(_)), None) => true,
            _ => false,
        };
        c.check(holds, ProviderRule::DedupOnce, || {
            format!("a resend of applied {} answered {again:?}", cap.operation)
        });
        let applied = harness.applications(adapter, &cap.operation, &first.operation_key);
        c.check(applied == 1, ProviderRule::DedupOnce, || {
            format!(
                "{} was applied {applied} times after a resend",
                cap.operation
            )
        });
    } else {
        c.check(
            !matches!(again, Ok(SendAck::Deduplicated(_))),
            ProviderRule::DedupClaimed,
            || {
                format!(
                    "{} has no idempotency and answered {again:?}",
                    cap.operation
                )
            },
        );
    }
}

/// A fresh provider: refusals and a dry run before the send, then the send, its observation,
/// a lookup and a resend.
fn applied<H: ProviderHarness>(
    harness: &mut H,
    c: &mut Checker<ProviderRule>,
    cap: &ProviderCapability,
    reqs: &mut Requests,
) {
    let mut adapter = harness.adapter();
    let key = reqs.next();
    let op = &cap.operation;
    let req = reqs.request(op, key, cap.requires_head_base);

    if cap.requires_head_base {
        let bare = reqs.request(op, key, false);
        let sent = adapter.send(&bare);
        c.check(
            sent == Err(SendError::MissingHeadBase),
            ProviderRule::HeadBaseRequired,
            || format!("{op} requires heads and a send without them answered {sent:?}"),
        );
        let applied = harness.applications(&adapter, op, &key);
        c.check(applied == 0, ProviderRule::HeadBaseRequired, || {
            format!("{op} without heads was applied {applied} times")
        });
    }

    let found = adapter.lookup(op, &key);
    check_not_applied(c, cap, &found, "before any send");

    let dry = adapter.dry_run(&req);
    if cap.supports_dry_run {
        let applied = harness.applications(&adapter, op, &key);
        c.check(
            dry.is_ok() && applied == 0,
            ProviderRule::DryRunAppliesNothing,
            || format!("a dry run of {op} answered {dry:?} and left {applied} applications"),
        );
    } else {
        c.check(
            dry == Err(ProviderError::Unsupported),
            ProviderRule::DryRunUnsupported,
            || format!("{op} has no dry run and a dry run answered {dry:?}"),
        );
    }

    let sent = adapter.send(&req);
    let applied = harness.applications(&adapter, op, &key);
    let Ok(SendAck::Accepted(remote)) = sent else {
        c.fail(
            ProviderRule::SendApplies,
            format!("a well-formed send of {op} answered {sent:?}"),
        );
        return;
    };
    c.check(applied == 1, ProviderRule::SendApplies, || {
        format!("an accepted send of {op} was applied {applied} times")
    });

    match adapter.observe(op, &remote) {
        Ok(seen) if seen.outcome == RemoteOutcome::Confirmed => {
            let expected = cap.supports_remote_marker.then_some(key);
            c.check(seen.marker == expected, ProviderRule::RemoteMarker, || {
                format!("{op} observed with marker {:?}", seen.marker)
            });
        }
        other => c.fail(
            ProviderRule::SendApplies,
            format!("an accepted send of {op} was observed as {other:?}"),
        ),
    }

    let found = adapter.lookup(op, &key);
    check_applied(c, cap, &found, Some(&remote), "after an accepted send");
    check_resend(harness, &mut adapter, c, cap, &req, Some(&remote));
}

/// A fresh provider whose first send ends in `fault`, then a lookup and, after a lost
/// acknowledgement, a resend.
fn faulted<H: ProviderHarness>(
    harness: &mut H,
    c: &mut Checker<ProviderRule>,
    cap: &ProviderCapability,
    fault: ProviderFault,
    reqs: &mut Requests,
) {
    let mut adapter = harness.adapter();
    let key = reqs.next();
    let op = &cap.operation;
    let req = reqs.request(op, key, cap.requires_head_base);
    harness.fault_next_send(&mut adapter, fault);
    let sent = adapter.send(&req);
    c.check(
        matches!(sent, Err(SendError::Transport(_))),
        ProviderRule::FaultIsUnknown,
        || format!("a send of {op} under {fault:?} answered {sent:?}"),
    );
    let found = adapter.lookup(op, &key);
    match fault {
        ProviderFault::DroppedRequest => {
            check_not_applied(c, cap, &found, "after a dropped request");
        }
        ProviderFault::LostAcknowledgement => {
            check_applied(c, cap, &found, None, "after a lost acknowledgement");
            check_resend(harness, &mut adapter, c, cap, &req, None);
        }
    }
}
