## Summary

Adds GitHub Actions dependency cache for Deno across four workflows. `actions/cache` steps now cache `~/.cache/deno` (Deno's default `DENO_DIR`) immediately after each of 5 instances of `denoland/setup-deno` across `ci.yml`, `deno-outdated.yml`, `markdown-lint.yml`, and `wasm-bundle.yml`. Cache keys are hash-based on `deno.lock` to ensure stale caches don't serve outdated modules. Closes #737.

## Evidence

- **Test-driven validation:** Created `tests/scripts/workflow_deno_cache.bats` with 7 test cases:
  - Real-workflow sweep confirms exactly 5 cache job sites with 0 violations.
  - Synthetic pass/fail tests validate correct cache step placement (must come immediately after `setup-deno`).
  - Key format, path, and restore-keys fallback validation.
  - All tests pass against the live workflows.

- **Cache step details (all 5 sites identical template):**
  - Path: `~/.cache/deno`
  - Primary key: `${{ runner.os }}-deno-${{ hashFiles('deno.lock') }}`
  - Restore-keys fallback: `${{ runner.os }}-deno-`
  - SHA pinned to `actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9` (v6.1.0) with version comment

- **Regression validation:** All workflow-related test suites pass:
  - `workflow_sha_pinning.bats` (4/4 pass) — SHA pin format validated
  - `workflow_container_pinning.bats` (4/4 pass) — container pins unchanged
  - `actionlint_workflow.bats` (8/8 pass) — YAML syntax valid
  - `ci_workflow.bats` (4/4 pass), `deno_outdated_workflow.bats` (10/10 pass), `markdown_lint_workflow.bats` (17/17 pass), `ci_workflow_quarantine.bats` (8/8 pass)
  - All existing gates pass; no workflow regressions.

- **Quality gate:** `./quality.sh < /dev/null` passes with 500+ Rust tests, all doctests, doc build, and release build green.

## Test Plan

- [x] Created test-driven validation framework for Deno cache placement
- [x] Verified all 5 cache steps correctly positioned (after setup-deno)
- [x] Verified cache key uses `hashFiles('deno.lock')` to invalidate on dependency changes
- [x] Verified all 5 cache steps use identical template with correct SHA pin
- [x] Ran all workflow-related regression test suites (no gates broken)
- [x] Ran full `./quality.sh` gate (all checks pass)
- [x] Confirmed no secrets staged and run-id trailer on commits
