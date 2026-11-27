# Public forest bridge observations (Issue #776)

Two original tests load `test/data/grq-23-forests-constants.json` (3,516,246
bytes, SHA-256 `36dcda9e779c785ecc9d50bbf53e859170bb8671a539d0e11a205f4c63cabb0a`).
The creature stays in the legacy checkout. `neat-core/tests/golden/compact_legacy_forest.json`
records the counts and refusals those tests assert.

| Capture | What the frozen helpers did |
|---|---|
| `ordering_declines_if_bridges` | `mergeRedundantConstants` then one `mergeParallelBridges` call. Constant merge leaves 2,263 exported neurons and all 24,100 synapses. The bridge call removes nothing, reload strips nothing, and 25 deterministic sine rows move no output. |
| `exactness_fixpoint_stays_forward` | `mergeParallelBridges` to a fixpoint. A loaded creature lists 2,511 inputs plus 2,538 exported neurons. The fixpoint removes no neuron and emits no backward synapse, so the synapse count stays 24,100. The original output sample uses unseeded `Math.random` and is not frozen. |

These observations are not a native bridge implementation and do not accept
issue #776. Reproduce from a clean checkout at
`60d6b4e5d3af20afbaaeb3420d588ad673ab8dab`:

```sh
deno run -A --config ../NEAT-AI/deno.json \
  scripts/capture_legacy_compaction.ts ../NEAT-AI \
  neat-core/tests/golden/compact_legacy_forest.json forest
cargo test -p neat-core --test compact_legacy_forest
```
