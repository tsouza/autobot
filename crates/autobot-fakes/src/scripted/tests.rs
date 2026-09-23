use super::*;
use autobot_adapters::runtime::{self, OutboxRefused, RuntimeHarness, Script, ToolRefused};
use autobot_adapters::text::ToolName;
use autobot_adapters::trust::TrustClass;
use autobot_kernel::types::Uid;

/// Replays the runtime suite's scripts as fixtures.
struct Harness;

impl RuntimeHarness for Harness {
    type Adapter = ScriptedAgent;

    fn adapter(&mut self, script: &Script) -> ScriptedAgent {
        let steps = script
            .tool_calls
            .iter()
            .map(|call| Step::Tool {
                call: call.clone(),
                touches: vec!["src/lib.rs".to_owned()],
            })
            .collect();
        let ending = match script.ending {
            runtime::Ending::Complete => Ending::Complete {
                candidate: Some(Digest::from_bytes([9; 32])),
            },
            runtime::Ending::Refuse => Ending::Refuse {
                reason: "I will not".to_owned(),
            },
            runtime::Ending::Block => Ending::Block {
                reason: "needs a decision".to_owned(),
            },
            runtime::Ending::Fail(failure) => Ending::Fail(failure),
        };
        ScriptedAgent::new(Fixture::new(steps, ending))
    }
}

#[test]
fn passes_the_runtime_contract_suite() {
    assert_eq!(runtime::run(&mut Harness), Ok(()));
}

/// One thing the agent did through the port.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Tool(Digest),
    Checkpoint(CheckpointClaim),
    Progress(String),
    Finding(FindingClaim),
    Outbox(OutboxRecord),
}

/// A port that records every call, refusing the tool calls in `refuse` and, when
/// `refuse_outbox`, every outbox write.
#[derive(Debug, Default)]
struct Port {
    events: Vec<Event>,
    refuse: Vec<Digest>,
    refuse_outbox: bool,
}

impl RuntimePort for Port {
    fn invoke_tool(&mut self, call: &ToolCall) -> Result<LabelledText, ToolRefused> {
        self.events.push(Event::Tool(call.request_digest));
        if self.refuse.contains(&call.request_digest) {
            Err(ToolRefused {
                reason: "outside the capsule".to_owned(),
            })
        } else {
            Ok(LabelledText::untrusted("tool output", "ok"))
        }
    }

    fn checkpoint(&mut self, checkpoint: &CheckpointClaim) {
        self.events.push(Event::Checkpoint(checkpoint.clone()));
    }

    fn progress(&mut self, summary: &str) {
        self.events.push(Event::Progress(summary.to_owned()));
    }

    fn finding(&mut self, finding: &FindingClaim) {
        self.events.push(Event::Finding(finding.clone()));
    }

    fn outbox(&mut self, record: &OutboxRecord) -> Result<(), OutboxRefused> {
        if self.refuse_outbox {
            return Err(OutboxRefused {
                reason: "disk full".to_owned(),
            });
        }
        self.events.push(Event::Outbox(record.clone()));
        Ok(())
    }
}

impl Port {
    fn checkpoints(&self) -> Vec<&CheckpointClaim> {
        self.events
            .iter()
            .filter_map(|e| match e {
                Event::Checkpoint(cp) => Some(cp),
                _ => None,
            })
            .collect()
    }

    fn records(&self) -> Vec<&OutboxRecord> {
        self.events
            .iter()
            .filter_map(|e| match e {
                Event::Outbox(r) => Some(r),
                _ => None,
            })
            .collect()
    }
}

fn context() -> SessionContext {
    SessionContext {
        agent_run: Uid::try_from("run-a".to_owned()).expect("a valid uid"),
        session_sequence: 4,
        execution_epoch: 1,
        capsule_digest: Digest::from_bytes([0xca; 32]),
        charter_digest: Digest::from_bytes([0xcb; 32]),
        items: vec![
            LabelledText::new(TrustClass::CanonicalFact, "capsule", "objective"),
            LabelledText::untrusted("issue body", "text"),
        ],
    }
}

fn call(n: u8) -> ToolCall {
    ToolCall {
        tool: ToolName::new("write-file").expect("a tool name"),
        request_digest: Digest::from_bytes([n; 32]),
        input: format!("call {n}"),
    }
}

fn tool(n: u8, touches: &[&str]) -> Step {
    Step::Tool {
        call: call(n),
        touches: touches.iter().map(|&t| t.to_owned()).collect(),
    }
}

