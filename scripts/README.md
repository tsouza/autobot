# Scripts

`scripts/<name>.rs` files are thin [rust-script](https://rust-script.org) entry points. Each declares `autobot-devtools = { path = "devtools" }` in its embedded manifest (the path is relative to the script) and calls one function of that library; all logic lives in `scripts/devtools`, which is a workspace member and goes through the same lint and test gates as every other crate.

A script is invoked as `rust-script --force --debug scripts/<name>.rs` with `RUSTUP_TOOLCHAIN` set to the channel in `rust-toolchain.toml`: rust-script builds in its own cache directory, so it sees neither `rust-toolchain.toml` nor the repository's `.cargo/config.toml` by itself, and `--force` makes it rebuild when the library changes.

`autobot-devtools` dependencies are pinned with `=x.y.z`, because rust-script resolves them outside the workspace lock file. The GitHub client sits behind the non-default `github` feature; crates that use the library as a dev-dependency set `default-features = false`.

The GitHub client reads its token from the first non-empty of `GITHUB_TOKEN` and `GH_TOKEN`, and otherwise from `<cli> auth token`, where `<cli>` is the `AUTOBOT_GH_CLI` environment variable (default `gh`).
