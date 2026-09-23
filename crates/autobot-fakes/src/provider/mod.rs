//! Fault-injecting fake provider adapters: an in-memory provider behind the
//! [`ProviderAdapter`] trait of `autobot-adapters`, whose faults a fixture scripts.
//!
//! M0 uses fault-injected fake provider adapters and asserts no real provider capability
//! (M0 preamble). A [`FakeProvider`] applies operations in memory, answers observations,
//! lookups, deduplication and dry runs exactly as its declared [`ProviderCapability`] list
//! says, and suffers the faults of its [`Script`]: timeouts and disconnects before the
//! provider, lost acknowledgements after it applied a send, and rate-limit windows, the
//! provider injections of FORMAL §7. Each [`ProviderFixture`] names one fake provider, its
//! capabilities and its script; the preset fixtures cover providers with and without lookup
//! or idempotency and unqualified capabilities. [`FakeProviderHarness`] runs the provider
//! contract suite of `autobot-adapters` against a fixture.
//!
//! Choices this module makes where the design is open:
//!
//! - Provider names are `fake-forge` and `fake-ci`, and every capability is the fake's own
//!   declaration, not a claim about any real provider.
//! - A rate-limited send is reported as [`SendError::RateLimited`]. A rate-limited observation,
//!   lookup or dry run is reported as [`TransportFault::Timeout`]: [`ProviderError`] has no
//!   throttled answer, and an unknown outcome is the answer that never claims anything
//!   (KERNEL §3.3).
//! - A send without declared idempotency that repeats an applied `operation_key` is applied
//!   again under a new remote identity; a lookup names the first.
//! - An observation of a remote identity the provider never issued, or that belongs to
//!   another operation, is [`RemoteOutcome::Failed`].
//! - A dry run of an undeclared operation is [`ProviderError::Unsupported`].
//! - `COMPENSATED` has no contract yet (#318), so the fake offers no compensation.

mod harness;
mod script;

pub use harness::FakeProviderHarness;
pub use script::{Call, Fault, Script, Trigger};

use autobot_adapters::provider::{
    Lookup, Observation, OperationKey, ProviderAdapter, ProviderCapability, ProviderError,
    ReconciliationMethod, RemoteOutcome, SendAck, SendError, SendRequest, TransportFault,
};
use autobot_adapters::text::{EmptyText, OperationName, ProviderName, RemoteIdentity};
use std::collections::BTreeMap;

/// One fake provider: its name, the capabilities its adapter declares and the faults it
/// suffers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderFixture {
    /// The provider.
    pub provider: ProviderName,
    /// The capability of each operation the adapter offers.
    pub capabilities: Vec<ProviderCapability>,
    /// The faults the provider suffers.
    pub script: Script,
}

/// The semantics a preset capability declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Semantics {
    /// Lookup by `operation_key` and a remote marker, reconciled by lookup.
    Lookup,
    /// Deduplication of a resent `operation_key`, reconciled by deduplication.
    Idempotency,
    /// Lookup, a remote marker and deduplication, reconciled by lookup.
    LookupAndIdempotency,
    /// Neither: an ambiguous operation is resolved only by human adjudication.
    Neither,
}

/// A qualified capability of `operation` at `provider` declaring `semantics`, with no dry
/// run and no required heads.
#[must_use]
pub fn capability(
    provider: &ProviderName,
    operation: OperationName,
    semantics: Semantics,
) -> ProviderCapability {
    let (idempotency, lookup, method) = match semantics {
        Semantics::Lookup => (false, true, ReconciliationMethod::ProviderLookup),
        Semantics::Idempotency => (true, false, ReconciliationMethod::ProviderDeduplication),
        Semantics::LookupAndIdempotency => (true, true, ReconciliationMethod::ProviderLookup),
        Semantics::Neither => (false, false, ReconciliationMethod::HumanAdjudication),
    };
    ProviderCapability {
        provider: provider.clone(),
        operation,
        supports_idempotency: idempotency,
        supports_lookup: lookup,
        supports_remote_marker: lookup,
        requires_head_base: false,
        supports_dry_run: false,
        reconciliation_method: method,
        qualified: true,
    }
}

