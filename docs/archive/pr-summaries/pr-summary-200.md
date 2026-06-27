## Summary

Removed the dead `manifest_dir` helper from the shared integration-test
`common` module (`neat-core/tests/common/mod.rs`). The function was an
exported test helper with **no caller anywhere in the repo** — it was
silenced with `#[allow(dead_code)]` and a doc comment noting it was
forward-looking scaffolding for fixtures that were never added. Its only
dependency, `use std::path::PathBuf;`, became unused once the function
was deleted, so that import was dropped too. Closes #200.

## Evidence

This is a test-only, CLI/library change with no web interface to
screenshot.

A full-repo reference search confirmed `manifest_dir` had no caller,
qualified path, or glob re-export (only its own definition matched):

```
$ grep -rn "manifest_dir" --include="*.rs" .
neat-core/tests/common/mod.rs:10:pub fn manifest_dir() -> PathBuf {
```

After removal, the `dead_code` lint no longer flags the symbol:

```
$ RUSTFLAGS="--force-warn dead_code" cargo check --workspace --all-targets --all-features
# no warning for tests/common/mod.rs / manifest_dir
```

The rest of the `common` module (`minimal_creature_json`) is still used
by the integration-test harnesses (`creature_compile.rs`,
`integration.rs`, `tests/creature/*`, etc.), which continue to compile
and pass under `cargo test --workspace` via `./quality.sh`.

## Test Plan

- No new test added: this is a pure dead-code removal. The guarantee is
  enforced by the compiler's `dead_code` lint (verified above) and by the
  existing integration-test suite, which exercises the surviving
  `common::minimal_creature_json` helper and still compiles/passes.
- Ran `./quality.sh < /dev/null`. The only failures (tests 43, 44, 45,
  49 — concerning `ci.yml`/`bump-deps.sh` quarantine wiring) are
  **pre-existing on the baseline** `Develop` branch and unrelated to this
  change; verified by stashing the change and re-running. This change
  introduces no new failures.
