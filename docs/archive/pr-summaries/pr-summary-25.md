## Summary

Fixes the `failed to return read buffer to pool` teardown race in the native pipelined path of `for_each_read_chunk` (`neat-core/src/training_bin_stream.rs`), and adds the `NEAT_TRAINING_READ_SEQUENTIAL` escape hatch documented in the issue. Closes #25.

Root causes addressed:

1. **Teardown race (reader thread dropped `empty_rx` too early)** — the reader used to send a `Chunk(buf, 0)` on EOF and then return, dropping `empty_rx`. If the consumer was still inside a slow `on_chunk` (e.g. Rayon fused MSE), its final `empty_tx.send(return_buf)` could race the reader's exit and fail with *"failed to return read buffer to pool"*.
2. **Buffer-pool shrinkage** — a closer look showed the reader consumed one buffer per file for the EOF read and never returned it. Over many files the pool reached zero and the next `empty_rx.recv()` would deadlock.

Fixes applied:

- The reader now holds its own `empty_tx` clone and **returns the buffer to the pool on EOF** instead of dropping it. This stops the pool from shrinking.
- A new **shutdown gate** (a third `sync_channel`) keeps `empty_rx` alive until the consumer has fully drained the pipeline. The reader parks on the gate after sending `Done` and only exits once the consumer signals it is finished. This is the "harden the pipelined design so the reader cannot drop the empty-buffer receiver until the consumer has returned all buffers" behaviour requested in the issue.
- The reader no longer forwards `Chunk(buf, 0)`; the consumer never sees an empty chunk.
- **`NEAT_TRAINING_READ_SEQUENTIAL=1`** (also `true`/`yes`/`on`, case-insensitive) selects the same sequential `File::read` loop used on `wasm32`. Downstream consumers can opt in to the simpler single-threaded path.
- **Richer error diagnostics**: open/read/`on_chunk` errors now include the file index (`file #N`) and, where relevant, the chunk size (`n={n} bytes`). Applies to both the pipelined and sequential paths.

Public API and observable output are unchanged for existing callers — only diagnostics are more detailed.

## Evidence

Backend/library change with no UI. Verified via new unit tests (see Test Plan). The full workspace test suite and `./quality.sh` run green locally.

## Test Plan

New `#[test]` cases in `neat-core/src/training_bin_stream.rs`:

- `slow_on_chunk_does_not_race_reader_teardown` — regression for issue #25: a slow `on_chunk` (20 ms sleep) across four 1 KiB files with a 256-byte buffer must complete without the pool-return race.
- `stress_many_small_files_pipelined` — 32 files of varying sizes (including a 1-byte file) stresses the EOF / pool-return path that previously leaked buffers.
- `sequential_env_var_selects_sequential_path` — `NEAT_TRAINING_READ_SEQUENTIAL=1` drives the sequential code path end-to-end.
- `sequential_env_var_accepts_truthy_variants` — `1`, `true`, `TRUE`, `Yes`, `on` all enable the escape hatch.
- `sequential_env_var_ignored_when_falsy` — `0`, `false`, `no`, `off`, empty-string keep the pipelined path.
- `sequential_and_pipelined_agree_on_output` — both paths yield the same concatenated byte stream for the same corpus.
- `on_chunk_error_includes_file_index_diagnostic` — `on_chunk` errors carry the file index and the original message.
- `missing_file_error_includes_file_index` — open-failure errors include the file index.
- `zero_read_buf_len_is_rejected` — preserves the existing input-validation contract.

Existing `for_each_read_chunk_concatenates_files` test retained unchanged.
