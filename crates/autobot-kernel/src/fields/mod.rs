//! The field classes of `docs/design/AUTOBOT-KERNEL.md` §1 and the classifier that reports
//! which classes a status write touches.
//!
//! Every status field is in one [`FieldClass`]: **domain**, written by the domain lane;
//! **control**, written by the control lane; **reconciliation**, recording only the progress of
//! an intent an earlier commit made canonical; or **structural**, the pending slot body and the
//! control-receipt ring entries, which belong to no lane's digest. A status type declares its
//! partition with `#[derive(FieldClasses)]` and one `#[field(...)]` attribute per field (the
//! grammar is on [`FieldClasses`](macro@FieldClasses)); a field without one does not compile.
//! [`StatusEnvelope`](crate::status::StatusEnvelope) and the records it holds implement
//! [`FieldClasses`] in this module, so a kind's status embeds the envelope with
//! `#[serde(flatten)] #[field(nested)]`.
//!
//! [`classify`] compares a status before and after a write and returns the [`ClassSet`] the
//! write touches; [`ClassSet::write`] names the write it can be, or why it is rejected. A write
//! touching both domain and control fields is [`RejectedWrite::DomainAndControl`], the FORMAL §5
//! negative variant "control commit touching a domain field". [`partition`] lists a status
//! type's declared fields by path and class.
//!
//! The KERNEL §1 classes as the envelope and the kinds declare them:
//!
//! | Kind | Control | Reconciliation |
//! |---|---|---|
//! | `WorkContext` | `hold_state`, `hold_generation`, `manager_authority[*].phase`, `dispatch_authority_generation` | `pending_commit.state`, `control_receipt_ring[*].state`, `dispatch_ledger` |
//! | `TaskRun`, `AgentRun` | `fence_state`, `execution_epoch`, `revocation_generation` | `pending_commit.state`, `control_receipt_ring[*].state` |
//! | every other kind | none | `pending_commit.state`, `control_receipt_ring[*].state` where a ring exists |
//!
//! Every kind also has `control_revision` as a control field and `commit_sequence` as a
//! structural one (below). Everything else is domain.
//!
//! Choices this module makes where the design is open:
//!
//! - KERNEL §1 names `state_revision`, `control_revision` and `commit_sequence` among no class,
//!   so its "everything else is domain" would make them domain, yet a control commit increments
//!   `control_revision` and `commit_sequence` while preserving every domain field. Following
//!   FORMAL F-4, "a control commit changes only control fields and `control_revision`",
//!   `state_revision` is domain, `control_revision` is control, and `commit_sequence`, which
//!   both lanes increment and neither digest can cover, is structural.
//! - A value added to or removed from a container, or an `Option` becoming present or absent,
//!   touches the container's shape class and the class of every part of the value: a new
//!   `manager_authority` entry touches domain and control, as #301 reads KERNEL §1.
//! - `Vec` elements are compared by position; `BTreeMap` values by key.
//! - A write that touches structural parts and neither domain nor control fields is rejected:
//!   the slot body is installed only by a domain commit and ring entries are appended only by a
//!   control commit, and a reconciliation-only CAS writes nothing but reconciliation fields.
//! - Whether a reconciliation-only write appends a `dispatch_ledger` entry, which KERNEL §1
//!   forbids, is outside the classes: the whole ledger is one reconciliation field.
//! - Paths name fields by their Rust names, which KERNEL §1 prints; a flattened field adds no
//!   segment, and `[*]` stands for every element of a `Vec` or ring and every value of a map.

mod envelope;

use std::collections::BTreeMap;
use std::fmt;

pub use autobot_kernel_derive::FieldClasses;

/// The class of a status field (KERNEL §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FieldClass {
    /// Written by the domain lane and covered by the domain digest.
    Domain,
    /// Written by the control lane and covered by the control digest.
    Control,
    /// Records the progress of an intent an earlier commit made canonical; outside both
    /// digests.
    Reconciliation,
    /// The pending slot body and ring entries; in no class's digest.
    Structural,
}

impl FieldClass {
    /// Every class, in declaration order.
    pub const ALL: [Self; 4] = [
        Self::Domain,
        Self::Control,
        Self::Reconciliation,
        Self::Structural,
    ];

    fn bit(self) -> u8 {
        match self {
            Self::Domain => 1,
            Self::Control => 2,
            Self::Reconciliation => 4,
            Self::Structural => 8,
        }
    }
}

impl fmt::Display for FieldClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Domain => "domain",
            Self::Control => "control",
            Self::Reconciliation => "reconciliation",
            Self::Structural => "structural",
        })
    }
}

