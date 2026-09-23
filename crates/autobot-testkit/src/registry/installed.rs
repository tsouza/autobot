//! The installed registrations: one line per implemented port, each added by the task that
//! implements it, as `registry.register::<Port>(make)`.

use super::Registry;

/// Registers every implemented port in `registry`.
pub(super) fn install(registry: &mut Registry) {
    let _ = registry;
}
