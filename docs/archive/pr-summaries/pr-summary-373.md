## Summary

Reworded the private-repo references in the `README.md` wasm64 lane (d)
paragraph to concept level. NEAT-AI-core is public, so the front page must be
self-contained for public readers — it no longer names the private downstream
repository or its internal script paths. Closes #373.

Wording changes (README.md, lane (d) paragraph):

| Before | After |
| --- | --- |
| "verifies the GRQ-side adoption end-to-end" | "verifies the downstream adoption end-to-end" |
| "GRQ's `worker/learn.sh` injects …" | "the production trainer's launch script injects …" |
| "sized by `memory_calc.sh`" | "sized by its memory-budget helper" |
| "re-derives that selection lock-step with GRQ" | "re-derives that selection lock-step with the production selector" |

The technical content is unchanged: the RAM-aware
`--v8-flags=--max-old-space-size` sizing, the safe floor below the 8 GB tier,
the fail-loud exit-133 behaviour, and the acceptance model
`tests/perf/learn_flags_wiring.ts` are all still described.

**Deliberately out of scope:** the link to
`docs/research/wasm64-lane-d-grq-learn-wiring-verification.md` is left as-is.
The issue notes that doc is covered by a separate finding; the link text is that
file's path, so it can only change once the doc itself is reworded or renamed —
changing it now would break the link.

## Evidence

Documentation-only change — no web interface to screenshot and no runtime
behaviour altered, so no Playwright evidence applies.

- `grep -nE "\bGRQ\b" README.md` now returns no matches (it previously matched
  the lane (d) paragraph).
- `./quality.sh < /dev/null` passes cleanly (exit 0): shellcheck, `deno check`,
  codespell, `cargo deny`, `cargo fmt`, `cargo clippy -D warnings`,
  `cargo check`, `cargo test --workspace`, `cargo doc` and the release build.

## Test Plan

Added `tests/scripts/readme_private_repo_reference.bats` — a regression guard in
the same artefact-content "what" test style as the existing
`tests/scripts/readme_glossary.bats` (it asserts on the observable content of the
published README, not on source implementation detail, so it stays within
`AGENTS.md`'s testing rule). A milestone merge had already re-introduced the
private `GRQ` name into this paragraph once, so the guard has demonstrated value.

- `README.md does not name the private repository (GRQ token)` — greps for the
  `\bGRQ\b` token and asserts it is absent. Fails against the pre-fix README,
  passes after the reword.
- `README.md does not reference the private trainer's internal scripts` —
  asserts `worker/learn.sh` and `memory_calc.sh` are absent from the README.
- Existing suite run unchanged as a regression check: `cargo test --workspace`
  and `./scripts/typescript-check.sh` both pass.
