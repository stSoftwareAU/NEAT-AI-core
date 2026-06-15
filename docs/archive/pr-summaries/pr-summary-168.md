# PR Summary — Issue #168

## Summary

The scheduled `Upgrade Cargo Dependencies` workflow authenticated the
`peter-evans/create-pull-request` step with `GITHUB_TOKEN` directly. GitHub
suppresses downstream workflow triggers (CI checks, labels, reviewer
automation) on a PR created with `GITHUB_TOKEN`, so those never fire until
somebody pushes a new commit.

This change makes the step authenticate with the org-level PAT
(`ACTIONS_PUSH`) and fall back to `GITHUB_TOKEN` only when the secret is unset
(Issue #1636), matching how `ci.yml` already pushes to PR branches.

Closes #168.

## Change

`.github/workflows/upgrade-dependencies.yml`, create-pull-request step:

```yaml
- uses: peter-evans/create-pull-request@22a9089034f40e5a961c8808d113e2c98fb63676
  with:
    token: ${{ secrets.ACTIONS_PUSH || secrets.GITHUB_TOKEN }}
```

## Evidence

Backend/CI-only change — no web interface to screenshot. Verified via a new
bats regression suite that asserts on the workflow YAML.

```mermaid
flowchart LR
    A[Scheduled bump job] --> B[create-pull-request]
    B -->|token: ACTIONS_PUSH PAT| C[New PR]
    C --> D[Downstream workflows fire:<br/>CI, labels, reviewer automation]
    B -.->|"old: bare GITHUB_TOKEN"| E[New PR]
    E -.->|triggers suppressed| F[No CI until manual push]
```

## Test Plan

Added `tests/scripts/pr_creator_token.bats` (regression for #168):

- `upgrade-dependencies.yml uses create-pull-request action` — sanity guard.
- `create-pull-request authenticates with ACTIONS_PUSH and GITHUB_TOKEN
  fallback` — asserts the fallback token expression is present.
- `create-pull-request step does not authenticate with GITHUB_TOKEN alone` —
  inspects the create-pull-request step's `token:` line and fails on a bare
  `GITHUB_TOKEN`.

All three tests fail against the unfixed workflow and pass after the fix.

### Pre-existing, out-of-scope failures

`tests/scripts/ci_workflow_quarantine.bats` reports four failures about
`ci.yml` (bump-deps.sh / quarantine knob). These pre-date this change, concern
a different workflow file, and are unrelated to issue #168 — left untouched.
