# PR Summary — Issue #329

## Summary

`.github/workflows/markdown-lint.yml` gated pull requests with
`branches: ["*"]`. GitHub branch-filter globs treat `*` as "any characters
except `/`", so that pattern never matched a `milestone/<slug>` branch. Every
milestone sub-issue PR merged into the shared milestone branch without the
Markdown lint gate running — the gap only surfaced later on the single rollup PR
into the default branch.

Added an explicit `milestone/*` pattern to the filter
(`branches: ["*", "milestone/*"]`), matching the fixes already landed for
actionlint (#326), the CI quality workflow (#327), and gitleaks (#328).
Milestone branch names are `milestone/<slug>` with no nested slashes, so the
single-level glob is sufficient.

Closes #329.

## Evidence

Backend/CI-configuration change — no web interface to screenshot. Verified by
the bats suite below.

Branch-filter coverage before and after:

```mermaid
flowchart LR
    subgraph Before["Before — branches: [\"*\"]"]
        B1["PR → Develop"] --> BG["markdown lint runs"]
        B2["PR → milestone/clean-up-23-jul"] -.->|no pattern matches| BS["gate skipped"]
    end
    subgraph After["After — branches: [\"*\", \"milestone/*\"]"]
        A1["PR → Develop"] --> AG["markdown lint runs"]
        A2["PR → milestone/clean-up-23-jul"] --> AG
    end
```

Full `./quality.sh` run passes (`✅ All quality checks passed!`), and the
targeted suite is green:

```text
1..12
ok 4 markdown-lint workflow pull_request filter matches milestone branches
...
```

The new test fails against the unfixed workflow (confirmed before applying the
fix) and passes after it.

## Test Plan

- Added `tests/scripts/markdown_lint_workflow.bats::"markdown-lint workflow
  pull_request filter matches milestone branches"` — parses the workflow YAML,
  reimplements GitHub's filter globbing (`*` does not cross `/`, `**` does), and
  asserts that at least one pattern matches `milestone/clean-up-23-jul` while
  `Develop` and `main` still match. This is the regression test for #329.
- Re-ran the whole `tests/scripts/markdown_lint_workflow.bats` suite (12/12
  pass) to confirm no existing behaviour regressed.
- Ran `./quality.sh < /dev/null` — all checks pass.
