# AGENTS.md

## TDD (required)

- **Test-driven development:** new behaviour or bugfixes start from **failing tests**, then minimal implementation, then refactor. Do not land Rust changes without **tests** in `neat-core` (or the relevant crate) and a green **`cargo test --workspace`**.
- Run **`./quality.sh`** before commit/PR.

## Testing: "what" not "how"

All test cases must be **"what" tests** (same rule as **NEAT-AI** `CONTRIBUTING.md`):

- **What tests** run real code paths and assert on **observable outcomes**: return values, errors, compiled structures, numerical results, public invariants.
- **How tests** tie to **implementation detail** and are discouraged: asserting on private fields, internal call order, source greps, line counts, or "this helper was invoked" unless the contract under test is explicitly that wiring.

Name tests after the behaviour or outcome (e.g. `relu_maps_negative_to_zero`), not the mechanism (`relu_calls_clamp_branch`).

## Repository layout

- **`neat-core/`** — shared native library; **WASM** stays in **NEAT-AI** (`wasm_activation`).
- **`training_bin_stream`** (`neat-core/src/training_bin_stream.rs`) — **one** chunked `.bin` scan API: pipelined double-buffer reads on native hosts, sequential `File::read` chunks on `wasm32` (same `for_each_read_chunk` callback). Used by **NEAT-AI-scorer** for production-sized forward-only scoring.
- Root **`Cargo.toml`** is a **virtual workspace**; **`[workspace.package].version`** is what the PR **auto-bump** job edits; **`neat-core`** uses `version.workspace = true`.

## CI / secrets

- PR pipeline: version bump + **`cargo upgrade --incompatible`**, **`cargo audit`**, dependency review, rustfmt bot, then fmt/clippy/deny/tests/doc. Pushes need **`ACTIONS_PUSH`** (PAT with **contents:write**).
- **Versioning/release policy:** see **`RELEASING.md`** (Issue #251). A **breaking** change is a **major-equivalent** bump (pre-1.0: **minor**, `0.1.x → 0.2.0`); non-breaking is **patch**. Signal a break with the **`breaking-change`** PR label or a Conventional Commit `type!:` / `BREAKING CHANGE:` marker. The `version-increment` job bumps minor on a break; the `version-gate` job **fails** a break shipped on a patch-only bump; `release.yml` cuts a **`v<version>`** tag + GitHub release on `Develop`, decoupled from `wasm-bundle-<sha>`.
- **`clippy::uninlined_format_args`** is not denied in CI until the test corpus is cleaned up; workspace lints still deny **`filter_next`** / **`collapsible_if`**.
