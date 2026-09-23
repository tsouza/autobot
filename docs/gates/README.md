# Gate records

Each gate of the gate table (`docs/design/AUTOBOT-M0-AND-GATES.md` §4) that has been run has one directory here, named by its gate id: `G-QUAL` for the row `M0-Q / G-QUAL`, `G-FORMAL` for `G-FORMAL`. A gate without a directory is `NOT_RUN`. Records are repository files; there is no Gate custom resource.

## Layout

```text
docs/gates/<gate>/record.json   the record manifest
docs/gates/<gate>/<artifact>    each evidence artifact the manifest lists
```

## Manifest

```json
{
  "version": 1,
  "gate": "G-QUAL",
  "state": "PASSED",
  "digests": {
    "installation": "sha256:…",
    "software": "sha256:…",
    "policy": "sha256:…",
    "profile": "sha256:…"
  },
  "artifacts": [
    { "path": "formal-sim.txt", "digest": "sha256:…" }
  ]
}
```

- `gate` equals the directory name.
- `state` is `RUNNING`, `PASSED`, `FAILED` or `INVALIDATED`. `NOT_RUN` is never written.
- `digests` holds exactly the four bindings below, each `sha256:` followed by 64 lowercase hexadecimal digits.
- `artifacts` lists each evidence file by its path relative to the gate directory (plain `/`-separated components, no `.` or `..`) and the SHA-256 of its bytes. A `PASSED` record lists at least one.
- No other key is accepted.

## Bindings

| Binding | Computed from |
| --- | --- |
| `installation` | the tracked files under `deploy/` |
| `software` | the tracked files `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` and under `crates/` |
| `policy` | the tracked files `CHARTER.md` and `deny.toml` |
| `profile` | the kernel's profile digest of `profiles/m0.toml` |

A file binding's digest is the SHA-256 of one `<sha256:file digest> <path>` line per tracked file, sorted by path, each ending in a newline; a symbolic link contributes the digest of its target path. `just gate-status` prints the current value of every binding, which is what a new record copies into `digests`.

## Signature

A record is signed by the owner's SSH-signed annotated tag on the record commit, the last commit that changed the gate directory:

```sh
git -c gpg.format=ssh tag -s gate/<gate>/<n> -m "<gate> record" <record-commit>
git push origin gate/<gate>/<n>
```

The tag name starts with `gate/<gate>/`. A verifier trusts the owner's key through its own `gpg.ssh.allowedSignersFile`; the file is not in the repository.

## Recipes

- `just gate-status` prints the current binding digests and each gate's state. A `PASSED` record whose installation, software, policy or profile digest differs from the current one is reported `INVALIDATED`, with the bindings that differ; every other record is reported in its recorded state. It writes no file, does not check signatures, and fails only on a manifest it cannot parse or a directory here that is not a gate id. The CI `test` job runs it.
- `just gate-evidence <gate>` verifies one record: every listed artifact exists with its digest, the gate directory has no uncommitted or untracked change, and a tag `gate/<gate>/*` on the record commit is an SSH-signed annotated tag that `git verify-tag` accepts. It fails when any of these does not hold.