/// A set of field classes: the classes a write touches.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ClassSet(u8);

impl ClassSet {
    /// The empty set.
    #[must_use]
    pub fn new() -> Self {
        Self(0)
    }

    /// Adds `class`.
    pub fn insert(&mut self, class: FieldClass) {
        self.0 |= class.bit();
    }

    /// Whether the set holds `class`.
    #[must_use]
    pub fn contains(self, class: FieldClass) -> bool {
        self.0 & class.bit() != 0
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The classes in the set, in [`FieldClass::ALL`] order.
    pub fn iter(self) -> impl Iterator<Item = FieldClass> {
        FieldClass::ALL
            .into_iter()
            .filter(move |c| self.contains(*c))
    }

    /// The write a status change touching these classes can be.
    ///
    /// # Errors
    ///
    /// [`RejectedWrite::DomainAndControl`] when the set holds domain and control;
    /// [`RejectedWrite::StructuralOnly`] when it holds structural and neither domain nor
    /// control.
    pub fn write(self) -> Result<Write, RejectedWrite> {
        let domain = self.contains(FieldClass::Domain);
        let control = self.contains(FieldClass::Control);
        match (domain, control) {
            (true, true) => Err(RejectedWrite::DomainAndControl),
            (true, false) => Ok(Write::Domain),
            (false, true) => Ok(Write::Control),
            (false, false) if self.contains(FieldClass::Structural) => {
                Err(RejectedWrite::StructuralOnly)
            }
            (false, false) if self.contains(FieldClass::Reconciliation) => {
                Ok(Write::ReconciliationOnly)
            }
            (false, false) => Ok(Write::Unchanged),
        }
    }
}

impl FromIterator<FieldClass> for ClassSet {
    fn from_iter<I: IntoIterator<Item = FieldClass>>(iter: I) -> Self {
        let mut set = Self::new();
        for class in iter {
            set.insert(class);
        }
        set
    }
}

impl fmt::Debug for ClassSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

/// The write a status change can be, by the classes it touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Write {
    /// Nothing changed.
    Unchanged,
    /// A domain-lane write: domain fields, possibly with structural and reconciliation parts.
    Domain,
    /// A control-lane write: control fields, possibly with structural and reconciliation parts.
    Control,
    /// A reconciliation-only write: reconciliation fields and nothing else.
    ReconciliationOnly,
}

/// Why a status change is no write the kernel admits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RejectedWrite {
    /// The change touches both domain and control fields (KERNEL §1: a CAS that touches both
    /// is rejected).
    DomainAndControl,
    /// The change touches structural parts without a domain or control field, which no lane
    /// and no reconciliation-only CAS writes.
    StructuralOnly,
}

impl fmt::Display for RejectedWrite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::DomainAndControl => "the write touches both domain and control fields",
            Self::StructuralOnly => {
                "the write touches structural parts without a domain or control field"
            }
        })
    }
}

impl std::error::Error for RejectedWrite {}

/// One step of a [`FieldPath`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Segment {
    /// A named field.
    Field(&'static str),
    /// Every element of a list or ring, or every value of a map.
    Each,
}

/// The path of a status field, such as `manager_authority[*].phase`.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldPath(Vec<Segment>);

impl FieldPath {
    /// The empty path, naming the status itself.
    #[must_use]
    pub fn root() -> Self {
        Self::default()
    }

    /// This path followed by the field `name`.
    #[must_use]
    pub fn field(&self, name: &'static str) -> Self {
        self.with(Segment::Field(name))
    }

    /// This path followed by [`Segment::Each`].
    #[must_use]
    pub fn each(&self) -> Self {
        self.with(Segment::Each)
    }

    /// The path's segments.
    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.0
    }

    fn with(&self, segment: Segment) -> Self {
        let mut segments = self.0.clone();
        segments.push(segment);
        Self(segments)
    }
}

impl fmt::Display for FieldPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, segment) in self.0.iter().enumerate() {
            match segment {
                Segment::Field(name) if i == 0 => f.write_str(name)?,
                Segment::Field(name) => write!(f, ".{name}")?,
                Segment::Each => f.write_str("[*]")?,
            }
        }
        Ok(())
    }
}

/// A status part whose fields carry [`FieldClass`]es.
///
/// Derived for a struct with `#[derive(FieldClasses)]`; implemented here for `Option`, `Vec`
/// and `BTreeMap` of such parts, and for the status envelope's records. `shape` is the class
/// a change to the value's shape counts as: presence for an `Option`, membership for a `Vec`
/// or map. A struct has no shape and ignores it.
pub trait FieldClasses {
    /// Adds to `touched` the class of every part that differs between `self` and `other`.
    fn diff(&self, other: &Self, shape: FieldClass, touched: &mut ClassSet);