impl ProviderFixture {
    /// A fixture with no scripted fault.
    #[must_use]
    pub fn new(provider: ProviderName, capabilities: Vec<ProviderCapability>) -> Self {
        Self {
            provider,
            capabilities,
            script: Script::new(),
        }
    }

    /// The fixture with `script` as its faults.
    #[must_use]
    pub fn with_script(self, script: Script) -> Self {
        Self { script, ..self }
    }

    /// `fake-forge`: `comment` with lookup, a remote marker and a dry run; `push` with
    /// idempotency and required heads; `label` with neither lookup nor idempotency; and an
    /// unqualified `merge` that requires heads.
    ///
    /// # Errors
    ///
    /// Never in practice: every name is a non-empty literal.
    pub fn forge() -> Result<Self, EmptyText> {
        let provider = ProviderName::new("fake-forge")?;
        let mut comment = capability(&provider, OperationName::new("comment")?, Semantics::Lookup);
        comment.supports_dry_run = true;
        let mut push = capability(
            &provider,
            OperationName::new("push")?,
            Semantics::Idempotency,
        );
        push.requires_head_base = true;
        let label = capability(&provider, OperationName::new("label")?, Semantics::Neither);
        let mut merge = capability(&provider, OperationName::new("merge")?, Semantics::Neither);
        merge.requires_head_base = true;
        merge.qualified = false;
        Ok(Self::new(provider, vec![comment, push, label, merge]))
    }

    /// `fake-ci`: `run` with lookup and idempotency, and `status` with neither.
    ///
    /// # Errors
    ///
    /// Never in practice: every name is a non-empty literal.
    pub fn ci() -> Result<Self, EmptyText> {
        let provider = ProviderName::new("fake-ci")?;
        let run = capability(
            &provider,
            OperationName::new("run")?,
            Semantics::LookupAndIdempotency,
        );
        let status = capability(&provider, OperationName::new("status")?, Semantics::Neither);
        Ok(Self::new(provider, vec![run, status]))
    }

    /// A fresh [`FakeProvider`] on which nothing has been applied.
    #[must_use]
    pub fn build(&self) -> FakeProvider {
        FakeProvider {
            fixture: self.clone(),
            applied: BTreeMap::new(),
            issued: 0,
        }
    }
}

/// An in-memory provider and its adapter.
#[derive(Debug, Clone)]
pub struct FakeProvider {
    fixture: ProviderFixture,
    /// The remote identities of every application of an `(operation, operation_key)`, first
    /// first.
    applied: BTreeMap<(OperationName, OperationKey), Vec<RemoteIdentity>>,
    /// How many remote identities the provider has issued.
    issued: u64,
}

impl FakeProvider {
    /// How many times the provider applied `key` for `operation`: the ground truth, not the
    /// adapter's claim.
    #[must_use]
    pub fn applications(&self, operation: &OperationName, key: &OperationKey) -> u32 {
        self.applied
            .get(&(operation.clone(), *key))
            .map_or(0, |remotes| {
                u32::try_from(remotes.len()).unwrap_or(u32::MAX)
            })
    }

    /// The faults not yet suffered.
    #[must_use]
    pub fn script(&self) -> &Script {
        &self.fixture.script
    }

    /// The faults not yet suffered, to script more.
    pub fn script_mut(&mut self) -> &mut Script {
        &mut self.fixture.script
    }

    fn capability(&self, operation: &OperationName) -> Option<&ProviderCapability> {
        self.fixture
            .capabilities
            .iter()
            .find(|c| &c.operation == operation)
    }

