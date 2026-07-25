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

- `grep -n "GRQ" README.md` now returns no matches (it previously matched lines
  44 and 49).
- `./quality.sh < /dev/null` passes cleanly (exit 0): shellcheck, `deno check`,
  codespell, `cargo deny`, `cargo fmt`, `cargo clippy -D warnings`,
  `cargo check`, `cargo test --workspace`, `cargo doc` and the release build.
- `git status` shows `README.md` as the only modified file — no dependency or
  lockfile churn.

## Test Plan

No tests were added or modified. The change is prose-only, and this repo's
`AGENTS.md` explicitly discourages source-grep assertions as tests ("How tests
tie to implementation detail and are discouraged: … source greps"), so a test
that greps `README.md` for a banned string would violate the repo's own testing
rule. The existing suite was run unchanged as a regression check:

- `cargo test --workspace --lib --tests --all-features` — all tests pass.
- `./scripts/typescript-check.sh` — TypeScript sources still valid.