fn complete() -> Ending {
    Ending::Complete {
        candidate: Some(Digest::from_bytes([7; 32])),
    }
}

/// Opens and runs `fixture` against `port`.
fn replay(fixture: Fixture, port: &mut Port) -> Result<SessionEnd, RuntimeFailure> {
    let mut agent = ScriptedAgent::new(fixture);
    agent.open(&context())?;
    agent.run(port)
}

#[test]
fn every_failure_is_its_own_category_and_writes_no_outcome() {
    for failure in RuntimeFailure::ALL {
        let mut port = Port::default();
        let result = replay(
            Fixture::new(vec![tool(1, &["a"])], Ending::Fail(failure)),
            &mut port,
        );
        assert_eq!(result, Err(failure), "{failure}");
        assert!(port.records().is_empty(), "{failure}: {:?}", port.events);
        let last_is_checkpoint = matches!(port.events.last(), Some(Event::Checkpoint(_)));
        let checkpoints = port.checkpoints().len();
        if failure == RuntimeFailure::Crash {
            assert_eq!(checkpoints, 1, "a crash adds no checkpoint");
        } else {
            assert!(last_is_checkpoint, "{failure}: {:?}", port.events);
            assert_eq!(checkpoints, 2, "{failure}: a continuation checkpoint");
        }
    }
}

#[test]
fn a_refusal_is_a_semantic_outcome_with_its_records() {
    let mut port = Port::default();
    let result = replay(
        Fixture::new(
            vec![tool(1, &[])],
            Ending::Refuse {
                reason: "not doing that".to_owned(),
            },
        ),
        &mut port,
    );
    assert_eq!(
        result,
        Ok(SessionEnd::Refused {
            reason: "not doing that".to_owned()
        })
    );
    let kinds: Vec<RecordKind> = port.records().iter().map(|r| r.kind).collect();
    assert_eq!(kinds, [RecordKind::Outcome, RecordKind::Usage]);
}

#[test]
fn a_failure_to_open_is_reported_as_its_category() {
    for failure in [RuntimeFailure::ProviderOutage, RuntimeFailure::RateLimited] {
        let mut agent = ScriptedAgent::new(Fixture {
            open_failure: Some(failure),
            ..Fixture::new(Vec::new(), complete())
        });
        assert_eq!(agent.open(&context()), Err(failure));
        let mut port = Port::default();
        assert_eq!(agent.run(&mut port), Err(RuntimeFailure::Crash));
        assert!(port.events.is_empty());
    }
}

#[test]
fn a_disconnect_leaves_its_call_open_and_stops_the_replay() {
    let mut port = Port::default();
    let result = replay(
        Fixture::new(
            vec![tool(1, &[]), Step::Disconnect(call(2)), tool(3, &[])],
            complete(),
        ),
        &mut port,
    );
    assert_eq!(result, Err(RuntimeFailure::StreamDisconnect));
    let calls: Vec<&Event> = port
        .events
        .iter()
        .filter(|e| matches!(e, Event::Tool(_)))
        .collect();
    assert_eq!(
        calls,
        [
            &Event::Tool(Digest::from_bytes([1; 32])),
            &Event::Tool(Digest::from_bytes([2; 32]))
        ]
    );
    let last = port.checkpoints().last().copied().cloned();
    assert_eq!(
        last.map(|cp| cp.open_tool_invocations),
        Some(vec![Digest::from_bytes([2; 32])])
    );
    assert!(port.records().is_empty());
}

#[test]
fn checkpoints_list_what_answered_calls_and_subprocesses_touched() {
    let mut port = Port {
        refuse: vec![Digest::from_bytes([2; 32])],
        ..Port::default()
    };
    let steps = vec![
        tool(1, &["src/a.rs", "src/b.rs"]),
        Step::SubprocessWrite {
            path: "target/out".to_owned(),
        },
        Step::OutOfScope {
            call: call(2),
            path: "/etc/passwd".to_owned(),
        },
        Step::OutOfScope {
            call: call(3),
            path: "../other".to_owned(),
        },
        Step::SubprocessWrite {
            path: "build.log".to_owned(),
        },
        Step::Checkpoint,
    ];
    let result = replay(Fixture::new(steps, complete()), &mut port);
    assert!(result.is_ok());
    let touched: Vec<Vec<String>> = port
        .checkpoints()
        .iter()
        .map(|cp| cp.touched.clone())
        .collect();
    let expected: Vec<Vec<&str>> = vec![
        vec!["src/a.rs", "src/b.rs"],
        vec!["target/out"],
        vec!["../other"],
        vec!["build.log"],
        vec![],
    ];
    assert_eq!(touched, expected);
}

