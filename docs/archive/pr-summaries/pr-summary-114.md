# PR Summary — Issue #114

## Summary

Several `pub const` items in the public `squash` and `range` modules carried
only ordinary `//` line comments, which rustdoc ignores, so they rendered in
`cargo doc` with no explanation. This change promotes those comments to `///`
rustdoc doc comments and adds one-line summaries where none existed. It also
adds a `///` summary to the public `apply_limit_range_f64` function.

Items documented:

- `src/squash.rs`: `SELU_ALPHA`, `SELU_LAMBDA`, `GELU_COEFF`, `SQRT_2_OVER_PI`,
  `LEAKY_RELU_ALPHA`, `JS_MAX_SAFE_INTEGER`, `SOFTSIGN_LIMIT`,
  `TAN_OUTPUT_CLAMP`, `SQUARE_OUTPUT_CLAMP`, `CUBE_OUTPUT_CLAMP`.
- `src/range.rs`: `GELU_MIN`, `SWISH_MIN`, `MISH_MIN`, `SOFTPLUS_MIN`,
  `SOFTPLUS_MAX`, `F32_LARGE`, and the `apply_limit_range_f64` function.

This is a documentation-only change to the public API surface — no runtime
behaviour changes. Closes #114.

## Evidence

Backend/library change with no web interface to screenshot. Verification:

- `cargo doc --workspace --no-deps` with `RUSTDOCFLAGS="-D warnings"` builds
  cleanly — the doc comments now render for these items.
- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets
  --all-features -- -D warnings` pass.
- The new behaviour test for the previously-untested public function passes.

```mermaid
flowchart LR
    A["// line comment<br/>(invisible to rustdoc)"] --> B["/// doc comment"]
    B --> C["renders in cargo doc"]
```

Note: `./quality.sh` halts early on four pre-existing `bats` failures in
`tests/scripts` (CI `ci.yml`/`bump-deps.sh` workflow assertions, tests 31–33
and 37). These fail identically on the unmodified base branch and are
unrelated to this change; the Rust gate (fmt, clippy, check, tests, doc) was
run directly and passes.

## Test Plan

- Added `neat-core/tests/range.rs::test_limit_range_f64_clamping` — exercises
  the public `apply_limit_range_f64`: in-range pass-through, out-of-range
  clamping, `NaN → 0.0`, bounded-range infinity clamping, and
  unbounded-range infinity clamping to `±F32_LARGE` (stays finite).
- Existing `tests/range.rs` and `tests/squash.rs` suites continue to pass.
