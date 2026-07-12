## Summary

`docs/research/ikaruga-neat-ai-comparison.md` still framed two adoption
candidates as *pending* work, but both have since landed on `Develop`. The
research doc was therefore an anti-map: an agent triaging follow-ups would be
sent to re-implement completed, tested features. This PR updates the two rows to
**done** while keeping the historical analysis intact. Closes #260.

Verified against current code before editing:

- **Topology export (Issue #22)** — `neat-core/src/topology_export.rs` exists and
  provides `to_dot` and `to_topology_json` from `CompiledNetwork` (module doc
  cites Issue #22), tested in `neat-core/tests/topology_export.rs`.
- **Serialize round-trip (Issue #30)** — `CreatureExport`, `NeuronExport` and
  `SynapseExport` in `neat-core/src/creature.rs:35,54,69` all derive **both**
  `Deserialize` and `Serialize`; the module doc states "derive both".

## Changes

- Module inventory line (`creature` row): now states the export structs derive
  both `Deserialize` and `Serialize` (Issue #30 landed).
- Candidate **#6 Topology visualisation**: 🎯 → ✅ adopted; "no export path" reworded
  to reference the shipped `topology_export.rs`; "No new issue needed" → **done**.
- Candidate **#8 Serialisation format**: 🎯 → ✅ adopted; "only `Deserialize`" reworded
  to "both `Serialize` + `Deserialize`", status **done**.
- Summary table rows 6 and 8: classification → ✅ adopted; `neat-core` columns
  updated to reflect the shipped state; follow-up column marked "(landed)".
- "Adoption candidates" list reframed from proposed work to shipped outcome.

## Evidence

Documentation-only change — no web interface to screenshot. Correctness was
verified two ways:

1. **Source of truth check** — grepped the current code to confirm both features
   ship (`topology_export.rs` present with `to_dot`/`to_topology_json`;
   `creature.rs` derives both traits).
2. **Quality gate** — `./quality.sh < /dev/null` passes cleanly, including the
   existing `topology_export.rs` tests
   (`to_dot_method_callable_on_compiled_network`,
   `to_topology_json_method_callable_on_compiled_network`) that exercise the very
   code the doc now describes as landed:

```
test result: ok. 9 passed; 0 failed; ...
✅ All quality checks passed!
```

## Test Plan

No new tests: this is a stale-documentation fix, and no doc-consistency test
references this file. The features the doc describes are already covered by
existing tests — `neat-core/tests/topology_export.rs` (topology export, Issue
#22) and the creature/serde tests (round-trip `Serialize`, Issue #30) — which
continue to pass under `./quality.sh`.
