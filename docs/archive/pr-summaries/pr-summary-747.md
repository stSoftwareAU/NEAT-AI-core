## Summary

Issue #747: the canonical `scripts/runlib.sh` toolchain gate could only enforce
a Rust version that something **declares**. A crate that declares neither a
`rust-toolchain.toml` pin nor its own `rust-version` had its "requirement" taken
entirely from third-party dependencies. That is how NEAT-AI-Discovery#2395 got
through: its code needed Rust 1.95, the graph's maximum was 1.93.1, and old
hosts were passed straight into a `cargo build` that failed with `E0658`.

On this branch:

- `scripts/runlib.sh` — `_runlib_check_msrv` now refuses, before any `rustc` or
  `cargo metadata` call, a crate that has neither an exact
  `rust-toolchain.toml` pin nor its own `rust-version`. It exits non-zero and
  names both fixes.
- `rust-toolchain.toml` (new) — pins NEAT-AI-core to the exact `1.98.0`, with
  `rustfmt`, `clippy` and the `wasm32-unknown-unknown` target.
- `Cargo.toml` / `neat-core/Cargo.toml` — declares `rust-version = "1.98.0"`
  (`[workspace.package]`, inherited by `neat-core`).
- `quality.sh` — the header comment now describes the pin instead of
  "use rustup `stable`".
- `RELEASING.md` — a `0.23.0` breaking-change entry for the declared MSRV.
- `README.md` — documents the refusal in the canonical `runlib.sh` section.
- `tests/scripts/core_toolchain_pin.bats` (new) — checks this repository's pin
  and `rust-version` through runlib's own helpers.

**Not done on this branch** (issue proposed-fix item 2 and part of the
definition of done): the one-shot `E0658` self-heal (`rustup update` + a single
retried build). There is no E0658 handling in `runlib_install`. The README had
described it as shipped, and this summary commit removes those passages (see
Docs sweep). The header comment of `scripts/runlib.sh` (lines 92–98) still
describes the retry. That is a code file and was left untouched here, so it is
still a false statement and must be corrected before merge.

**Known regression on this branch.** `bats tests/scripts/runlib.bats`: **61 of
98 tests fail at this head**, 0 on `origin/Develop`. Most of its fixture crates
declare neither a pin nor a `rust-version`, so the new refusal stops them before
the build they exercise. An example is `a manifest without rust-version skips
the MSRV check`, whose premise this issue makes untrue. The fixtures and that
test have not been updated, and no `runlib.bats` case asserts the refusal
itself. This branch is not merge-ready until that is fixed.

## Reproduction

- **symptom** — the toolchain gate passes any `rustc` for a crate whose real
  requirement is undeclared, and NEAT-AI-core itself declared neither an exact
  `rust-toolchain.toml` pin nor a `rust-version`.
- **status** — `partial` — reason: for NEAT-AI-core's own missing declaration
  the fail-before / pass-after was observed. `core_toolchain_pin.bats` tests 1
  and 2 fail against `origin/Develop` and pass at this head (test 3 passes on
  both, vacuously on base, because two empty values compare equal). The gate
  half — an undeclared fixture crate refused by `runlib.sh` — has no regression
  test. The `E0658` retry is not implemented, so it was not reproduced.
- **regression test** —
  `tests/scripts/core_toolchain_pin.bats::the repository root pins an exact rust-toolchain.toml channel`,
  `tests/scripts/core_toolchain_pin.bats::neat-core resolves a non-empty rust-version`

## Test Plan

- No assertion was removed from an existing test. The only test-file change is
  the new `tests/scripts/core_toolchain_pin.bats` (3 tests, all green at head).
- `tests/scripts/runlib.bats` is unchanged, but 61 of its 98 tests now fail
  against this head's `scripts/runlib.sh` (see Summary). Those fixtures still
  need an exact pin or a `rust-version`, and the "without rust-version skips"
  test needs to become a refusal assertion.

**Branch outcomes:**
- `scripts/runlib.sh:782` — exact `rust-toolchain.toml` pin → refusal skipped,
  gate proceeds — `tests/scripts/runlib.bats::a pin above the requirement passes with no rustup call`
  — flipped to `if true` (refusal evaluated despite the pin), test went red
- `scripts/runlib.sh:784` — no exact pin, own `rust-version` declared → passes
  the refusal — `tests/scripts/runlib.bats::a rustc at or above the manifest MSRV builds`
  — flipped to `[[ -z … ]]`, test went red
- `scripts/runlib.sh:784` — neither declared → `_runlib_die`, non-zero exit —
  **no test asserts this outcome**. It is reached by
  `tests/scripts/runlib.bats::a manifest without rust-version skips the MSRV check`,
  which is **red** at this head because it asserts the old pass. Flipping the
  guard to `true ||` (no refusal) turns that test green.

**Docs sweep** — grep: `_runlib_check_msrv`, `rust-version`, `rust-toolchain`, "toolchain gate", `E0658`, `tee` over `README.md`, `docs/` (excluding `docs/archive/`), `*/README.md`, `AGENTS.md`, `RELEASING.md`; section: `README.md#canonical-runlibsh-issue-680`; updated: `README.md`. Removed the false E0658 claims: the "Fail" row sentence, the `tee`/`mktemp` build-path requirement, the "One E0658 self-heal" paragraph, the E0658 nodes in the flowchart, and the sentence claiming shim-log tests for the refusal and the retry. The refusal paragraph, the Gate row and the refusal node are true of the code and stay. The `scripts/runlib.sh` header comment (lines 92–98) is still false but is in a code file, so it is flagged above and not edited.
