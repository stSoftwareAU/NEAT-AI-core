## Summary
Added a canonical `## Related Repositories` section to the NEAT-AI-core README listing all seven public NEAT-AI-* repositories, their one-line roles, and a Mermaid dependency diagram. The same block will be reused verbatim in the other NEAT-AI-* READMEs so the cross-repo description stays consistent. Closes #18.

## Evidence
Documentation-only change — no UI or runtime behaviour affected.

- `./quality.sh < /dev/null` passes cleanly (fmt, clippy, tests, doc, deny, release build).
- Rendered Mermaid graph shows the dependency arrows: NEAT-AI → NEAT-AI-Discovery (Deno FFI), NEAT-AI → NEAT-AI-Snapshot (produces), NEAT-AI-scorer → NEAT-AI-core (path dep), NEAT-AI-Explore → NEAT-AI-Snapshot (reads), NEAT-AI-Examples → NEAT-AI (depends on).

## Test Plan
- [x] `./quality.sh` passes.
- [x] README renders the new `## Related Repositories` section with all 7 repos, links, and Mermaid diagram.
- [x] Canonical block posted as a comment on issue #18 for verbatim reuse by sibling sub-issues.