    /// Applies `request` at the provider: deduplicated when the capability declares
    /// idempotency and the key was applied, else applied under a new remote identity.
    fn apply(&mut self, request: &SendRequest, idempotent: bool) -> Result<SendAck, EmptyText> {
        let entry = (request.operation.clone(), request.operation_key);
        if idempotent && let Some(first) = self.applied.get(&entry).and_then(|r| r.first()) {
            return Ok(SendAck::Deduplicated(first.clone()));
        }
        self.issued += 1;
        let remote = RemoteIdentity::new(format!(
            "{}/{}/{}",
            self.fixture.provider, request.operation, self.issued
        ))?;
        self.applied.entry(entry).or_default().push(remote.clone());
        Ok(SendAck::Accepted(remote))
    }
}

/// The transport fault a faulted call other than a send ends in.
fn unanswered(fault: Fault) -> ProviderError {
    ProviderError::Transport(match fault {
        Fault::Dropped(t) | Fault::LostAcknowledgement(t) => t,
        Fault::RateLimited { .. } => TransportFault::Timeout,
    })
}

impl ProviderAdapter for FakeProvider {
    fn provider(&self) -> ProviderName {
        self.fixture.provider.clone()
    }

    fn capabilities(&self) -> Vec<ProviderCapability> {
        self.fixture.capabilities.clone()
    }

    fn send(&mut self, request: &SendRequest) -> Result<SendAck, SendError> {
        let Some(cap) = self.capability(&request.operation).filter(|c| c.qualified) else {
            return Err(SendError::Unqualified);
        };
        let heads = request.source_head.is_some() && request.base_head.is_some();
        if cap.requires_head_base && !heads {
            return Err(SendError::MissingHeadBase);
        }
        let idempotent = cap.supports_idempotency;
        let fault = self.fixture.script.take(Call::Send, &request.operation);
        match fault {
            Some(Fault::Dropped(t)) => Err(SendError::Transport(t)),
            Some(Fault::RateLimited { .. }) => Err(SendError::RateLimited),
            // The acknowledgement is lost whatever it was. A remote identity is never empty,
            // so `apply` never fails; if it did, it applied nothing and the unknown outcome
            // still proves nothing either way.
            Some(Fault::LostAcknowledgement(t)) => {
                let _ = self.apply(request, idempotent);
                Err(SendError::Transport(t))
            }
            // As above, `apply` never fails; a failure would have applied nothing.
            None => self
                .apply(request, idempotent)
                .map_err(|_| SendError::Transport(TransportFault::Disconnect)),
        }
    }

    fn observe(
        &mut self,
        operation: &OperationName,
        remote: &RemoteIdentity,
    ) -> Result<Observation, ProviderError> {
        if let Some(fault) = self.fixture.script.take(Call::Observe, operation) {
            return Err(unanswered(fault));
        }
        let marker = self
            .capability(operation)
            .is_some_and(|c| c.supports_remote_marker);
        let key = self
            .applied
            .iter()
            .find(|((op, _), remotes)| op == operation && remotes.contains(remote))
            .map(|((_, key), _)| *key);
        Ok(match key {
            Some(key) => Observation {
                outcome: RemoteOutcome::Confirmed,
                marker: marker.then_some(key),
            },
            None => Observation {
                outcome: RemoteOutcome::Failed,
                marker: None,
            },
        })
    }

    fn lookup(
        &mut self,
        operation: &OperationName,
        key: &OperationKey,
    ) -> Result<Lookup, ProviderError> {
        if !self
            .capability(operation)
            .is_some_and(|c| c.supports_lookup)
        {
            return Err(ProviderError::Unsupported);
        }
        if let Some(fault) = self.fixture.script.take(Call::Lookup, operation) {
            return Err(unanswered(fault));
        }
        Ok(
            match self
                .applied
                .get(&(operation.clone(), *key))
                .and_then(|r| r.first())
            {
                Some(remote) => Lookup::Applied(remote.clone()),
                None => Lookup::NotApplied,
            },
        )
    }

    fn dry_run(&mut self, request: &SendRequest) -> Result<(), ProviderError> {
        if !self
            .capability(&request.operation)
            .is_some_and(|c| c.supports_dry_run)
        {
            return Err(ProviderError::Unsupported);
        }
        match self.fixture.script.take(Call::DryRun, &request.operation) {
            Some(fault) => Err(unanswered(fault)),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
