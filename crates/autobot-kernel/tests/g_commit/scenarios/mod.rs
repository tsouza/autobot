//! The group's scenarios: store-agnostic scripts, each a function over `&mut dyn Driver`
//! ([`autobot_testkit::harness::Driver`]) that names no concrete store.
//!
//! The scripts depend only on `autobot-kernel`, `autobot-testkit` and `autobot-fakes`, and on
//! no item outside this directory, so a test target of another crate compiles them unchanged
//! by including this module by path, `#[path = "<this directory>/mod.rs"] mod scenarios;`, and
//! runs each one on its own driver. A driver for an asynchronous store blocks on each
//! operation inside [`Driver::perform`](autobot_testkit::harness::Driver::perform).
//!
//! The kernel surfaces a script needs that the kernel does not have yet are the ports of
//! [`autobot_testkit::registry::g_commit`], resolved through the installed testkit registry:
//! until an implementation task registers its port, a script that needs it fails with
//! `no <port> implementation is registered (awaiting #<task>)`. A script that needs several
//! ports resolves the one of the latest task first, so it fails naming the task its test awaits.
//!
//! A script whose outcome a FORMAL §5 guard of the group decides takes the [`Guards`] it runs
//! under and passes them to the surface the guard constrains, so a guard-removal run passes
//! the guards with that guard disabled.
//!
//! [`Guards`]: autobot_kernel::reducer::Guards

pub(crate) mod commands;
pub(crate) mod commit;
pub(crate) mod create;
pub(crate) mod projection;
pub(crate) mod store;
