## Summary

`.github/workflows/markdown-lint.yml` triggered on both `pull_request` and `push`
to `Develop`. As a lint/check gate it already runs on the PR, so the post-merge
push run was a duplicate — burning CI minutes and able to leave a red tick on the
default branch for a check that already passed. Dropped the `push:` trigger,
keeping `pull_request` and adding `workflow_dispatch` for manual re-runs. This
matches the PR-only checkers `actionlint.yml` (Issue #314), `gitleaks.yml` and
`semgrep.yml`. Deploy/publish workflows (`release.yml`, `wasm-bundle.yml`) and the
main `ci.yml` build are untouched — they must keep firing on push. Closes #316.

## Evidence

Backend/CI-config change — no web interface to screenshot. Verified via the
existing BATS suite (`bats tests/scripts/markdown_lint_workflow.bats`, 10/10
passing, including the local `markdownlint-cli2` run over the current tree) and a
clean `./quality.sh`.

```mermaid
flowchart LR
    subgraph before["Before"]
        P1[PR opened] --> A1[markdownlint run]
        M1[merge → push Develop] --> A2[markdownlint run again — duplicate]
    end
    subgraph after["After"]
        P2[PR opened] --> A3[markdownlint run]
        M2[merge → push Develop] --> N2[no run]
        D2[workflow_dispatch] --> A4[markdownlint run]
    end
```

## Test Plan

- Modified `tests/scripts/markdown_lint_workflow.bats::"markdown-lint workflow
  gates PRs only and does not re-run on push to Develop"` (previously
  *"...triggers on PRs and on pushes to Develop"*). **Documented business-logic
  change:** the old assertion required `Develop` in the `push.branches` list,
  which is exactly the behaviour this issue removes; the test now asserts
  `pull_request` is still a trigger and that `Develop` is absent from any
  `push.branches` filter. It failed against the unfixed workflow and passes after
  the change. No tests were removed or commented out.
- All 10 tests in `tests/scripts/markdown_lint_workflow.bats` pass.
- `./quality.sh` passes cleanly (shellcheck, bats, TypeScript check, fmt, clippy,
  deny, workspace tests, doc, release build).
