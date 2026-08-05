## Summary

`neat-core/benches/BASELINE.md` described the `TrainingDataset::evaluate_mse`
lane in the present tense ("It now bounds-checks once…"), even though the whole
`wasm_dataset` module — `TrainingDataset`, `evaluate_mse`, `input_batch` and the
`dataset_evaluate_mse` bench group — was removed as unconsumed dead code in
Issue #415. The removal note sat twenty lines further down, attached to the
results table, so a reader hit the present-tense claim first. Closes #506.

Changes to the #386 *Flat-slice record input for batched scoring* section:

1. The Issue #415 removal note is now a single blockquote placed **immediately
   before** the `evaluate_mse` paragraph — the first mention of any removed
   symbol in the file is inside that note.
2. The paragraph is rewritten in past tense: "The biggest win at the time was …
   #386 changed it to bounds-check once, take `input_batch(start, count)` as a
   single slice, and drive it through the flat batched path…". No sentence
   claims current behaviour for a removed symbol.
3. The old note beside the table is reduced to a back-reference ("the
   since-removed group — see the Issue #415 note above") so there is exactly one
   removal note.

The `dataset_evaluate_mse` results table and the drift-control table are
unchanged — they are the explicitly retained historical record.

## Evidence

Docs-only prose edit; no code path, test, or CI gate consumes this file, so
there is no web interface to screenshot and no behaviour to test.

Verification performed:

- `./quality.sh < /dev/null` — passed cleanly (fmt, clippy, deny, workspace
  tests, doc build, release build).
- `grep -n "evaluate_mse\|TrainingDataset\|input_batch\|wasm_dataset"
  neat-core/benches/BASELINE.md` — the first hit is line 279, inside the
  historical blockquote.
- `git diff` confirms both measurement tables are byte-identical.

## Test Plan

No tests added or modified — the change is documentation prose with no
executable surface. The existing suite was run via `./quality.sh` to confirm the
edit introduces no regressions.
