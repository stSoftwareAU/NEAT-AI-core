## Summary
Research-only PR documenting a feature-by-feature comparison between the MIT-licensed [Ikaruga/NEAT-AI](https://github.com/Ikaruga/NEAT-AI) Rust project and `neat-core` (Apache-2.0). Adds `docs/research/ikaruga-neat-ai-comparison.md` covering genome/mutation, speciation, population management, feed-forward evaluation, topology visualisation, GPU (Burn/WGPU), serialisation, emulator bridge, and fitness — each classified as already present / owned by parent repo / out of scope / worth adopting, with licence-compatibility note (MIT → Apache-2.0 inbound). Closes #21.

Two adoption candidates identified:
- **Topology export (DOT/JSON)** — already tracked by existing open issue #22; no new issue filed.
- **`CreatureExport` round-trip JSON (`Serialize` derive)** — new follow-up issue #30 filed and linked from the document.

No code changes to `neat-core` itself; `./quality.sh` run as a sanity check and passes.

## Evidence
No UI or performance change — this is a research document. The only artefact is `docs/research/ikaruga-neat-ai-comparison.md`. Validation: `./quality.sh < /dev/null` runs fmt, clippy, workspace tests, doc build, release build, and `cargo deny` — all pass unchanged.

## Test Plan
- [x] `./quality.sh < /dev/null` passes (existing test suite unchanged).
- [x] `docs/research/ikaruga-neat-ai-comparison.md` exists and covers every feature area listed in issue #21.
- [x] Each candidate has a classification + one-paragraph justification.
- [x] Each "adopt" candidate has a linked issue (#22 existing, #30 new).
- [x] Licence-compatibility note included (MIT inbound → Apache-2.0).
