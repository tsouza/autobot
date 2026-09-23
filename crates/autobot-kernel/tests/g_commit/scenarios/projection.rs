//! The projection scenario (F-6): audit events delivered out of order and with a conflicting
//! digest.

use super::store::{aggregate, clear, commit, domain_change, port, profile, state_pin, status};
use autobot_kernel::digest::event_digest;
use autobot_kernel::reducer::Guards;
use autobot_kernel::store::{ClearOutcome, CommitOutcome};
use autobot_kernel::types::{CommitSequence, Uid};
use autobot_testkit::harness::Driver;
use autobot_testkit::registry::g_commit::{
    Delivery, Event, Gap, Integrity, ProjectionView, ProjectionsPort,
};

/// The events of `count` domain commits on a new aggregate `name`, in commit order: each is
/// rebuilt from the audit envelope its commit put in the slot. Returns the aggregate's UID.
fn events(driver: &mut dyn Driver, name: &str, count: u64) -> (Uid, Vec<Event>) {
    let target = aggregate(driver, name, false);
    let mut events = Vec::new();
    for revision in 0..count {
        let command = format!("{name}-{revision}");
        let outcome = commit(
            driver,
            &target,
            &command,
            state_pin(revision),
            domain_change(&command, &command),
        );
        assert!(matches!(outcome, CommitOutcome::Committed { .. }));
        let slot = status(driver, &target.0)
            .envelope
            .pending_commit
            .unwrap_or_else(|| panic!("{command} installed no slot"));
        let event_digest = event_digest(&slot.audit_envelope).unwrap_or_else(|e| panic!("{e}"));
        events.push(Event {
            envelope: slot.audit_envelope,
            event_digest,
        });
        assert_eq!(clear(driver, &target, &command), ClearOutcome::Cleared);
    }
    (target.1, events)
}

/// The commit sequences `1..=n`.
fn sequences(n: u64) -> Vec<CommitSequence> {
    (1..=n)
        .map(|s| CommitSequence::new(s).unwrap_or_else(|e| panic!("{e}")))
        .collect()
}

/// Event 3 delivered before 1 and 2 waits in the buffer behind a visible gap; 1 applies, a
/// second 1 changes nothing, and 2 closes the gap and releases 3, so events apply in commit
/// order. A same-identity event with another digest is rejected and marks the projection's
/// integrity, and a gap that is never filled stays visible until declared permanent.
pub(crate) fn out_of_order_and_conflicting(driver: &mut dyn Driver, guards: &Guards) {
    let buffer = profile().values().objects.late_event_buffer.get();
    let mut projection = port::<ProjectionsPort>().projection(buffer, guards);
    let (uid, delivered) = events(driver, "projected", 3);
    let [first, second, third] = &delivered[..] else {
        panic!("three events expected, got {}", delivered.len());
    };

    assert_eq!(projection.deliver(third), Delivery::Buffered);
    let waiting = ProjectionView {
        applied: Vec::new(),
        buffered: 1,
        gap: Gap::Open,
        integrity: Integrity::Ok,
    };
    assert_eq!(projection.view(&uid), waiting);
    assert_eq!(projection.deliver(first), Delivery::Applied);
    assert_eq!(projection.view(&uid).applied, sequences(1));
    assert_eq!(projection.view(&uid).gap, Gap::Open);
    assert_eq!(projection.deliver(first), Delivery::Duplicate);
    assert_eq!(projection.deliver(second), Delivery::Applied);
    let caught_up = ProjectionView {
        applied: sequences(3),
        buffered: 0,
        gap: Gap::None,
        integrity: Integrity::Ok,
    };
    assert_eq!(projection.view(&uid), caught_up);

    let mut forged = second.envelope.clone();
    forged.event_type = "Forged".to_owned();
    let conflicting = Event {
        event_digest: event_digest(&forged).unwrap_or_else(|e| panic!("{e}")),
        envelope: forged,
    };
    assert_ne!(conflicting.event_digest, second.event_digest);
    assert_eq!(projection.deliver(&conflicting), Delivery::Rejected);
    let quarantined = projection.view(&uid);
    assert_eq!(quarantined.applied, sequences(3));
    assert_eq!(quarantined.integrity, Integrity::DigestConflict);

    let (gapped, gapped_events) = events(driver, "gapped", 3);
    assert_eq!(projection.deliver(&gapped_events[0]), Delivery::Applied);
    assert_eq!(projection.deliver(&gapped_events[2]), Delivery::Buffered);
    assert_eq!(projection.view(&gapped).applied, sequences(1));
    assert_eq!(projection.view(&gapped).gap, Gap::Open);
    projection.declare_permanent_gap(&gapped);
    assert_eq!(projection.view(&gapped).gap, Gap::Permanent);
    assert!(!projection.view(&gapped).applied.contains(&sequences(2)[1]));
    assert_eq!(projection.view(&uid), quarantined);
}
