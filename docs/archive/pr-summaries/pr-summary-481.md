## Summary

Removed seven counter-productive tests from `neat-core/tests/integration.rs`. Closes #481.

- **Six verbatim duplicates deleted** — `test_relu`, `test_tanh`, `test_logistic` were byte-identical to `tests/squash.rs`; `test_derivative_identity`, `test_derivative_relu` to `tests/derivative.rs`; `test_unsquash_identity` to `tests/unsquash.rs`. The per-module suites keep the coverage, so a future change to those contracts is now a one-file edit instead of two (audit check 9, near-duplicate test bodies).
- **`test_version_constant` deleted** — its body was `let _ = SquashType::Relu; let _ = SynapseType::Standard;` with no assertion. It tested no version constant despite its name, and compilation is already proven by every other test in the binary (audit check 10, framework-guarantee test).

Resolution **(a)** from the issue was taken rather than **(b)**. A crate-root smoke test would have been redundant: `squash_identity_round_trip` already exercises the `apply_squash` re-export from the crate root, the five surviving `apply_*` tests use crate-root paths, and the `SynapseType` re-export is covered by `tests/creature_compile.rs` and `tests/creature/roundtrip.rs`. `SynapseType` was dropped from the file's import list as it became unused. No production code changed.

## Evidence

Backend-only test cleanup — no web interface to screenshot. Verification is the test run itself.

`cargo test --test integration` — 14 tests before, 7 after, all passing; every survivor is genuine crate-root coverage:

```
running 7 tests
test squash_identity_round_trip ... ok
test test_calculate_error ... ok
test test_get_range ... ok
test test_limit_range ... ok
test test_safe_zone_adjustment ... ok
test test_validate_range ... ok
test parses_and_compiles_minimal_creature ... ok

test result: ok. 7 passed; 0 failed
```

The coverage the deleted duplicates claimed still runs, in its proper home:

```
Running tests/squash.rs      — 6 passed (test_relu, test_tanh, test_logistic, …)
Running tests/derivative.rs  — passed (test_derivative_identity, test_derivative_relu, …)
Running tests/unsquash.rs    — 12 passed (test_unsquash_identity, …)
```

`./quality.sh` passes cleanly (fmt, clippy, deny, `cargo test --workspace`, doc build, release build): `✅ All quality checks passed!`

## Test Plan

No tests added — this issue is the removal of counter-productive tests, which the issue explicitly sanctions as a valid PR outcome.

Tests removed from `neat-core/tests/integration.rs` (business-logic coverage unaffected; each duplicate's twin is named):

| Removed | Coverage retained by |
| --- | --- |
| `test_relu` | `neat-core/tests/squash.rs::test_relu` |
| `test_tanh` | `neat-core/tests/squash.rs::test_tanh` |
| `test_logistic` | `neat-core/tests/squash.rs::test_logistic` |
| `test_derivative_identity` | `neat-core/tests/derivative.rs::test_derivative_identity` |
| `test_derivative_relu` | `neat-core/tests/derivative.rs::test_derivative_relu` |
| `test_unsquash_identity` | `neat-core/tests/unsquash.rs::test_unsquash_identity` |
| `test_version_constant` | none needed — asserted nothing |
