# Scripts

`scripts/<name>.rs` files are thin [rust-script](https://rust-script.org) entry points. Each declares `autobot-devtools = { path = "devtools" }` in its embedded manifest (the path is relative to the script) and calls one function of that library; all logic lives in `scripts/devtools`, which is a workspace member and goes through the same lint and test gates as every other crate.

A script is invoked as `rust-script --force --debug scripts/<name>.rs` with `RUSTUP_TOOLCHAIN` set to the channel in `rust-toolchain.toml`: rust-script builds in its own cache directory, so it sees neither `rust-toolchain.toml` nor the repository's `.cargo/config.toml` by itself, and `--force` makes it rebuild when the library changes.

`autobot-devtools` dependencies are pinned with `=x.y.z`, because rust-script resolves them outside the workspace lock file. Each optional dependency sits behind a non-default feature, which a script enables in its embedded manifest (`features = ["<name>"]`) only when it calls what the feature gates:

| Feature | Dependency | Gates | Used by |
| --- | --- | --- | --- |
| `github` | `ureq` | the `github` module (the GitHub API client), the `scope` module behind `just scope`, `worktree::cli`, and the `Http` and `Token` variants of `Error` | `scripts/dag.rs`, `scripts/dag_lint.rs`, `scripts/dependabot_automerge.rs`, `scripts/label_gate.rs`, `scripts/main_red.rs`, `scripts/repo_settings.rs`, `scripts/review_gate.rs`, `scripts/scope.rs`, `scripts/sensitive_terms.rs`, `scripts/wt.rs` |
| `hooks` | `regex` | the `hooks` module: the hook installer behind `just hooks` and the deny-terms check behind `just hook-pre-push`, and, with `github`, the `sensitive` module behind `just sensitive-terms` | `scripts/hooks.rs`, `scripts/sensitive_terms.rs` |
| `doctor` | `toml` | the `doctor` module behind `just doctor` | `scripts/doctor.rs` |
| `judge` | `ring`, and `github` | the `judge` module: the `judged` charter entries asked of a pull request through the typed-question service, behind `just judge` | `scripts/judge.rs` |
| `ledger` | `judge` | the `ledger` module: the delivery report over merged pull requests, their review verdicts and `judge` reports, and the red runs on `main`, behind `just delivery-ledger` | `scripts/delivery_ledger.rs` |
| `gates` | `autobot-kernel`, `sha2` | the `gates` module: the gate records behind `just gate-status` and `just gate-evidence` | `scripts/gate_evidence.rs`, `scripts/gate_status.rs` |

The default feature set is empty. Crates that use the library as a dev-dependency set `default-features = false` and enable none of these features. The workspace lint, test and doc recipes build with `--all-features`, so every feature goes through the gates.

The GitHub client reads its token from the first non-empty of `GITHUB_TOKEN` and `GH_TOKEN`; set one of them before running a script that calls the GitHub API.
