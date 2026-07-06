# PR Summary — Issue #237

## Summary

`neat-core/tests/bench_fixtures.rs::score_records_on_production_batch_yields_finite_ordered_outputs`
failed to compile on `milestone/227`. Issue #229 changed
`CompiledNetwork::score_records` to return a **flat** `Vec<f32>` in the
`[record * num_outputs]` layout (dropping the per-record `Vec`), but the
bench_fixtures test still treated the result as `Vec<Vec<f32>>` rows —
calling `.len()` and `.flatten()` on `&f32`. Because bench_fixtures is a
`--tests` target, this broke `cargo test --workspace --lib --tests
--all-features`, blocking the whole CI test build and `quality.sh` on the
milestone branch.

The fix updates the assertions to the flat layout, mirroring
`tests/parallel_scoring.rs`: assert the flat length is
`records.len() * prod.num_outputs` and that every element is finite.

Fixes #237.

### Base branch

This PR targets **`milestone/227-speed-up-neat-evolution-on-production-sized-cr`**,
not `Develop`. The flat `score_records` layout (#229) and the offending test
exist **only** on the milestone branch — `Develop` still returns
`Vec<Vec<f32>>` and does not contain this test — so the compile break, and its
fix, belong on the milestone branch.

```mermaid
flowchart LR
    A["#229: score_records → flat Vec&lt;f32&gt;"] --> B["bench_fixtures still<br/>treats output as rows"]
    B --> C["E0599 / E0277:<br/>--tests target won't compile"]
    C --> D["CI test build + quality.sh<br/>blocked on milestone/227"]
    D --> E["#237 fix: assert flat<br/>length + finiteness"]
    E --> F["cargo test compiles &amp; passes"]
```

## Evidence

Backend/test-only change — no web interface to screenshot.

**Before (clean milestone base):**

```
error[E0599]: no method named `len` found for reference `&f32`
  --> neat-core/tests/bench_fixtures.rs:197
error[E0277]: `&f32` is not an iterator
  --> neat-core/tests/bench_fixtures.rs:199
error: could not compile `neat-core` (test "bench_fixtures") due to 3 previous errors
```

**After:**

```
running 9 tests
test score_records_on_production_batch_yields_finite_ordered_outputs ... ok
...
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

## Test Plan

- `cargo test --test bench_fixtures --all-features` — compiles and all 9 tests
  pass, including the previously-broken
  `score_records_on_production_batch_yields_finite_ordered_outputs`.
- The reproduction: the same command failed to compile against the unmodified
  milestone base with E0599/E0277; it compiles and passes after the change.

## Out of scope

`./quality.sh` reports four pre-existing failures unrelated to this change
(BATS 48/49/50/54, concerning `ci.yml` / `bump-deps.sh` dependency-quarantine
wiring). These fail identically on the clean milestone base **without** this
change and are not caused by, nor within the scope of, this compile-break fix.
This PR touches only `neat-core/tests/bench_fixtures.rs`.
