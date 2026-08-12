# PR Summary — Issue #538

## Summary

`loss.rs` gains two additive public items so consumers stop re-implementing MSE.
Closes #538.

- **`mse_record(targets, outputs) -> f64`** — the per-record reduction (mean
  over outputs of `(target - output)^2`, `f64` accumulation, `0.0` for an empty
  record), now exported. The scalar `mse_sum_batch_packed` fallback closure and
  the `mse_mean_record` closure were the same maths written twice; both now
  delegate to it. The SIMD tiles (`interleaved_tile_mse`,
  `mse_sum_batch_scattered`) read strided/interleaved buffers and are
  bit-parity-critical — untouched.
- **`mse_mean_streaming(network, dir, input_size, num_outputs, forward_only,
  max_records) -> Result<(f64, u64), String>`** — mean per-record MSE over a
  `.bin` training directory, streamed. Chunked reads through
  `training_bin_stream::for_each_read_chunk_with_mode`, whole chunks scored via
  `mse_sum_batch_packed` so the tiled SIMD fast path still does the work, and a
  small residual buffer for records straddling a chunk boundary. Read sizing
  honours `NEAT_SCORER_IO_MODE` / `NEAT_SCORER_READ_BYTES` like every other
  `.bin` scan. `max_records` truncates the final chunk and stops the scan there,
  so the cap costs no extra I/O. Returns `(0.0, 0)` for a directory with no
  whole records — the caller decides whether that is an error. Not on the
  `wasm_bindgen` export surface.

Both are re-exported from `lib.rs`. No existing signature changed, so this is a
patch/minor bump, not a breaking one. The `wasm-bindgen 0.2.126 → 0.2.127` and
`Cargo.lock` bumps are `quality.sh`'s dependency refresh riding along.

### Early stop is loud, not silent

`for_each_read_chunk` has no early exit, so the `max_records` cap stops the
reader by returning a sentinel error from the chunk callback. That sentinel is
swallowed **only** when this function raised it (`cap_reached`) **and** the
error is nothing but the wrapped sentinel — when the reader thread also failed
it prefixes its own I/O message, and that error reaches the caller unchanged.

```mermaid
flowchart LR
    D["`.bin` directory<br/>find_bin_files"] --> F["for_each_read_chunk_with_mode"]
    F --> P["pending residual<br/>+ whole records"]
    P --> K["mse_sum_batch_packed<br/>per chunk"]
    K --> A["f64 sum + record count"]
    A --> C{"max_records hit?"}
    C -- yes --> S["sentinel Err → stop reading"]
    C -- no --> F
    S --> M["mean = sum / records"]
    A --> M
```

## Evidence

Backend/library change — no web interface to screenshot. Evidence is the test
suite plus per-site mutation runs.

`./quality.sh` passes clean (fmt, clippy `-D warnings`, deny, doc, release
build, and the full `cargo test --workspace`: 45 test groups, 0 failures). The
existing bit-parity tests (`interleaved_mse_parity`,
`mse_batch_interleaved_parity`, `mse_squash_simd_parity`,
`packed_record_scan`, `batch_record_skeleton`) pass **unchanged** — the
delegation is bit-identical because `outputs.len() == num_outputs` in the scan,
so the divisor and the summation order are the same values in the same order.

### Mutation evidence (AGENTS.md rule 2 — every former site must die)

Each mutation was applied alone, the suite run with `--no-fail-fast`, then
reverted.

| # | Mutation | Result |
| --- | --- | --- |
| A | `mse_record` returns `… + 0.5` | **red** — 18 tests across `loss::tests`, `packed_record_scan`, `batch_record_skeleton`, `mse_batch_interleaved_parity`, `inline_squash_dispatch`, and both new suites |
| B | former site 1: `mse_sum_batch_packed` calls `\|t, o\| mse_record(t, o) + 0.5` | **red** — 15 tests, incl. `every_sum_entry_point_equals_the_sum_of_its_single_record_scans`, `stateless_reset_is_conditional_on_forward_only`, `max_records_truncates_to_the_mean_over_the_first_n_records` |
| C | former site 2: `mse_mean_record` calls `\|t, o\| mse_record(t, o) + 0.5` | **red** — `mse_mean_record_matches_hand_rolled_reference`, `mse_mean_record_agrees_with_sum_divided_by_records_on_forward_only`, `stateless_reset_is_conditional_on_forward_only` |
| D | drop the `max_records` truncation (`whole.min(room)`) | **red** — `max_records_truncates_to_the_mean_over_the_first_n_records` |
| E | return the sum instead of `sum / records` | **red** — 7 of the new streaming tests |

B and C kill disjoint test sets, so the suite reaches **both** collapsed copies
independently, not just the shared helper.

### Oracle independence

Expected values come from the single-record `CompiledNetwork::activate` forward
pass with the squared-error arithmetic written out in the test — never from
`mse_record` or the batched kernels — so a fault inside the code under test
moves only one side of each assertion (AGENTS.md rule 1). Tolerance is `1e-6`:
chunking re-associates the sums, so parity is stated, not bit-exact. The
`streaming_mean_equals_packed_sum_over_record_count` test is the issue's
acceptance criterion and deliberately does share the kernel; it is not the only
oracle.

## Test Plan

New — `neat-core/tests/mse_streaming_directory.rs` (15 tests):

- `mse_record_averages_squared_error_over_outputs`,
  `mse_record_of_an_exact_prediction_is_zero`,
  `mse_record_empty_outputs_is_zero`,
  `mse_record_accumulates_in_f64_beyond_f32_precision` (an `f32` accumulator
  would overflow to `+inf`).
- `streaming_mean_matches_the_scalar_activate_reference` (37 records, 3 shards)
  and `streaming_mean_equals_packed_sum_over_record_count` (the issue's
  `mse_sum_batch_packed(..) / N` criterion).
- `streaming_an_empty_directory_yields_zero_records`,
  `streaming_a_directory_of_empty_files_yields_zero_records`,
  `streaming_a_missing_directory_yields_zero_records`,
  `a_zero_width_record_yields_zero_records` — all `(0.0, 0)`.
- `max_records_truncates_to_the_mean_over_the_first_n_records` (caps 1/7/10/23
  against the scalar reference), `max_records_of_zero_reads_nothing`,
  `max_records_above_the_corpus_size_scores_every_record`.
- `a_trailing_partial_record_is_not_scored`.
- `forward_only_false_resets_state_between_records` — self-loop network; the
  test also asserts the leaked-state value *differs*, so the check is not
  vacuous.

New — `neat-core/tests/mse_streaming_chunk_boundary.rs` (1 test, its own binary
because it sets a process-wide env var): `NEAT_SCORER_READ_BYTES=20` against a
12-byte record forces every chunk to end mid-record;
`records_straddling_a_read_chunk_boundary_are_scored_once` asserts the record
count, the mean, and that the cap still holds across boundaries.

Docs: `README.md` gains a "Streaming directory MSE (Issue #538)" section with a
Mermaid flow; `AGENTS.md` records `mse_record` as the single home of the scalar
reduction.
