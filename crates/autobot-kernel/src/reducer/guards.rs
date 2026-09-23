//! The FORMAL §5 guards and the set of them a reducer decides under.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

include!("guards_macro.rs");

guards! {
    /// Acceptance reads the registers and records the acceptance in one CAS on one object.
    AcceptanceInOneCas =>
        "acceptance reads registers then records in another object", "F-7, F-8, F-9, F-10";
    /// `QuiescePlan` and `ResumePlanRevision` each advance `plan_generation`.
    PlanGenerationAdvance =>
        "`QuiescePlan` and `ResumePlanRevision` without `plan_generation + 1` (one guard: generation advance on quiesce and resume)",
        "F-9";
    /// A Manager takeover is a sequence of CASes, and `ResumeManager` follows
    /// `AdvanceManagerEpoch`.
    ManagerTakeoverSequence =>
        "takeover as one CAS, or `ResumeManager` before `AdvanceManagerEpoch`", "F-4 or F-11";
    /// A send is preceded by its `send_attempt` marker.
    SendAttemptMarker => "send without a `send_attempt` marker", "F-18";
    /// `RecordSendAttempt` writes 2a before 2b.
    SendAttemptOrder => "`RecordSendAttempt` 2b before 2a", "F-19";
    /// A control commit writes control fields only.
    ControlFieldsOnly => "control commit touching a domain field", "F-4";
    /// A pending slot is cleared only after its receipt and audit event are verified.
    SlotClearedOnVerification => "clearing a pending slot on elapsed time", "F-3";
    /// A projection applies events in commit-sequence order.
    ProjectionInOrder => "projection applying a later event first", "F-6";
    /// A continuation keeps its execution identity.
    ContinuationIdentity => "continuation with a fresh identity", "F-26";
    /// A workspace retires on a completion marker only after restore verification.
    RetireAfterRestoreVerification =>
        "retire a workspace on a completion marker without restore verification", "F-16";
    /// A restored installation dispatches only after a witness receipt.
    RestoreWitnessReceipt =>
        "dispatch on a restored installation without a witness receipt", "F-15";
    /// A scope check compares the canonical path, not the requested string.
    CanonicalScopeCheck => "lexical glob check on the requested string", "F-23";
    /// A telemetry gap is created at `record_deadline`.
    GapAtRecordDeadline => "no gap created at `record_deadline`", "F-32";
    /// A reviewer's identity differs from the worker session's.
    ReviewerIndependence => "reviewer identity equal to worker session", "F-28";
    /// A `correlated` review never satisfies the required review of `COMPATIBILITY_RISK` work.
    CorrelatedReviewNotCompatibility =>
        "a `correlated` review accepted as the required review of `COMPATIBILITY_RISK` work", "F-28";
    /// A required security review comes only from a configuration in `securityReviewers`.
    SecurityReviewerPool =>
        "a required `SECURITY_OR_DATA_INTEGRITY` review accepted from a configuration outside `securityReviewers`", "F-28";
    /// Admission compares the routing pin's tier with the floor.
    AdmissionFloor => "admission without the floor comparison", "F-37";
    /// Plan acceptance pins a charter revision.
    PinnedCharterRevision => "plan acceptance without a pinned charter revision", "F-38";
    /// A project entry never weakens an inherited law.
    NoInheritedLawWeakened => "a project entry that weakens an inherited law", "F-39";
    /// A continuation after a tightened law gets a fresh capsule.
    FreshCapsuleOnTightenedLaw =>
        "continuation keeping its capsule after a law was tightened", "F-40";
    /// Only a human principal accepts a charter revision.
    HumanCharterAcceptance => "charter revision accepted by an agent principal", "F-41";
    /// A `judged` "complies" never satisfies the `review` backstop.
    JudgedNotReviewBackstop => "a `judged` \"complies\" satisfying the `review` backstop", "F-42";
    /// No accept is admitted from the intake-submitter identity.
    IntakeSubmitterCannotAccept => "an accept admitted from the intake-submitter identity", "F-43";
    /// An `Intake` reaches `PROPOSED` only with every repository forge-verified.
    ForgeVerifiedBeforeProposed =>
        "an `Intake` reaching `PROPOSED` with a repository lacking a forge-adapter answer", "F-44";
}

/// The integer type of the [`Guards`] bitset.
type Bits = u32;

