# Metal supplement v5 — frozen before validation

The direct native probe found an Apple Paravirtual Metal device even though
`system_profiler` listed no display adapter. This supplement exercises the
scorer's existing public `BatchedRunner`, `select_adapter` and `catch_gpu_panic`
APIs, not a copied GPU kernel. Scorer source is pinned to
`742c5df30fc98ece3afe784be1aca18f925b47b3`; both it and the adapter use the same
`neat-core` release tag `v0.23.4`. The standalone experiment is outside the core
workspace and changes no production API or default.

Freeze this source and [manifest](manifest-metal-v5.json) before executing on
the Apple runner. Calibration seed 1001 remains separate from fresh validation
network seed 42002, input seed 43003 and target seed 44004. Targets are independently
generated and nonzero, so sign errors do not hide behind squared zero targets.
The six shapes, three inactive fractions, widths 1/8/32, 64 records, 15 paired
repetitions and 2,000 bootstrap resamples remain. Limits are 15 seconds per cell
and 600 seconds overall, soft at paired-round boundaries; the outer command
cap is 900 seconds. No threshold is tuned after validation.

The supported GPU surface accepts canonical f32 networks and returns MSE sums.
It cannot accept experimental CSR/COO/blocked buffers or bf16/f16 weight types.
There are 54 planned canonical f32 cells, 810 unavailable sparse format/precision
slots and 54 whole-scan controls not selected by this supplement. Both CPU and
GPU chunk the same corpus by the requested width. The unsupported combinations
are described explicitly in the output rather than counted as successful runs.

An independent scalar f64 forward oracle computes the expected MSE sum against
the generated targets. Absolute **mean-loss** error is bounded by 0.001; this
differs from v4's pointwise output-error metric, and the two studies are not
pooled. Device absence, another backend, preparation/readback faults, numerical
failures and budget truncation are failures or unavailable outcomes, never CPU
fallbacks or speed wins. The CLI preserves the completed cell distributions and
failure reasons and exits non-zero unless complete. A failed cell has no timing
verdict; samples from a cell that aborts during a scan are discarded.

Within each paired round, CPU/GPU order is shuffled. Warm scans include transfer,
dispatch, synchronisation, readback and reduction. End-to-end samples also include
owned typed-parts construction and, on GPU, scorer serialisation, buffer upload,
pipeline construction and scratch initialisation. Device creation is measured
once separately and reused. First GPU scan and preparation are reported apart.
Accuracy checks are outside timing. Both warm and end-to-end CI upper bounds
must be at most the unchanged 0.90 threshold to label a complete cell a joint
win; this is supplemental evidence, not a new adoption or dispatch rule.

The existing OS-resource wrapper records build and run RSS. Compiler, CPU,
kernel, source blobs, lockfile and commands are saved. Device-only allocation
peaks are not exposed by the public scorer API and remain unmeasured. Shared
host/virtual GPU timings are indicative; they cannot establish physical Apple
GPU performance or a population-level gain. After representative evidence,
retire this optional adapter if it earns no adoption or reusable capability.

```bash
bash experiments/sparse_metal/run.sh < /dev/null
```
