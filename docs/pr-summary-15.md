## Summary
Adds a public `mse_mean_record` function to `neat-core` so `rust_scorer` in
NEAT-AI-scorer can compile — the symbol was referenced in
`rust_scorer/src/main.rs:24` and the NEAT-AI-scorer README but had never been
added to `neat-core`. The function activates the network once per record on
the non-fused recurrent path (`forwardOnly: false`, i.e. `reset_state()`
between records), computes squared error per output summed across a record's
outputs, and returns the **mean per-record MSE** — matching the TypeScript
`MSE.calculate()` semantics used in NEAT-AI. The new symbol is re-exported
from `neat-core/src/lib.rs`. Closes #15.

## Evidence
Backend/library change — no UI to screenshot. Verified by:

- `cargo test -p neat-core --lib mse_mean_record` → 3 passed.
- `./quality.sh` → passed (rustfmt, clippy with `-D warnings`, cargo-deny,
  workspace build, tests, docs with `-D warnings`, release build).

## Test Plan
Three `#[cfg(test)] mod tests` cases added to `neat-core/src/loss.rs`, all
asserting on returned values (`"what"` tests):

- `mse_mean_record_matches_hand_rolled_reference` — asserts the returned
  mean against a hand-computed reference over a small synthetic dataset on
  the linear identity creature.
- `mse_mean_record_agrees_with_sum_divided_by_records_on_forward_only` —
  asserts `mse_mean_record == mse_sum_batch_packed(..., forward_only=true) /
  num_records` within `1e-6` on a 9-record fixture (exercises the 8-way
  SIMD path in `mse_sum_batch_packed`).
- `mse_mean_record_empty_input_returns_zero` — asserts `0.0` for an empty
  record slice and for the degenerate `input_size=0, num_outputs=0` case
  (choice documented in the doc-comment).
