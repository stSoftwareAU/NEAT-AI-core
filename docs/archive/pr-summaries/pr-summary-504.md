# BASELINE.md: `score_records` → `score_records_flat` (Issue #504)

## Summary

`neat-core/benches/BASELINE.md` named `score_records` in the present tense at
seven sites, but that per-record `&[Vec<f32>]` entry point was removed in Issue
#409 (`neat-core/src/parallel_scoring.rs:58-61`). The document already
contradicted itself — the wasm32 harness was described as calling
`score_records` in one place and `CompiledNetwork::score_records_flat` a few
hundred lines later in the same *Reproducing* area.

Six sites are renamed to `score_records_flat` (the sequential record-scoring
method that exists today) because they describe the path a reader would use now.
One site is genuinely historical and is left as `score_records` with an explicit
removal pointer. No recorded measurement number was touched — this is a
naming/tense correction, not a re-benchmark.

Closes #504.

| Line | Change |
| --- | --- |
| 106 | renamed → `score_records_flat` |
| 168 | renamed → `score_records_flat` |
| 180 | renamed → `score_records_flat` |
| 225 | kept historical: "the then-current per-record `score_records` entry point (removed in Issue #409; `score_records_flat` is its successor)" |
| 370 | renamed → `score_records_flat` |
| 394 | renamed → `score_records_flat` (now matches line 524) |
| 454 | renamed → `score_records_flat` |

Out of scope, handled separately by sibling sub-issues of #497:
`score_records_parallel` renames, the `evaluate_mse` section, the `scoring_flat`
section, and the `benches/README.md` table row.

## Evidence

Docs-only prose change — no web interface to screenshot and no runtime surface,
so correctness is verified by the acceptance grep from the issue:

```
$ grep -n 'score_records\b' neat-core/benches/BASELINE.md
225:locality win #287 landed for the then-current per-record `score_records` entry
```

The single remaining bare occurrence is past tense and carries the Issue #409
pointer on the following line. Every symbol the file now presents as current
exists in `neat-core/src`:

```
$ grep -rn 'pub fn score_records' neat-core/src/
neat-core/src/parallel_scoring.rs:96:    pub fn score_records_flat(
neat-core/src/parallel_scoring.rs:113:    pub fn score_records_flat_into(
neat-core/src/parallel_scoring.rs:157:    pub fn score_records_parallel_flat(
neat-core/src/parallel_scoring.rs:193:    pub fn score_records_parallel_flat(
```

Line 394 and line 524 now name the same symbol (`score_records_flat`), removing
the internal contradiction.

`./quality.sh < /dev/null` passes cleanly (fmt, clippy, TypeScript gate, Mermaid
gate, `cargo test --workspace`, doc build, release build).

## Test Plan

No tests added or modified. The change is documentation prose in
`neat-core/benches/BASELINE.md`; per AGENTS.md, source-grep assertions are "how"
tests and are not written here. The existing workspace test suite was run
unchanged via `./quality.sh` to confirm nothing regressed.