    /// Adds to `touched` the class of every part `self` holds, as a write that creates or
    /// removes `self` touches them.
    fn classes(&self, shape: FieldClass, touched: &mut ClassSet);

    /// Appends the path and class of every declared field under `prefix`.
    fn partition(prefix: &FieldPath, shape: FieldClass, out: &mut Vec<(FieldPath, FieldClass)>);
}

/// The classes a write from `before` to `after` touches.
#[must_use]
pub fn classify<S: FieldClasses>(before: &S, after: &S) -> ClassSet {
    let mut touched = ClassSet::new();
    before.diff(after, FieldClass::Domain, &mut touched);
    touched
}

/// The declared fields of `S`, by path and class, in declaration order.
#[must_use]
pub fn partition<S: FieldClasses>() -> Vec<(FieldPath, FieldClass)> {
    let mut out = Vec::new();
    S::partition(&FieldPath::root(), FieldClass::Domain, &mut out);
    out
}

impl<T: FieldClasses> FieldClasses for Option<T> {
    fn diff(&self, other: &Self, shape: FieldClass, touched: &mut ClassSet) {
        match (self, other) {
            (Some(a), Some(b)) => a.diff(b, shape, touched),
            (None, None) => {}
            (Some(v), None) | (None, Some(v)) => {
                touched.insert(shape);
                v.classes(shape, touched);
            }
        }
    }

    fn classes(&self, shape: FieldClass, touched: &mut ClassSet) {
        if let Some(v) = self {
            touched.insert(shape);
            v.classes(shape, touched);
        }
    }

    fn partition(prefix: &FieldPath, shape: FieldClass, out: &mut Vec<(FieldPath, FieldClass)>) {
        out.push((prefix.clone(), shape));
        T::partition(prefix, shape, out);
    }
}

impl<T: FieldClasses> FieldClasses for Vec<T> {
    fn diff(&self, other: &Self, shape: FieldClass, touched: &mut ClassSet) {
        diff_slices(self, other, shape, touched);
    }

    fn classes(&self, shape: FieldClass, touched: &mut ClassSet) {
        classes_of_slice(self, shape, touched);
    }

    fn partition(prefix: &FieldPath, shape: FieldClass, out: &mut Vec<(FieldPath, FieldClass)>) {
        partition_of_elements::<T>(prefix, shape, out);
    }
}

impl<K: Ord, V: FieldClasses> FieldClasses for BTreeMap<K, V> {
    fn diff(&self, other: &Self, shape: FieldClass, touched: &mut ClassSet) {
        for (key, a) in self {
            match other.get(key) {
                Some(b) => a.diff(b, shape, touched),
                None => {
                    touched.insert(shape);
                    a.classes(shape, touched);
                }
            }
        }
        for (key, b) in other {
            if !self.contains_key(key) {
                touched.insert(shape);
                b.classes(shape, touched);
            }
        }
    }

    fn classes(&self, shape: FieldClass, touched: &mut ClassSet) {
        for v in self.values() {
            touched.insert(shape);
            v.classes(shape, touched);
        }
    }

    fn partition(prefix: &FieldPath, shape: FieldClass, out: &mut Vec<(FieldPath, FieldClass)>) {
        partition_of_elements::<V>(prefix, shape, out);
    }
}

/// [`FieldClasses::diff`] for a sequence compared by position.
fn diff_slices<T: FieldClasses>(a: &[T], b: &[T], shape: FieldClass, touched: &mut ClassSet) {
    for (x, y) in a.iter().zip(b) {
        x.diff(y, shape, touched);
    }
    let (longer, common) = if a.len() > b.len() {
        (a, b.len())
    } else {
        (b, a.len())
    };
    classes_of_slice(&longer[common..], shape, touched);
}

/// [`FieldClasses::classes`] for a sequence.
fn classes_of_slice<T: FieldClasses>(values: &[T], shape: FieldClass, touched: &mut ClassSet) {
    for v in values {
        touched.insert(shape);
        v.classes(shape, touched);
    }
}

/// [`FieldClasses::partition`] for a container of `T`: its membership, then `T`'s fields under
/// `[*]`.
fn partition_of_elements<T: FieldClasses>(
    prefix: &FieldPath,
    shape: FieldClass,
    out: &mut Vec<(FieldPath, FieldClass)>,
) {
    let each = prefix.each();
    out.push((each.clone(), shape));
    T::partition(&each, shape, out);
}

#[cfg(test)]
mod tests;
