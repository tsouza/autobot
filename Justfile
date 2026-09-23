# AutoBot development frontend: every local and CI action goes through a recipe here.

set shell := ["bash", "-euo", "pipefail", "-c"]

# rust-script builds outside the repository, so it would not see rust-toolchain.toml.
export RUSTUP_TOOLCHAIN := `sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml`

# --force so edits to autobot-devtools are never ignored; --debug for fast builds.
rs := "rust-script --force --debug"

# List the recipes.
default:
    @just --list

# Format every crate.
fmt:
    cargo fmt --all

# Check formatting.
fmt-check:
    cargo fmt --all --check

# Lint every target, denying warnings.
clippy:
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

# Run the test suite and the doctests.
test:
    cargo nextest run --workspace --all-features --locked --no-tests=pass
    cargo test --workspace --all-features --locked --doc

# Build the documentation, denying warnings.
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked

# Check licenses, advisories, bans and sources.
deny:
    cargo deny --all-features check

# Find unused dependencies.
machete:
    cargo machete

# Check the design set in docs/design for consistency.
check-design:
    {{rs}} scripts/check_design.rs docs/design

# Fail when a GLOSSARY retired term is used in the design, the crates, formal or deploy.
check-retired-terms:
    {{rs}} scripts/retired_terms.rs .

# Check traceability: I-n, F-n, fixture groups, owning tests and model invariants.
# Arguments: none, `--closed G-X`, `--awaiting #N` or `--diff`.
[positional-arguments]
trace-lint *args:
    {{rs}} scripts/trace_lint.rs "$@"

# Everything CI checks, locally.
ci: fmt-check clippy test doc deny machete

# Apply the ruleset and repository settings to GitHub; `--dry-run` only prints the diff.
repo-settings *args:
    {{rs}} scripts/repo_settings.rs {{args}}

# Post the `review-gate` commit status on pull request `pr` from its latest review verdict.
review-gate pr:
    {{rs}} scripts/review_gate.rs {{pr}}

# CI job: formatting.
ci-fmt: fmt-check

# CI job: lints.
ci-clippy: clippy

# CI job: tests.
ci-test: test

# CI job: documentation.
ci-doc: doc

# CI job: dependency policy.
ci-deny: deny

# CI job: unused dependencies.
ci-machete: machete

# Worktree per pull request: `just wt new <issue#>`, `just wt list`, `just wt rm <issue#>`.
[positional-arguments]
wt *args:
    {{rs}} scripts/wt.rs "$@"

# Install the git pre-push hook that checks pushes against ~/.config/autobot/deny-terms.
hooks:
    {{rs}} scripts/hooks.rs install

# What the pre-push hook runs; git passes the remote name and URL, and the refs on stdin.
[positional-arguments]
hook-pre-push *args:
    {{rs}} scripts/hooks.rs pre-push "$@"

# Check the local build setup; `--measure` also reports the sccache hit split.
[positional-arguments]
doctor *args:
    {{rs}} scripts/doctor.rs "$@"

# Lint the issue graph on GitHub (read-only).
dag-lint:
    {{rs}} scripts/dag_lint.rs

# Render the blocked-by graph as Mermaid; `--critical` prints the longest open chain instead.
dag *args:
    {{rs}} scripts/dag.rs {{args}}

# Scheduled job: issue-graph lint.
ci-dag-lint: dag-lint
