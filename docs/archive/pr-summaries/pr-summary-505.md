# PR Summary — Issue #505

## Summary

`neat-core/benches/BASELINE.md` named `CompiledNetwork::score_records_parallel`
as the shipped native rayon lane in four places. That entry point was removed in
Issue #409; the surviving native lane is
`CompiledNetwork::score_records_parallel_flat`
(`neat-core/src/parallel_scoring.rs:157`, with the sequential/wasm32 fallback at
line 193). The worst offender was the **Issue #288 verdict** paragraph, which
told readers the core-side path "is ready" while naming a symbol that no longer
compiles.

Renamed all four references to `score_records_parallel_flat`, matching the
spelling already used in the #386 section. The one surviving occurrence of the
old name is the historical framing, which now explicitly marks it as the
pre-#409 name. No recorded measurement numbers were changed — the measurements
remain valid for the same kernel; only the entry-point name changed.

Docs-only change to prose in one file. Closes #505.

| Line | Change |
| --- | --- |
| ~350 | historical framing — names `score_records_parallel_flat` today, notes `score_records_parallel` as the pre-#409 name |
| ~360 | #288 verdict → `score_records_parallel_flat` |
| ~381 | "fixed-size rayon pool … via" → `score_records_parallel_flat` |
| ~435 | idle-core tail win → `score_records_parallel_flat` |

## Evidence

No web interface to screenshot — this is a documentation-only change.

Acceptance grep from the issue (only the explicitly-marked historical mention
remains):

```
$ grep -n 'score_records_parallel\b' neat-core/benches/BASELINE.md
352:`score_records_parallel` before Issue #409 removed it — but it was never A/B'd
```

Every symbol the #288 verdict now names resolves in the source:

```
$ grep -n 'fn score_records_parallel_flat' neat-core/src/parallel_scoring.rs
157:    pub fn score_records_parallel_flat(
193:    pub fn score_records_parallel_flat(
```

`./quality.sh < /dev/null` passes cleanly (fmt, clippy, deny, full test suite,
doc build, release build).

## Test Plan

No tests added or modified. Per the issue's own Failure Detection section this
is prose in a benchmark note — no code, test, or CI behaviour depends on the
symbol names in this file. A test asserting the file's text would be a source-
grep "how" test, which `AGENTS.md` explicitly discourages. Verification is the
acceptance grep above plus the unchanged green `./quality.sh` run.
