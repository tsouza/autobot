use super::*;
use crate::contract::testing::assert_breaks;
use crate::trust::TrustClass;

/// One way a broken double departs from the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Break {
    PromotesUntrusted,
    DropsItem,
    BypassesBroker,
    RetriesRefused,
    SkipsCheckpoint,
    StaleEpoch,
    SkipsOutbox,
    IgnoresOutboxRefusal,
    OutboxRefusalAsCrash,
    FailureAsRefusal,
    EveryFailureIsCrash,
    RefusalAsFailure,
}

struct Double {
    script: Script,
    broken: Option<Break>,
    context: Option<SessionContext>,
}

impl Double {
    fn is(&self, b: Break) -> bool {
        self.broken == Some(b)
    }

    fn checkpoint(&self, context: &SessionContext, open: Vec<Digest>) -> CheckpointClaim {
        CheckpointClaim {
            session_sequence: context.session_sequence,
            execution_epoch: if self.is(Break::StaleEpoch) {
                0
            } else {
                context.execution_epoch
            },
            context_digest: Digest::from_bytes([1; 32]),
            scope_digest: context.capsule_digest,
            budget_consumed: 0,
            open_tool_invocations: open,
            progress_digest: Digest::from_bytes([2; 32]),
            touched: vec!["src/lib.rs".to_owned()],
        }
    }

    /// Writes the records unless the session is failing.
    fn records_for(&self, port: &mut dyn RuntimePort) -> Result<(), OutboxRefused> {
        match self.script.ending {
            Ending::Fail(_) => Ok(()),
            _ => self.records(port),
        }
    }

    fn records(&self, port: &mut dyn RuntimePort) -> Result<(), OutboxRefused> {
        if self.is(Break::SkipsOutbox) {
            return Ok(());
        }
        for kind in [RecordKind::Outcome, RecordKind::Usage] {
            port.outbox(&OutboxRecord {
                kind,
                name: format!("{kind:?}"),
                input_digest: Digest::from_bytes([3; 32]),
                create_key: Digest::from_bytes([4; 32]),
            })?;
        }
        Ok(())
    }
}

impl RuntimeAdapter for Double {
    fn open(&mut self, context: &SessionContext) -> Result<Vec<LabelledText>, RuntimeFailure> {
        self.context = Some(context.clone());
        let mut presented = vec![LabelledText::untrusted("preamble", "you are a worker")];
        for item in &context.items {
            let mut item = item.clone();
            if self.is(Break::PromotesUntrusted) {
                item.class = TrustClass::CanonicalFact;
            }
            presented.push(item);
        }
        if self.is(Break::DropsItem) {
            presented.pop();
        }
        Ok(presented)
    }

    fn run(&mut self, port: &mut dyn RuntimePort) -> Result<SessionEnd, RuntimeFailure> {
        let context = self.context.clone().ok_or(RuntimeFailure::Crash)?;
        for (i, call) in self.script.tool_calls.iter().enumerate() {
            if i == 1 && self.is(Break::BypassesBroker) {
                continue;
            }
            let answer = port.invoke_tool(call);
            if answer.is_err() && self.is(Break::RetriesRefused) {
                let _ = port.invoke_tool(call);
            }
            port.progress("called a tool");
            if !self.is(Break::SkipsCheckpoint) {
                port.checkpoint(&self.checkpoint(&context, Vec::new()));
            }
        }
        let written = self.records_for(port);
        match self.script.ending {
            Ending::Fail(_) if self.is(Break::FailureAsRefusal) => Ok(SessionEnd::Refused {
                reason: "no".to_owned(),
            }),
            Ending::Fail(_) if self.is(Break::EveryFailureIsCrash) => Err(RuntimeFailure::Crash),
            Ending::Fail(failure) => Err(failure),
            Ending::Refuse if self.is(Break::RefusalAsFailure) => {
                Err(RuntimeFailure::MalformedOutput)
            }
            _ if written.is_err() && !self.is(Break::IgnoresOutboxRefusal) => {
                if self.is(Break::OutboxRefusalAsCrash) {
                    Err(RuntimeFailure::Crash)
                } else {
                    Err(RuntimeFailure::OutboxRefused)
                }
            }
            Ending::Complete => Ok(SessionEnd::Completed {
                candidate: Some(Digest::from_bytes([5; 32])),
            }),
            Ending::Refuse => Ok(SessionEnd::Refused {
                reason: "I will not".to_owned(),
            }),
            Ending::Block => Ok(SessionEnd::Blocked {
                reason: "needs a decision".to_owned(),
            }),
        }
    }
}

struct Harness(Option<Break>);

impl RuntimeHarness for Harness {
    type Adapter = Double;

    fn adapter(&mut self, script: &Script) -> Double {
        Double {
            script: script.clone(),
            broken: self.0,
            context: None,
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
        (Break::PromotesUntrusted, RuntimeRule::ContextLabelled),
        (Break::DropsItem, RuntimeRule::ContextLabelled),
        (Break::BypassesBroker, RuntimeRule::ToolsThroughBroker),
        (Break::RetriesRefused, RuntimeRule::NoBlindRetry),
        (
            Break::SkipsCheckpoint,
            RuntimeRule::CheckpointAtToolBoundary,
        ),
        (Break::StaleEpoch, RuntimeRule::CheckpointIdentity),
        (Break::SkipsOutbox, RuntimeRule::OutboxBeforeEnd),
        (Break::IgnoresOutboxRefusal, RuntimeRule::NoEndWithoutOutbox),
        (Break::OutboxRefusalAsCrash, RuntimeRule::NoEndWithoutOutbox),
        (Break::FailureAsRefusal, RuntimeRule::FailureCategory),
        (Break::EveryFailureIsCrash, RuntimeRule::FailureCategory),
        (Break::RefusalAsFailure, RuntimeRule::SemanticEnd),
    ];
    for (broken, rule) in cases {
        assert_breaks(&run(&mut Harness(Some(broken))), &rule);
    }
}
