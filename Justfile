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

# Everything CI checks, locally.
ci: fmt-check clippy test doc deny machete

# Apply the ruleset and repository settings to GitHub; `--dry-run` only prints the diff.
repo-settings *args:
    {{rs}} scripts/repo_settings.rs {{args}}

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
