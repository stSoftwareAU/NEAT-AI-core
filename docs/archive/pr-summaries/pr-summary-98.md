## Summary

Bumped `actions/setup-node` in `.github/workflows/markdown-lint.yml` from the
deprecated Node 20 release (`@49933ea5288caeca8642d1e84afbd3f7d6820020`, v4.0.2)
to the current `v6.4.0` (`@48b55a011bda9f5d6aeb4c2d9c7362e8dae4041e`), which
runs on Node 24. This clears the `Node.js 20 actions are deprecated` warning
flagged on recent Markdown Lint runs and protects the workflow from the
scheduled Node 20 → Node 24 forced upgrade (2026-06-02) and full removal
(2026-09-16). Closes #98.

The new SHA is the annotated-tag target for `v6.4.0`, published 2026-04-20 —
well outside the 24h supply-chain quarantine window. The neighbouring version
comment (`# actions/setup-node@v6.4.0`) is updated alongside the SHA so the
existing "every SHA-pinned action has a version comment" gate keeps passing.

## Evidence

This is a CI workflow change with no UI surface, so the evidence is the
bats regression test plus the targeted `bats` runs below.

Targeted bats output (`bats tests/scripts/markdown_lint_workflow.bats`):

```
ok 1 markdown-lint workflow file exists
ok 2 markdown-lint workflow is valid YAML
ok 3 markdown-lint workflow triggers on PRs and on pushes to Develop
ok 4 markdown-lint workflow exposes a markdownlint job that runs markdownlint-cli2
ok 5 markdown-lint workflow pins third-party actions to commit SHAs
ok 6 markdown-lint workflow does not pin actions/setup-node to a deprecated Node 20 SHA
ok 7 markdown-lint workflow gates Mermaid validation on a Deno worker module
ok 8 markdownlint config file exists and is valid JSONC
ok 9 markdownlint-cli2 passes against the current tree
ok 10 markdownlint-cli2 rejects a known-bad Markdown file
```

Flow before/after:

```mermaid
flowchart LR
    A[markdown-lint.yml] --> B[setup-node@v4 / Node 20]
    B --> C[Deprecation warning on runs]
    A2[markdown-lint.yml] --> B2[setup-node@v6.4.0 / Node 24]
    B2 --> C2[No deprecation warning]
```

## Test Plan

- Added `markdown-lint workflow does not pin actions/setup-node to a deprecated Node 20 SHA`
  in `tests/scripts/markdown_lint_workflow.bats`. The test enumerates the known
  deprecated SHAs (the previous `v4` Node 20 SHA and the older `v3` Node 16 SHA)
  and fails if the workflow regresses to either. It targets observable
  workflow state (the parsed `uses:` ref), not source-text patterns.
- The existing tests in the same file (YAML validity, SHA pinning, version
  comments, Mermaid gate) continue to pass against the bumped SHA.

## Out-of-scope pre-existing failures

`./quality.sh` reports failures that pre-date this change and are unrelated
to issue #98 (verified by re-running bats against `HEAD` before applying any
edits):

- `ci.yml version-increment job invokes bump-deps.sh` and related
  quarantine checks in `tests/scripts/ci_workflow_quarantine.bats`.
- `every action in every workflow is pinned to a 40-char commit SHA` —
  `upgrade-dependencies.yml` still uses `taiki-e/install-action@v2`.

These are tracked separately under the same `github-actions-audit` bucket
and are out of scope for the setup-node bump.
