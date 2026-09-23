//! The field classes of the status envelope and the records it holds.

use super::{
    ClassSet, FieldClass, FieldClasses, FieldPath, classes_of_slice, diff_slices,
    partition_of_elements,
};
use crate::status::{ControlReceipt, ControlReceiptRing, PendingCommit, StatusEnvelope};

use FieldClass::{Control, Domain, Reconciliation, Structural};

/// Implements [`FieldClasses`] for a record from one list of its fields: leaves with their
/// class, then nested parts with their shape class and type. The record is destructured
/// without `..`, so a field added to it does not compile until it is listed here.
macro_rules! record_classes {
    (
        $record:ty {
            $($leaf:ident: $class:ident,)*
        }
        nested {
            $($part:ident: $shape:ident as $part_ty:ty,)*
        }
    ) => {
        impl FieldClasses for $record {
            fn diff(&self, other: &Self, _shape: FieldClass, touched: &mut ClassSet) {
                let Self { $($leaf,)* $($part,)* } = self;
                $(
                    if *$leaf != other.$leaf {
                        touched.insert($class);
                    }
                )*
                $( $part.diff(&other.$part, $shape, touched); )*
            }

            fn classes(&self, _shape: FieldClass, touched: &mut ClassSet) {
                $( touched.insert($class); )*
                $( self.$part.classes($shape, touched); )*
            }

            fn partition(
                prefix: &FieldPath,
                _shape: FieldClass,
                out: &mut Vec<(FieldPath, FieldClass)>,
            ) {
                $( out.push((prefix.field(stringify!($leaf)), $class)); )*
                $( <$part_ty>::partition(&prefix.field(stringify!($part)), $shape, out); )*
            }
        }
    };
}

record_classes! {
    StatusEnvelope {
        observed_generation: Domain,
        conditions: Domain,
        state_revision: Domain,
        control_revision: Control,
        commit_sequence: Structural,
        last_receipt_ref: Domain,
    }
    nested {
        pending_commit: Structural as Option<PendingCommit>,
        control_receipt_ring: Structural as Option<ControlReceiptRing>,
    }
}

record_classes! {
    PendingCommit {
        command_uid: Structural,
        receipt_uid: Structural,
        commit_sequence: Structural,
        before_digest: Structural,
        after_digest: Structural,
        expected_revision: Structural,
        proposed_revision: Structural,
        control_revision_at_commit: Structural,
        audit_digest: Structural,
        effect_intents: Structural,
        state: Reconciliation,
    }
    nested {}
}

record_classes! {
    ControlReceipt {
        control_uid: Structural,
        control_revision: Structural,
        commit_sequence: Structural,
        before_control_digest: Structural,
        after_control_digest: Structural,
        audit_envelope: Structural,
        principal: Structural,
        state: Reconciliation,
    }
    nested {}
}

impl FieldClasses for ControlReceiptRing {
    fn diff(&self, other: &Self, shape: FieldClass, touched: &mut ClassSet) {
        diff_slices(self.entries(), other.entries(), shape, touched);
    }

    fn classes(&self, shape: FieldClass, touched: &mut ClassSet) {
        classes_of_slice(self.entries(), shape, touched);
    }

    fn partition(prefix: &FieldPath, shape: FieldClass, out: &mut Vec<(FieldPath, FieldClass)>) {
        partition_of_elements::<ControlReceipt>(prefix, shape, out);
    }
}
