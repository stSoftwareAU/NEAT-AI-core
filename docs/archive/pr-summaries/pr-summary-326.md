## Summary

The `actionlint.yml` CI quality workflow gated pull requests with a
`pull_request.branches` filter of `["*"]`. GitHub branch-filter globbing treats
`*` as "any characters except `/`", so the pattern never matched
`milestone/<slug>` feature branches. Milestone sub-issue PRs — which target a
shared `milestone/<name>` branch under the planning delivery workflow — merged
without the actionlint gate ever running; the gap was only caught later by the
single rollup PR into the default branch.

Added an explicit `milestone/*` pattern to the filter so the lint gate runs on
milestone PRs too, while the existing `*` keeps matching `Develop`, `main`, and
other single-segment branches. Milestone branch names are `milestone/<slug>`
with no nested slashes, so the single-level `milestone/*` glob is sufficient.

Closes #326.

## Evidence

Backend/CI-config change — no web interface to screenshot. Verified via the
behavioural bats test below, which reimplements GitHub's filter globbing (`**`
crosses `/`, `*` does not) and asserts the configured patterns match a real
milestone branch name while still matching the default branches.

```mermaid
flowchart LR
    A["milestone/clean-up-23-jul PR"] --> B{"branches filter"}
    B -- "before: [\"*\"] — * stops at /" --> C["gate SKIPPED ✗"]
    B -- "after: [\"*\", \"milestone/*\"]" --> D["actionlint runs ✓"]
```

Test run:

```
ok 4 actionlint workflow pull_request filter matches milestone branches
ok 9 actionlint passes against the current workflows on disk
```

## Test Plan

- Added `actionlint workflow pull_request filter matches milestone branches` to
  `tests/scripts/actionlint_workflow.bats`. It parses the workflow's
  `pull_request.branches` filter and, using GitHub's glob semantics, asserts a
  milestone branch (`milestone/clean-up-23-jul`) matches a pattern and that
  `Develop`/`main` still match. This test fails against the old `["*"]` filter
  and passes after the fix.
- Full `tests/scripts/actionlint_workflow.bats` suite: 9 passing.
- `./quality.sh` passes cleanly (Rust workspace tests, docs, release build).
