# dead-code: remove `apply_squash_f64`

## Summary

Deleted the ~136-line `apply_squash_f64` f64 mirror from
`neat-core/src/squash.rs` and its test-only callers. The function had no caller
outside its own integration tests: it is not re-exported from `lib.rs`, has no
`#[wasm_bindgen]` wrapper, and `network.rs` has no f64 squash path — so its doc
comment's claim about "the compiled-network activator" described a caller that
does not exist. The redundant `#[allow(dead_code)]` went with it (`dead_code`
never fires on a `pub` item in a reachable `pub mod` of a library crate, so it
suppressed nothing and merely obscured that the function was unused).

The shared clamp/activation constants (`TAN_OUTPUT_CLAMP`,
`SQUARE_OUTPUT_CLAMP`, `CUBE_OUTPUT_CLAMP`, `SQRT_2_OVER_PI`, `GELU_COEFF`,
`SELU_*`) are still used by the f32 `apply_squash` and were left untouched.

No public-surface change and no behaviour change. Closes #426.

## Evidence

Backend/library-only change — there is no web interface to screenshot.

Symbol is gone from the crate:

```
$ grep -rn '\bapply_squash_f64\b' neat-core/
$ grep -n 'allow(dead_code)' neat-core/src/squash.rs
(no hits)
```

`./quality.sh` (fmt, clippy `-D warnings`, deny, tests, doc, release build):

```
✅ All quality checks passed!
```

`cargo test --workspace --lib --tests --all-features -- --test-threads=2` — all
suites green, including the surviving f32 clamping tests in
`neat-core/tests/squash.rs`.

## Test Plan

Deleted (no coverage lost — each has a surviving f32 twin exercising the same
clamp constant):

- `neat-core/tests/squash.rs::test_tan_clamped_f64` → covered by
  `test_tan_clamped_f32` (`TAN_OUTPUT_CLAMP`)
- `neat-core/tests/squash.rs::test_square_clamped_f64` → covered by
  `test_square_clamped_f32` (`SQUARE_OUTPUT_CLAMP`)
- `neat-core/tests/squash.rs::test_cube_clamped_f64` → covered by
  `test_cube_clamped_f32` (`CUBE_OUTPUT_CLAMP`)

No tests added: this is a pure dead-code removal with no new behaviour. The
compile gate is the primary net — clippy `-D warnings` fails on any missed call
site because the symbol no longer exists.
