// The `lifecycle!` macro every machine file uses.

/// Defines a state enum and its [`Lifecycle`](super::Lifecycle) table, or gives an enum defined
/// elsewhere its table.
///
/// ```text
/// lifecycle! {
///     /// Enum docs.
///     pub enum Name in "Machine", field "label" {
///         /// State docs.
///         State = "STATE",
///     }
///     edges { [From, ...] -> [To, ...] if Requirement; ... }
///     sibling_sets { From -> "field" := Sibling::State; }
/// }
/// ```
///
/// `, field "label"`, `if Requirement` and `sibling_sets` are optional. With `impl Name in ...`
/// and states without docs it only writes the table of the existing enum `Name`.
macro_rules! lifecycle {
    (
        $(#[$meta:meta])*
        pub enum $name:ident in $machine:literal $(, field $field:literal)? {
            $($(#[$sdoc:meta])* $state:ident = $text:literal,)+
        }
        $($rest:tt)*
    ) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
            serde::Serialize, serde::Deserialize, schemars::JsonSchema,
        )]
        pub enum $name {
            $($(#[$sdoc])* #[serde(rename = $text)] $state,)+
        }

        impl $name {
            /// The state's printed name, which is also its serde form.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$state => $text,)+
                }
            }
        }

        lifecycle!(@table $name in $machine [$($field)?] [$($state = $text,)+] $($rest)*);
    };
    (
        impl $name:ident in $machine:literal $(, field $field:literal)? {
            $($state:ident = $text:literal,)+
        }
        $($rest:tt)*
    ) => {
        lifecycle!(@table $name in $machine [$($field)?] [$($state = $text,)+] $($rest)*);
    };
    (
        @table $name:ident in $machine:literal [$($field:literal)?] [$($state:ident = $text:literal,)+]
        edges { $([$($from:ident),+] -> [$($to:ident),+] $(if $req:ident)?;)* }
        $(sibling_sets { $($sfrom:ident -> $sfield:literal := $sto:expr;)* })?
    ) => {
        impl $crate::lifecycle::Lifecycle for $name {
            const MACHINE: &'static str = $machine;
            const FIELD: Option<&'static str> = lifecycle!(@opt $($field)?);
            const STATES: &'static [Self] = &[$(Self::$state,)+];
            const EDGES: &'static [$crate::lifecycle::Edges<Self>] = &[$(
                $crate::lifecycle::Edges {
                    from: &[$(Self::$from),+],
                    to: &[$(Self::$to),+],
                    requires: lifecycle!(@req $($req)?),
                },
            )*];
            $(const SIBLING_SETS: &'static [$crate::lifecycle::SiblingSet<Self>] = &[$(
                $crate::lifecycle::SiblingSet { from: Self::$sfrom, field: $sfield, to: $sto.as_str() },
            )*];)?

            fn as_str(self) -> &'static str {
                match self {
                    $(Self::$state => $text,)+
                }
            }
        }
    };
    (@opt) => { None };
    (@opt $v:literal) => { Some($v) };
    (@req) => { None };
    (@req $r:ident) => { Some($crate::lifecycle::Requirement::$r) };
}
