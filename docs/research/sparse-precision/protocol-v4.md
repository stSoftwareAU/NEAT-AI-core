# Sparse precision protocol v4 — frozen before validation

Archived protocol: the executable experiment and manual CI job below belong to
replay revision `17d222d`, archived as [replay-cpu-v4.patch](replay-cpu-v4.patch).
They were retired from the current source tree after the three CPU comparisons.

Part of issue #758. The factors, sample counts, limits, tolerances and adoption
rule in [protocol v2](protocol-v2.md) remain unchanged. V1–v3 evidence stays
separate. Validation network seed 32002 and record seed 33003 are frozen in
[manifest-v4.json](manifest-v4.json); calibration remains at seed 1001. Commit
this protocol and source before validation. No search or threshold tuning occurs.

## Additional memory accounting

The timing executable has no allocation instrumentation. A separate
`sparse_precision_memory` executable calls the same conversion and execution
module, using the same validation networks, records and widths. It measures one
conversion, first initialisation/scan and compile/convert/scan per variant.
Every planned variant is probed, including numerical failures; this memory
coverage is distinct from timed coverage, which excludes failed variants.

A forwarding System allocator records requested live heap bytes before and after
each scope, peak live bytes and the additional peak above the starting live
bytes. Conversion includes temporary row/triple vectors and the returned layout;
the canonical parent stays live. These counts include Vec capacities but exclude
allocator metadata, allocator-internal overlap during realloc, stacks, mappings
and resident-set effects. The probe drops the warmed variant before measuring
the fresh end-to-end path. No allocation thresholds affect adoption.

The Deno driver runs build, timing and memory commands sequentially. A small
Python standard-library `resource.getrusage(RUSAGE_CHILDREN)` wrapper records
each child's process high-water RSS, normalising Linux KiB and macOS bytes. This
is a process high-water mark, including inherited pages around process creation,
not an incremental layout footprint or the simultaneous RSS sum of a build's
children. Raw values, commands, exit codes, logs and elapsed times are saved.
Each subprocess has a 900-second external cap; the existing manifest's softer
paired-round budgets remain in force. A numerical failure saves both timing and
memory evidence and exits non-zero. An unexpected command failure also fails.

## Platform execution

The existing CI workflow accepts the manual boolean input `sparse_precision`,
default false. Its independent research job runs on `macos-15` (Apple Silicon)
and `ubuntu-24.04` (x86_64), with fail-fast disabled and artefact upload even
after a failed measurement. Ordinary pull requests do not run the matrix.
[GitHub's runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
documents those architectures; actual CPU, OS and compiler identities in each
result take precedence over a runner label. Timings on hosted/shared machines
remain indicative. A local aarch64 Linux execution completes the CPU comparison.

The scorer's existing f32 wgpu loss API is not a layout/precision forward API.
No GPU kernel is copied here. Metal factorial cells remain unavailable unless a
compatible adapter can be exercised; software bf16/f16 storage results do not
claim hardware reduced-precision arithmetic. Missing Metal results are recorded
explicitly, never inferred from CPU results.

## Reproduce

```bash
deno run --allow-read --allow-write --allow-run --allow-env \
  scripts/run_sparse_precision.ts \
  docs/research/sparse-precision/manifest-v4.json <new-output-directory>
```

The output directory must not exist. Source and manifest must be committed.
The wrapper needs Python 3 for OS resource accounting; Deno owns orchestration
because its subprocess API does not expose child resource usage.

```bash
gh workflow run ci.yml --ref <frozen-commit-or-branch> -f sparse_precision=true
```
