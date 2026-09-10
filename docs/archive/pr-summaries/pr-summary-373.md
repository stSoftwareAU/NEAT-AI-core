## Summary

Reworded the private-repo references in the `README.md` wasm64 lane (d)
paragraph to concept level. NEAT-AI-core is public, so the front page must be
self-contained for public readers — it no longer names the private downstream
repository or its internal script paths. Closes #373.

Wording changes (README.md, lane (d) paragraph):

| Before | After |
| --- | --- |
| "verifies the …-side adoption end-to-end", naming the private downstream repository | "verifies the downstream adoption end-to-end" |
| the private downstream repository's "`worker/learn.sh` injects …" | "the production trainer's launch script injects …" |
| "sized by `memory_calc.sh`" | "sized by its memory-budget helper" |
| "re-derives that selection lock-step with" the named private repository | "re-derives that selection lock-step with the production selector" |

The technical content is unchanged: the RAM-aware
`--v8-flags=--max-old-space-size` sizing, the safe floor below the 8 GB tier,
the fail-loud exit-133 behaviour, and the acceptance model
`tests/perf/learn_flags_wiring.ts` are all still described.

**Deliberately out of scope:** the link to the lane (d) learn-wiring research
doc under `docs/research/` is left as-is. That file's name still embedded the
private repository's name at the time; the issue notes the doc is covered by a
separate finding, and the link text is that file's path, so it can only change
once the doc itself is reworded or renamed — changing it now would break the
link.

## Evidence

Documentation-only change — no web interface to screenshot and no runtime
behaviour altered, so no Playwright evidence applies.

- A word-boundary grep for the private repository's name in `README.md` now
  returns no matches (it previously matched the lane (d) paragraph).
- `./quality.sh < /dev/null` passes cleanly (exit 0): shellcheck, `deno check`,
  codespell, `cargo deny`, `cargo fmt`, `cargo clippy -D warnings`,
  `cargo check`, `cargo test --workspace`, `cargo doc` and the release build.

## Test Plan

Added `tests/scripts/readme_private_repo_reference.bats` — a regression guard in
the same artefact-content "what" test style as the existing
`tests/scripts/readme_glossary.bats` (it asserts on the observable content of the
published README, not on source implementation detail, so it stays within
`AGENTS.md`'s testing rule). A milestone merge had already re-introduced the
private repository's name into this paragraph once, so the guard has
demonstrated value.

- The private-name token test — greps for the private repository's name on word
  boundaries and asserts it is absent. Fails against the pre-fix README, passes
  after the reword.
- The internal-scripts test — asserts `worker/learn.sh` and `memory_calc.sh` are
  absent from the README.
- The repository-path test — asserts the private repository's full `owner/repo`
  path is absent.
- Existing suite run unchanged as a regression check: `cargo test --workspace`
  and `./scripts/typescript-check.sh` both pass.
