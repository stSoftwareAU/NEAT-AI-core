# Reword worker-orchestration issue references in bump-deps.sh to concept level

## Summary

The `bump-deps.sh` header comment named the private worker-orchestration
repository twice, as issue slugs. NEAT-AI-core is public and that orchestration
repository is not, so those pointers were dead weight to every public reader and
disclosed the existence of a private orchestration repository
(private-repo-reference audit, check 3).

The fix rewords the header to concept level — it now states the contract inline
("refreshes external Cargo dependencies ahead of the quality gate, honouring the
quarantine window; NEAT-AI-core is the root of the internal dependency chain, so
there are no internal pins to refresh") and drops the private issue slugs. No
behaviour changes — only the header comment. Closes #377.

## Evidence

Backend/CLI-only change with no web interface to screenshot. Verified via the
new bats guard plus the existing `bump-deps.sh` suite and the full quality gate.

```mermaid
flowchart LR
    A["bump-deps.sh header"] -->|before| B["names two private<br/>orchestration issue slugs"]
    A -->|after| C["concept-level contract, no private slugs"]
    C --> D["bump_deps_private_repo_reference.bats guards regression"]
```

## Test Plan

- Added `tests/scripts/bump_deps_private_repo_reference.bats` — "what" tests over
  the committed `bump-deps.sh` artefact:
  - names no private repository (the orchestration repo's name absent)
  - references no private path or issue slug (its `owner/repo` path and
    `repo#N` slugs absent)
  - concept-level contract wording survives (quarantine + "no internal pins to
    refresh" still present)
  - These fail against the pre-fix header and pass after the rewrite.
- Re-ran the existing `tests/scripts/bump_deps.bats` suite — all pass.
- `./quality.sh` passes cleanly.
