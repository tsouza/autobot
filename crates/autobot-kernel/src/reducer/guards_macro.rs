// The `guards!` macro, which `guards.rs` includes; the `Guards::CAPACITY` doctests include it
// too, so that they build a guard list of their own through the same macro.

/// Defines `GuardId` from one line per FORMAL §5 row: variant, row text, invariants.
///
/// The expansion asserts at compile time that the list fits the `Guards` bitset: a list longer
/// than `Guards::CAPACITY` does not compile.
macro_rules! guards {
    ($($(#[$doc:meta])* $name:ident => $removed:literal, $violates:literal;)*) => {
        /// One guard of `docs/design/AUTOBOT-FORMAL-SURFACE.md` §5, one variant per row of its
        /// negative-variant table, in table order.
        ///
        /// Serde and the JSON schema use the variant name in kebab case.
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
            JsonSchema,
        )]
        #[serde(rename_all = "kebab-case")]
        pub enum GuardId {
            $($(#[$doc])* $name,)*
        }

        impl GuardId {
            /// Every guard, in FORMAL §5 table order.
            pub const ALL: &'static [GuardId] = &[$(GuardId::$name,)*];

            /// The row's "Guard removed" cell, verbatim: what the negative variant does
            /// without this guard.
            #[must_use]
            pub fn removed(self) -> &'static str {
                match self {
                    $(GuardId::$name => $removed,)*
                }
            }

            /// The row's "Must violate" cell, verbatim: the fixtures the negative variant
            /// must fail.
            #[must_use]
            pub fn must_violate(self) -> &'static str {
                match self {
                    $(GuardId::$name => $violates,)*
                }
            }
        }

        const _: () = assert!(
            GuardId::ALL.len() <= Guards::CAPACITY,
            "more guards than the Guards bitset has bits"
        );
    };
}
