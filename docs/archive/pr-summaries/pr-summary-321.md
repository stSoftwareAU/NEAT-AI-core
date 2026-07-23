## Summary

Hardened the `gitleaks` CI job so the workflow's `GITHUB_TOKEN` is no longer
persisted to disk. The `actions/checkout` step now sets
`persist-credentials: false`, preventing the token from being written into
`.git/config` as an auth header where a later step (or a compromised
dependency) could read it and act as the token. The `gitleaks` job only scans
the checked-out tree — it never pushes back to the repository or fetches
private submodules — so the persisted credential is unnecessary and only
widens the blast radius of a compromised step.

Closes #321.

## Change

`.github/workflows/gitleaks.yml` — added `persist-credentials: false` to the
checkout step (alongside the existing `fetch-depth: 0`, which gitleaks needs
for full-history scanning):

```yaml
      - uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd
        with:
          fetch-depth: 0
          persist-credentials: false
```

```mermaid
flowchart LR
    A[checkout] -->|persist-credentials: false| B[no token in .git/config]
    B --> C[gitleaks detect: read-only scan]
    C --> D[no credential exposed to later steps]
```

## Evidence

Backend/CI-only change — no web interface to screenshot. The affected job is a
security scanner that reads the working tree; the change removes a persisted
credential and does not alter scan behaviour (`fetch-depth: 0` is retained so
gitleaks still scans full history).

Verification:

- `./quality.sh < /dev/null` → `✅ All quality checks passed!` (full Rust
  workspace: fmt, clippy, deny, tests, doc, release build).
- YAML remains valid; the only downstream consumers of the checkout are the
  gitleaks install and `gitleaks detect`, neither of which uses the
  `GITHUB_TOKEN`.

## Test Plan

No Rust code changed, so no `cargo` test is applicable to this workflow-only
security hardening. The `job permissions` are already `contents: read`, and the
gitleaks job performs no `git push` / submodule fetch, so dropping the
persisted credential cannot break the pipeline. Confirmed the full quality gate
stays green after the change.
