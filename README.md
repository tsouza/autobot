# AutoBot

AutoBot is a durable multi-agent software delivery engine. It takes an accepted milestone and converges on it with a swarm of role-specialized agents — manager, worker, reviewer, tester, integrator, and an optional task-local micro-manager — spending as little frontier-model money as the quality bar allows, never losing work and never acting on stale authority.

Kubernetes custom resources are the source of truth; a Rust operator reconciles them. Forges, CI, model providers and telemetry are observed, adapted to or projected into, and none of them is authoritative.

## Goals, highest first

1. Converge on one milestone target.
2. Never lose work, never act on stale authority, and degrade gracefully under outage.
3. Minimize expected total cost per accepted milestone; quality is a constraint, cost is the objective.
4. Route semantic judgments to a typed-question service that selects from a deterministically computed set and grants nothing.
5. Mirror the state of work to forges as projections of canonical records.
6. Onboard from a design document or from a work-in-progress repository.
7. Keep observability as a side channel that authorizes nothing.

## Status

Design and foundation. The first milestone, **M0-Q**, is a non-production qualification slice: it proves one complete path — brief to canonical records — on an existing, non-Rust repository through Kubernetes resources, using a fake forge and CI, a deterministic fake semantic judge and fault-injected fake provider adapters. Progress is tracked in the [milestones](../../milestones).

## Charter

The laws, rules, conventions and vocabulary every change follows are in [CHARTER.md](CHARTER.md).

## Design

The design lives in [`docs/design/`](docs/design/). Read it in this order:

1. [Thesis](docs/design/AUTOBOT-THESIS.md)
2. [Glossary](docs/design/AUTOBOT-GLOSSARY.md)
3. [Trust model](docs/design/AUTOBOT-TRUST-MODEL.md)
4. [Kernel](docs/design/AUTOBOT-KERNEL.md)
5. [Roles and runtime](docs/design/AUTOBOT-ROLES-AND-RUNTIME.md)
6. [Onboarding and convergence](docs/design/AUTOBOT-ONBOARDING-AND-CONVERGENCE.md)
7. [Formal surface](docs/design/AUTOBOT-FORMAL-SURFACE.md)
8. [M0 and gates](docs/design/AUTOBOT-M0-AND-GATES.md)
9. [Extensions](docs/design/extensions/)

## Building

The toolchain is pinned in `rust-toolchain.toml`; `rustup` installs it on first use.

```sh
cargo build --locked
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
