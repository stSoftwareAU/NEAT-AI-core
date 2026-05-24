## Summary
Completed the Cargo Security Audit workflow by adding the `rustsec/audit-check@v2` action to `.github/workflows/security.yml`. The reusable security workflow now runs the official RustSec action (which annotates PRs with advisories) as the primary audit, with the existing manual `cargo audit` run retained as a fallback that still fails the job if the action cannot post annotations (e.g. forked PR permission edge cases). Closes #5.

## Evidence
Backend/CI-only change — no UI. The workflow change was verified by diff review:

- The three detection patterns called out by VibeCoding workflow sync are now all present in `.github/workflows/security.yml`:
  - `cargo audit` — fallback step and comment references
  - `cargo-audit` — `taiki-e/install-action` `tool:` entry plus the step name
  - `rustsec/audit-check` — new `uses: rustsec/audit-check@v2` step

YAML structure was eyeballed against the existing (valid) workflow; no structural changes were made beyond inserting a new step before the fallback.

## Test Plan
- [x] `.github/workflows/security.yml` contains `rustsec/audit-check@v2` as a step.
- [x] The existing `cargo audit` invocation is retained so the job still fails on vulnerabilities even if the action cannot post annotations.
- [x] `cargo-audit` is still installed via `taiki-e/install-action@v2`.
- [ ] On the first PR after merge, confirm the new step reports green and that RustSec annotations appear on the PR if any advisories match.
