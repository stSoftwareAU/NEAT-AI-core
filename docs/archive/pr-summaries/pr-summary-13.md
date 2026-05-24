## Summary

Added tunable I/O modes to `training_bin_stream` so the NEAT-AI scorer can A/B
between the existing pipelined double-buffer reader and a simpler sequential
reader at run time — no rebuild required. Closes #13.

Carried over from the unmerged `makefaster` branch (PR #2):

- `TrainingReadMode { PipelinedDoubleBuffer, SingleBufferSequential }` —
  `Default` is `PipelinedDoubleBuffer`.
- `training_read_tuning_from_env(record_bytes)` — reads
  `NEAT_SCORER_IO_MODE` (case-insensitive `single`/`double`, default `double`)
  and `NEAT_SCORER_READ_BYTES` (decimal `usize`, default 2 MiB, clamped to
  `[record_bytes.max(1), 64 MiB]`).
- `io_backend_label(mode)` — stable telemetry string. On `wasm32` always
  `"sequential_chunked_file_read"`; on native `"pipelined_double_buffer"` or
  `"single_buffer_sequential"`.
- `for_each_read_chunk_with_mode` — primary native entry point; ignores the
  `mode` on `wasm32`.
- `for_each_read_chunk` is now a thin wrapper that delegates with
  `PipelinedDoubleBuffer` (the legacy `NEAT_TRAINING_READ_SEQUENTIAL` escape
  hatch from issue #25 is still honoured for backwards compatibility).
- The private `for_each_read_chunk_sequential` is shared between the
  `SingleBufferSequential` path and `wasm32`; `for_each_read_chunk_native`
  renamed to `for_each_read_chunk_native_double`.
- Module doc comment updated to describe both modes and the new env vars.

## Evidence

Backend/library change — no UI. Verification via tests and quality gate:

- `cargo test -p neat-core --lib training_bin_stream` — 19 passed, 0 failed.
- `./quality.sh` — green (fmt, clippy `-D warnings`, deny, tests, doc, release
  build).
- `cargo build -p neat-core --target wasm32-unknown-unknown` — compiles
  cleanly; the `mode` argument is suppressed with `let _ = mode;` on
  `wasm32` so the warning-free build is preserved.

## Test Plan

Added to `neat_core::training_bin_stream::tests`:

- `both_modes_agree_on_multi_file_corpus` — multi-file fixture spanning a
  chunk boundary; asserts the two modes produce byte-identical output via the
  new `run_files(mode)` helper.
- `both_modes_handle_empty_files` — empty-files coverage for both modes.
- `zero_read_buf_len_is_rejected_in_both_modes` — the existing
  `"read_buf_len must be positive"` error path still fires under
  `for_each_read_chunk_with_mode` for both modes.
- `training_read_mode_default_is_pipelined_double_buffer` — verifies the
  documented default.
- `io_backend_label_native_reflects_mode` — stable telemetry strings.
- `training_read_tuning_defaults_without_env` — no env vars → pipelined +
  default 2 MiB.
- `training_read_tuning_parses_modes_case_insensitively` — `single`/`SINGLE`/
  `Single`/`double`/`DOUBLE`/unknown all map correctly.
- `training_read_tuning_clamps_read_bytes` — below lower bound, above upper
  bound, within-bounds pass-through, and unparsable fallback to
  `DEFAULT_READ_BYTES`.
- `training_read_tuning_handles_zero_record_bytes` — `record_bytes = 0`
  clamps up to 1.

All pre-existing tests in the module are retained (including the issue #25
teardown race regression and the legacy `SEQUENTIAL_ENV` coverage) and
continue to pass.
