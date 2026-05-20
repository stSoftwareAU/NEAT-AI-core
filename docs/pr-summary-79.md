# Script-injection hardening for `github.head_ref` in CI workflows

## Summary

Closes #79. Eliminated direct interpolation of `${{ github.head_ref }}` into
every `run:` shell block under `.github/workflows/`. Each affected step now
binds `github.head_ref` to a step-level `env:` variable (`HEAD_REF`) and the
shell references it via `"$HEAD_REF"`, so any shell metacharacters smuggled
into a branch name are inert parameter-expansion text rather than template
substitution that the shell would evaluate.

The fix follows the GitHub Security Lab guidance on untrusted input in
GitHub Actions and matches the pattern recommended in the issue.

## Affected call sites

| File | Step | Before | After |
| --- | --- | --- | --- |
| `.github/workflows/ci.yml` | `version-increment` → Pull latest changes | `git pull origin ${{ github.head_ref }}` | `env: HEAD_REF`, `git pull origin "$HEAD_REF"` |
| `.github/workflows/ci.yml` | `version-increment` → Commit version and dependency changes | `git push origin ${{ github.head_ref }}` | `env: HEAD_REF`, `git push origin "$HEAD_REF"` |
| `.github/workflows/ci.yml` | `auto-format` → Commit rustfmt if needed | `git push origin ${{ github.head_ref }}` | `env: HEAD_REF`, `git push origin "$HEAD_REF"` |
| `.github/workflows/ci.yml` | `quality` → Pull latest changes | `git pull origin ${{ github.head_ref }}` | `env: HEAD_REF`, `git pull origin "$HEAD_REF"` |
| `.github/workflows/security.yml` | `security` → Pull latest | `git pull --ff-only origin "${{ github.head_ref }}" \|\| true` | `env: HEAD_REF`, `git pull --ff-only origin "$HEAD_REF" \|\| true` |

Remaining `${{ github.head_ref }}` references inside `with:` blocks of
`actions/checkout` are passed as action inputs (not shell), so they are
safe and were left in place.

## Evidence

Added a regression bats test
(`tests/scripts/workflow_script_injection.bats`) that parses every workflow
under `.github/workflows/` and fails if any `run:` body matches a tainted
context expression (`github.head_ref`, `github.event.pull_request.*`,
`github.event.issue.*`, `github.event.comment.body`,
`github.event.review.body`, `github.event.commits.*`,
`github.event.workflow_run.head_branch`). A second test pins the
detector regex against known-bad and known-safe snippets so a future
weakening of the detector is caught.

Before the fix, `bats tests/scripts/workflow_script_injection.bats`
reported:

```text
not ok 1 no run: block interpolates tainted github contexts directly
```

After the fix:

```text
ok 1 no run: block interpolates tainted github contexts directly
ok 2 tainted-context detector regex catches known-bad patterns
```

```mermaid
flowchart LR
    A[PR branch name<br/>'evil$(curl ...|bash)'] --> B{run: body}
    B -->|Before: template substitution| C[Shell evaluates<br/>command substitution<br/>= RCE on runner]
    B -->|After: env-var binding| D["$HEAD_REF<br/>= literal string<br/>= safe"]
```

## Test plan

- New: `tests/scripts/workflow_script_injection.bats` — both tests pass.
- Existing bats suites (`workflow_sha_pinning.bats`,
  `ci_workflow_quarantine.bats`, etc.) were unaffected; the failures
  visible in those suites pre-date this change (verified by stashing the
  PR diff and re-running the suite).
- `python3` YAML parse of both modified workflow files succeeds.
- `shellcheck` and `codespell` clean on the new bats file and the
  modified workflows.