#[test]
fn checkpoints_carry_identity_budget_and_progress() {
    let mut port = Port::default();
    let steps = vec![
        tool(1, &[]),
        Step::Progress("halfway".to_owned()),
        tool(2, &[]),
    ];
    assert!(replay(Fixture::new(steps, complete()), &mut port).is_ok());
    let ctx = context();
    let cps = port.checkpoints();
    assert_eq!(cps.len(), 3);
    for cp in &cps {
        assert_eq!(cp.session_sequence, ctx.session_sequence);
        assert_eq!(cp.execution_epoch, ctx.execution_epoch);
        assert_eq!(cp.scope_digest, ctx.capsule_digest);
        assert_eq!(cp.context_digest, cps[0].context_digest);
    }
    let budgets: Vec<u64> = cps.iter().map(|cp| cp.budget_consumed).collect();
    assert_eq!(budgets, [1, 2, 2]);
    assert_ne!(cps[0].progress_digest, cps[1].progress_digest);
    assert_eq!(cps[1].progress_digest, cps[2].progress_digest);
}

#[test]
fn progress_and_findings_are_reported_in_order() {
    let finding = FindingClaim {
        observed: "the lockfile is stale".to_owned(),
        affected_scope: vec!["Cargo.lock".to_owned()],
    };
    let mut port = Port::default();
    let steps = vec![
        Step::Progress("started".to_owned()),
        Step::Finding(finding.clone()),
        Step::Progress("done".to_owned()),
    ];
    assert!(replay(Fixture::new(steps, complete()), &mut port).is_ok());
    let reported: Vec<&Event> = port
        .events
        .iter()
        .filter(|e| matches!(e, Event::Progress(_) | Event::Finding(_)))
        .collect();
    assert_eq!(
        reported,
        [
            &Event::Progress("started".to_owned()),
            &Event::Finding(finding),
            &Event::Progress("done".to_owned()),
        ]
    );
}

#[test]
fn records_follow_the_final_checkpoint_and_are_deterministic() {
    let fixture = Fixture::new(vec![tool(1, &[])], complete());
    let mut first = Port::default();
    let mut second = Port::default();
    assert_eq!(
        replay(fixture.clone(), &mut first),
        Ok(SessionEnd::Completed {
            candidate: Some(Digest::from_bytes([7; 32]))
        })
    );
    assert!(replay(fixture, &mut second).is_ok());
    assert_eq!(first.events, second.events);
    let n = first.events.len();
    assert!(matches!(first.events[n - 3], Event::Checkpoint(_)));
    let records = first.records();
    assert_eq!(records[0].name, "outcome-run-a");
    assert_eq!(records[1].name, "usage-run-a");
    assert_eq!(
        records[0].create_key,
        sha256(format!("outcome-run-a\n{}", records[0].input_digest).as_bytes())
    );

    let mut other = Port::default();
    assert!(
        replay(
            Fixture::new(
                vec![tool(1, &[])],
                Ending::Complete {
                    candidate: Some(Digest::from_bytes([8; 32]))
                }
            ),
            &mut other,
        )
        .is_ok()
    );
    let (a, b) = (first.records(), other.records());
    assert_ne!(a[0].input_digest, b[0].input_digest);
    assert_eq!(a[1].input_digest, b[1].input_digest);
}

#[test]
fn a_refused_outbox_reports_no_end() {
    let mut port = Port {
        refuse_outbox: true,
        ..Port::default()
    };
    let result = replay(Fixture::new(vec![tool(1, &[])], complete()), &mut port);
    assert_eq!(result, Err(RuntimeFailure::Crash));
}

#[test]
fn the_context_is_presented_after_an_untrusted_preamble() {
    let mut agent = ScriptedAgent::new(Fixture::new(Vec::new(), complete()));
    let presented = agent.open(&context()).expect("opens");
    assert_eq!(presented[0].class, TrustClass::UntrustedContent);
    assert_eq!(presented[0].label, PREAMBLE_LABEL);
    assert_eq!(presented[1..], context().items[..]);
}

#[test]
fn a_session_runs_once() {
    let mut agent = ScriptedAgent::new(Fixture::new(Vec::new(), complete()));
    let mut port = Port::default();
    assert_eq!(agent.run(&mut port), Err(RuntimeFailure::Crash));
    assert!(agent.open(&context()).is_ok());
    assert!(agent.run(&mut port).is_ok());
    assert_eq!(agent.run(&mut port), Err(RuntimeFailure::Crash));
}
