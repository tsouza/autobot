//! The registry: the kernel pieces a fixture scenario resolves at run time, so that the
//! scenario compiles and runs, failing, before the piece is implemented.
//!
//! - A [`Port`] names one piece a scenario needs, such as a reducer, a command path, a repair
//!   loop or a projection: what resolving it gives ([`Port::Object`], usually a trait object),
//!   its name and the task whose implementation fills it.
//! - A [`Registry`] maps each port to the function that makes its implementation.
//!   [`Registry::resolve`] makes one, or fails with [`Unresolved`]: an unregistered port
//!   reads `no <name> implementation is registered (awaiting #<task>)`.
//! - [`resolve`] resolves against the installed registry, the one [`installed`] builds once
//!   per process from the registrations in `installed.rs`. A scenario calls it and fails with
//!   that message until the implementation's registration is installed; installing it changes
//!   nothing in the scenario.
//!
//! Choices this module makes where the design is open:
//!
//! - Registration is a line in `installed.rs`, not a declaration in the implementing crate:
//!   the kernel sits below this crate in the dependency graph (`autobot-testkit` depends on
//!   `autobot-fakes`, which depends on `autobot-kernel`), so kernel code cannot name a port of
//!   this crate, and the standard library has no link-time collection a crate could register
//!   into instead. Each registration names the implementing crate's public items, as
//!   `autobot_controllers::all_controllers` does for controllers.
//! - A port is declared in this crate, where both a scenario and `installed.rs` can name it: a
//!   port declared in a fixture's own test target could be registered only from that target.
//! - Registering one port twice is not refused at registration: resolving that port fails
//!   with [`Unresolved::Duplicate`], so the conflict is reported by the scenario that needs
//!   the port, and a self-test checks that the installed registry has none.

mod installed;

use std::any::{Any, TypeId};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;

/// One piece a scenario resolves.
pub trait Port: 'static {
    /// What resolving the port gives, usually `dyn Trait` for the piece's trait.
    type Object: ?Sized + 'static;
    /// The piece's name, as the failure of an unresolved port shows it.
    const NAME: &'static str;
    /// The issue number of the task whose implementation fills the port.
    const AWAITING: u32;
}

/// The function that makes the implementation of port `P`.
pub type Make<P> = fn() -> Box<<P as Port>::Object>;

/// A port the registry cannot resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unresolved {
    /// No implementation is registered for the port.
    Unregistered {
        /// The port's name.
        port: &'static str,
        /// The task whose implementation fills it.
        awaiting: u32,
    },
    /// More than one implementation is registered for the port.
    Duplicate {
        /// The port's name.
        port: &'static str,
        /// How many implementations are registered.
        registrations: usize,
    },
}

impl fmt::Display for Unresolved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unregistered { port, awaiting } => write!(
                f,
                "no {port} implementation is registered (awaiting #{awaiting})"
            ),
            Self::Duplicate {
                port,
                registrations,
            } => write!(
                f,
                "{registrations} {port} implementations are registered; exactly one may be"
            ),
        }
    }
}

impl std::error::Error for Unresolved {}

/// One port's registrations: the first maker and how many were registered.
struct Entry {
    name: &'static str,
    make: Box<dyn Any + Send + Sync>,
    registrations: usize,
}

impl fmt::Debug for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entry")
            .field("name", &self.name)
            .field("registrations", &self.registrations)
            .finish_non_exhaustive()
    }
}

/// The implementations registered for each port.
#[derive(Debug, Default)]
pub struct Registry {
    entries: BTreeMap<TypeId, Entry>,
}

impl Registry {
    /// A registry with nothing registered.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `make` as the implementation of `P`.
    pub fn register<P: Port>(&mut self, make: Make<P>) {
        self.entries
            .entry(TypeId::of::<P>())
            .and_modify(|e| e.registrations += 1)
            .or_insert_with(|| Entry {
                name: P::NAME,
                make: Box::new(make),
                registrations: 1,
            });
    }

    /// Makes the implementation of `P`.
    ///
    /// # Errors
    ///
    /// [`Unresolved::Unregistered`] if nothing is registered for `P`, and
    /// [`Unresolved::Duplicate`] if more than one implementation is.
    pub fn resolve<P: Port>(&self) -> Result<Box<P::Object>, Unresolved> {
        let unregistered = Unresolved::Unregistered {
            port: P::NAME,
            awaiting: P::AWAITING,
        };
        let entry = self.entries.get(&TypeId::of::<P>()).ok_or(unregistered)?;
        if entry.registrations > 1 {
            return Err(Unresolved::Duplicate {
                port: P::NAME,
                registrations: entry.registrations,
            });
        }
        // The entry under `P`'s type id holds a `Make<P>`: `register` is the only writer.
        let make = entry.make.downcast_ref::<Make<P>>().ok_or(unregistered)?;
        Ok(make())
    }

    /// The names of the ports registered more than once.
    #[must_use]
    pub fn duplicates(&self) -> Vec<&'static str> {
        self.entries
            .values()
            .filter(|e| e.registrations > 1)
            .map(|e| e.name)
            .collect()
    }
}

/// The installed registry: every registration in `installed.rs`, built once per process.
pub fn installed() -> &'static Registry {
    static INSTALLED: OnceLock<Registry> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let mut registry = Registry::new();
        installed::install(&mut registry);
        registry
    })
}

/// Makes the implementation of `P` from the [`installed`] registry.
///
/// # Errors
///
/// As [`Registry::resolve`].
pub fn resolve<P: Port>() -> Result<Box<P::Object>, Unresolved> {
    installed().resolve::<P>()
}

#[cfg(test)]
mod tests;
