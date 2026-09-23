//! fixture-task: #88
//!
//! The G-COMMIT fixture group of `docs/design/AUTOBOT-M0-AND-GATES.md` §3: the commit, receipt
//! and audit properties F-1 … F-6 of `docs/design/AUTOBOT-FORMAL-SURFACE.md` §4.
//!
//! The store-agnostic scripts are in [`scenarios`]; each fixture test of [`tests`] runs one of
//! them on the in-memory store of `autobot-fakes`, and one more test checks the store faults
//! the scripts position themselves. The bounds a scenario depends on, the ring capacity, the
//! replay window and the late-event buffer, are read from the M0 profile, never restated.
//!
//! | Test | Scenario | Awaiting |
//! |---|---|---|
//! | `f1_idempotency` | duplicate, different-payload, different-principal and expired commands | #83 |
//! | `f2_receipt_durability` | C1 crash after the domain commit, then C2 repair | #84 |
//! | `f3_receipt_barrier` | the barrier holds through repair rounds while the receipt store is down | #84 |
//! | `domain_commit_is_held_by_an_unresolved_slot` | an `OCCUPIED` or `REPAIRING` slot holds the next domain commit (F-3) | #80 |
//! | `f4_lane_separation` | a hold beside an `OCCUPIED` slot; a control transition writing the domain is refused | #81 |
//! | `slot_clearing_changes_only_reconciliation_fields` | the clear writes nothing but the slot's state (F-4) | #82 |
//! | `full_ring_refuses_the_next_control_transition` | the transition beyond the profile's ring capacity (F-4) | #81 |
//! | `hold_beside_a_pending_domain_commit_with_verified_repair` | a hold while the receipt store is down, then digest-verified repair (F-3, F-4) | #84 |
//! | `f5_create_identity` | a create with a lost acknowledgement | #86 |
//! | `no_delete_or_recreate_before_a_terminal_create_receipt` | deletion or recreation before the create receipt is terminal (F-5) | #86 |
//! | `f6_projection_order` | events out of order and with a conflicting digest | #85 |
//!
//! The hold of these scenarios is a synthetic control-lane commit; `RequestHold` beside an
//! `OCCUPIED` `WorkContext` slot belongs to G-DISPATCH.
//!
//! FORMAL §5 variants of the group, for the guard-removal report, with the
//! [`GuardId`](autobot_kernel::reducer::GuardId) of each guard and the surface the scenario
//! passes its [`Guards`](autobot_kernel::reducer::Guards) to:
//!
//! | Guard removed | `GuardId` | Must violate | Violating test | Guards reach |
//! |---|---|---|---|---|
//! | clearing a pending slot on elapsed time | `SlotClearedOnVerification` | F-3 | `f3_receipt_barrier` | `Repair::repair` |
//! | control commit touching a domain field | `ControlFieldsOnly` | F-4 | `f4_lane_separation` | `reducer::step` |
//! | projection applying a later event first | `ProjectionInOrder` | F-6 | `f6_projection_order` | `Projections::projection` |
//!
//! The variant "takeover as one CAS, or `ResumeManager` before `AdvanceManagerEpoch`"
//! (`ManagerTakeoverSequence`) must violate F-4 or F-11; its scenario is G-MANAGER's.
//!
//! Gaps in what the scenarios can reach today:
//!
//! - The command path, slot repair and audit publication, projections and the create command
//!   path have no kernel API yet; their scenarios resolve the ports of
//!   `autobot_testkit::registry::g_commit`, and each implementation task registers its port.
//! - The store has no delete operation, so the delete scenario observes only a refused delete.
//! - The design names no condition type for the ring-full degraded condition, so the ring
//!   scenario checks the refusal and the kept receipts, not a condition.
//! - The in-memory store has no per-kind outage; the receipt-store outage is the scenarios'
//!   own [`Outage`](scenarios::store::Outage) driver over whichever driver they run on.

pub(crate) mod scenarios;
mod tests;
