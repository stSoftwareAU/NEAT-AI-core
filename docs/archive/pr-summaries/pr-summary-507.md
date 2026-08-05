## Summary

`neat-core/benches/BASELINE.md` introduced `scoring_flat` as "the new group",
implying it could still be run. It cannot: Issue #408 retired the A/B group and
Issue #409 deleted the per-record `score_records` / `score_records_parallel`
entry point it was compared against. The passage (and the `scoring`-as-
no-regression-gate paragraph immediately above it, which described the same
removed `&[Vec<f32>]` wrapper) is now framed as history, with the measured
numbers kept as the evidence for the #386 flat-input win and clearly labelled as
pre-retirement. Closes #507.

Changes to `neat-core/benches/BASELINE.md` only:

- The `scoring` no-regression-gate paragraph moved to past tense and notes that
  #409 deleted the `&[Vec<f32>]` wrapper it gated; `scoring` now drives
  `score_records_flat` directly.
- The `scoring_flat` paragraph became a **Historical A/B** blockquote citing
  #408 (group retired) and #409 (entry point removed), stating the Criterion
  filter has matched no benchmark since #408.
- The results table is retained, its header marked
  `` `scoring_flat` — retired by #408, entry point removed by #409 ``.
- Cross-references `neat-core/benches/README.md` ("The `scoring` group (Issue
  #228)") and the `bench_scoring` doc comment in
  `neat-core/benches/hot_paths.rs`, so all three documents agree.

## Evidence

Docs-only change — no code, benchmark, or CI behaviour changes, so there is no
web interface to screenshot and no benchmark to re-run. Verified against the
issue's acceptance criteria:

```
$ grep -n 'scoring_flat' neat-core/benches/BASELINE.md
332:> **Historical A/B — `scoring_flat` was retired by Issue #408, and the
347:| benchmark (mean of rounds, 2026-07-26) | `scoring` | `scoring_flat` — retired by #408, entry point removed by #409 | change |
```

Both remaining occurrences are marked as retired and cite Issue #408 (and #409
for the entry-point removal); no text implies the group can be run today.

`./quality.sh` passes cleanly (markdownlint, Mermaid gate, fmt, clippy, deny,
`cargo test --workspace`, docs, release build).

## Test Plan

No automated tests added — this is a prose-only edit to a benchmark baseline
document with no executable surface. Verification is the acceptance grep above
plus the full `./quality.sh` gate, which includes the repo's markdownlint pass
over `**/*.md`.