impl GuardId {
    /// The guard's bit in [`Guards`]: one at its position in [`GuardId::ALL`].
    fn bit(self) -> Bits {
        1 << (self as u32)
    }
}

/// The guards a reducer decides under.
///
/// Built without `cfg(test)` and without the crate's `testing` feature, [`Guards::all`], with
/// every FORMAL §5 guard enabled, is the only value that can be made. [`Guards::without`],
/// which disables one guard so that a negative variant can show its fixture failing, exists
/// only under `cfg(test)` or the `testing` feature:
///
#[cfg_attr(not(feature = "testing"), doc = "```compile_fail")]
#[cfg_attr(feature = "testing", doc = "```")]
/// use autobot_kernel::reducer::{GuardId, Guards};
///
/// let guards = Guards::all().without(GuardId::ControlFieldsOnly);
/// assert!(!guards.is_enabled(GuardId::ControlFieldsOnly));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Guards {
    /// One bit per disabled guard, at the guard's position in [`GuardId::ALL`].
    disabled: Bits,
}

impl Guards {
    /// The most guards the bitset holds: one bit each.
    ///
    /// The guard list is checked against it at compile time. A list of `CAPACITY` guards
    /// builds:
    ///
    /// ```
    /// # use autobot_kernel::reducer::Guards;
    /// # use schemars::JsonSchema;
    /// # use serde::{Deserialize, Serialize};
    /// include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/reducer/guards_macro.rs"));
    /// guards! {
    /// G0 => "", ""; G1 => "", ""; G2 => "", ""; G3 => "", ""; G4 => "", ""; G5 => "", "";
    /// G6 => "", ""; G7 => "", ""; G8 => "", ""; G9 => "", ""; G10 => "", ""; G11 => "", "";
    /// G12 => "", ""; G13 => "", ""; G14 => "", ""; G15 => "", ""; G16 => "", ""; G17 => "", "";
    /// G18 => "", ""; G19 => "", ""; G20 => "", ""; G21 => "", ""; G22 => "", ""; G23 => "", "";
    /// G24 => "", ""; G25 => "", ""; G26 => "", ""; G27 => "", ""; G28 => "", ""; G29 => "", "";
    /// G30 => "", ""; G31 => "", "";
    /// }
    ///
    /// fn main() {
    ///     assert_eq!(GuardId::ALL.len(), Guards::CAPACITY);
    /// }
    /// ```
    ///
    /// and one more guard does not:
    ///
    /// ```compile_fail,E0080
    /// # use autobot_kernel::reducer::Guards;
    /// # use schemars::JsonSchema;
    /// # use serde::{Deserialize, Serialize};
    /// include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/reducer/guards_macro.rs"));
    /// guards! {
    /// G0 => "", ""; G1 => "", ""; G2 => "", ""; G3 => "", ""; G4 => "", ""; G5 => "", "";
    /// G6 => "", ""; G7 => "", ""; G8 => "", ""; G9 => "", ""; G10 => "", ""; G11 => "", "";
    /// G12 => "", ""; G13 => "", ""; G14 => "", ""; G15 => "", ""; G16 => "", ""; G17 => "", "";
    /// G18 => "", ""; G19 => "", ""; G20 => "", ""; G21 => "", ""; G22 => "", ""; G23 => "", "";
    /// G24 => "", ""; G25 => "", ""; G26 => "", ""; G27 => "", ""; G28 => "", ""; G29 => "", "";
    /// G30 => "", ""; G31 => "", ""; G32 => "", "";
    /// }
    /// # fn main() {}
    /// ```
    pub const CAPACITY: usize = Bits::BITS as usize;

    /// Every guard enabled.
    #[must_use]
    pub const fn all() -> Self {
        Self { disabled: 0 }
    }

    /// These guards with `guard` disabled.
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn without(self, guard: GuardId) -> Self {
        Self {
            disabled: self.disabled | guard.bit(),
        }
    }

    /// Whether `guard` is enabled.
    #[must_use]
    pub fn is_enabled(self, guard: GuardId) -> bool {
        self.disabled & guard.bit() == 0
    }

    /// The disabled guards, in FORMAL §5 table order; empty for [`Guards::all`].
    #[must_use]
    pub fn disabled(self) -> Vec<GuardId> {
        GuardId::ALL
            .iter()
            .copied()
            .filter(|&g| !self.is_enabled(g))
            .collect()
    }
}

impl Default for Guards {
    /// [`Guards::all`].
    fn default() -> Self {
        Self::all()
    }
}
