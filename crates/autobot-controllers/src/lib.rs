//! AutoBot controllers: one module per owning controller of `docs/design/AUTOBOT-M0-AND-GATES.md` §1,
//! plus the Kubernetes store driver, admission, broker and custody.
//!
//! [`all_controllers`] is the controller registry: each owning controller adds its own entry,
//! and the operator host iterates it and selects a controller by [`Controller::name`].
#![warn(missing_docs)]

pub mod queue;

/// An owning controller the operator host can run.
pub trait Controller: Send + Sync {
    /// The controller's name: the owner of its kinds in `docs/design/AUTOBOT-M0-AND-GATES.md`
    /// §1, unique within [`all_controllers`].
    fn name(&self) -> &'static str;
}

/// Every owning controller, one entry each.
#[must_use]
pub fn all_controllers() -> Vec<Box<dyn Controller>> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::all_controllers;
    use std::collections::BTreeSet;

    #[test]
    fn registry_names_are_non_empty_and_unique() {
        let controllers = all_controllers();
        let names: BTreeSet<&str> = controllers.iter().map(|c| c.name()).collect();
        assert_eq!(names.len(), controllers.len(), "a controller name repeats");
        assert!(!names.contains(""), "a controller has an empty name");
    }
}
