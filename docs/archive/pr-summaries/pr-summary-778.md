## Summary

This ports two of the existing speculative compaction rules into `neat-core`,
together with the shared primitive one of them needs and the tag-free part of a
third. Nothing calls them yet: no caller is switched and no existing public
struct changes.

- `aggressive_prune` (`neat-core/src/compact_aggressive_prune.rs`) ports
  TypeScript `aggressivePrune`, the dataset-free low-weight prune.
- `simplify_large_weights` and `weight_bias_penalty`
  (`neat-core/src/compact_simplify_large_weights.rs`) port
  `simplifyLargeWeights`, the rescaling of homogeneous squashes, including its
  MCMC acceptance.
- `magnitude_penalty` and `mean_magnitude_penalty`
  (`neat-core/src/magnitude_penalty.rs`) port the per-value penalty from
  `Score.ts` that `simplifyLargeWeights` accepts against.
- `remove_neuron_by_bias_fold` (`neat-core/src/compact_remove_neuron.rs`) ports
  the bias-fold branch of `compactUnused`'s `removeNeuron`, along with the
  `removeHiddenNeuron` orphan cascade it ends with.

A new `simplify` capture mode freezes the TypeScript results so the native tests
can replay them bit for bit, and the `metadata` capture mode now also records
TypeScript's constant branch for every target squash that defines `propagate`. Two parts of `compactUnused` are not ported, so
this is Part of #778:

- its neuron selection, which reads trace statistics and the newborn-grace tag
  and stays with the caller;
- its constant-neuron branch, which is keyed on the `compact-unity-slot` neuron
  tag that core's structs cannot carry.

The original `compactUnused` fixtures that need either part are not replayed
either (see "Parity with the original fixtures" in the closure list): `FixIF.ts`,
`CompactConstant.ts` and `CompactKeepOrder.ts` (constant branch), `NewbornGrace.ts`
(tags) and the constant-feeder cascade of `RemoveHiddenNeuronCascade.ts`.

The branch also carries the routine `bump-deps.sh` lock refresh. The
`wasm-bindgen` family (`wasm-bindgen`, its macro and shared crates, `js-sys`,
`web-sys`) is held at 0.2.128 / 0.3.105, the version `wasm-bundle.yml` pins for
the CLI.

## Spec

### Intent and Rationale

- #776 froze the TypeScript behaviour, and #778 asks for the existing rules to
  be ported against it. Each rule is ported literally: same guards, same
  constants (`1e-3`, the `1000` imbalance ratio, the `1e-15` margin), same order
  of operations.
- The `simplifyLargeWeights` captures did not exist yet.
  `scripts/legacy_compaction_simplify.ts` generates them from the frozen
  checkout in the same way as the other capture modes, so the expected values
  come from TypeScript and not from the Rust under test. They include the eight
  per-squash fixtures of the original
  `CompactCreatureSimplifyLargeWeightsSupportedSquashes.ts` (ReLU, LeakyReLU,
  MAXIMUM, MINIMUM, IF, HYPOT, HYPOTv2, MEAN).

### Essential Design Decisions

- **Frozen synapses are passed by index.** `aggressive_prune` takes a
  `frozen_synapses: &[usize]` argument. `SynapseExport` has no `frozen` field,
  and adding one is a public-struct change that the three-phase rule
  (`RELEASING.md`) puts out of scope for an additive PR. An out-of-range index
  is refused with `FrozenIndexOutOfRange`.
- **Randomness and acceptance stay with the caller.**
  - `simplify_large_weights` takes `Option<McmcAcceptance>`, and the caller
    supplies the uniform draw that TypeScript took from `Math.random`.
  - Neither pass scores anything. Keeping the aggressive candidate is still the
    caller's `selectCompactVariant` policy.
  - The magnitude penalty is the existing heuristic gate inside
    `simplifyLargeWeights`, not the authoritative score.
- **Exactness follows the actual semantics.**
  - `aggressive_prune` is approximate: it removes non-zero weights, including
    into aggregates.
  - `simplify_large_weights` is exact in real arithmetic for positively
    homogeneous squashes, but only rounding-equal in floating point. With MCMC
    it can accept a worse penalty.
- **Memetic handling and stricter refusals.**
  - Memetic handling follows what each rule does. `aggressive_prune` only
    removes edges, so it uses `prune_memetic`, the crate's convention from
    `compact_zero_weight` (TypeScript deletes the whole record).
  - `simplify_large_weights` rewrites weights, so it clears the record
    (`memetic = None`), as TypeScript does.
  - A `NaN` aggressive threshold is refused with `NanThreshold`. In TypeScript a
    NaN threshold would prune every finite unpinned edge. This is the one
    stricter departure.

- **`removeNeuron` is ported only where it needs no tags.**
  - `remove_neuron_by_bias_fold` keeps the TypeScript guards. It computes every
    folded bias before applying any, and when one is non-finite it returns
    `Ok(None)` (TypeScript's `false`) with nothing changed. A target left with
    fewer than two inbound synapses gets a zero-weight edge from input
    `floor(u * input)`. The edge is placed where TypeScript's `(from, to)` index
    order puts it. On removal the memetic record is cleared.
  - The uniform draw `u` comes from the caller, as in `simplify_large_weights`.
    A draw outside `[0, 1)` names no input and is refused, as is an outward
    synapse into a target that is not listed (`DanglingTarget`, where TypeScript
    would throw).
  - TypeScript sends a target whose squash defines `propagate` down the
    constant branch. At the frozen revision that is six squashes: `IF`,
    `MINIMUM`, `MAXIMUM`, `MEAN`, `HYPOT` and `HYPOTv2`. The `metadata` capture
    records TypeScript taking that branch for each (a unity constant neuron is
    wired in and the target's bias is left alone). That branch reuses a constant neuron
    keyed on the `compact-unity-slot` neuron tag, and `NeuronExport` has no
    tags. Adding a field breaks the scorer's struct literals and needs the
    three-phase flow in `RELEASING.md`. Such a target is therefore refused with
    `ConstantBranchNotPorted`, not approximated. The refusal is derived from
    `SquashType::is_aggregate`, the single home of the aggregate set (Issue
    #446), after `parse_squash_name`, so a seventh aggregate is refused without
    an edit here. Folding into a `MEAN`, `HYPOT`
    or `HYPOTv2` bias would be wrong even for an exactly constant activation,
    because dropping an inbound edge changes `MEAN`'s `n` and the sum of squares
    under `HYPOT`.
  - Choosing the neuron stays with the caller: the trace statistics, the shuffle
    and the newborn-grace skip (also a neuron tag).

### Undiscoverable Facts

- `aggressivePrune` pins any `type !== undefined`, so an empty-string role is
  pinned. The zero-weight pass treats `""` as untyped. Both ports keep their own
  TypeScript reading.
- `Math.max` propagates NaN where Rust's `f64::max` ignores it.
  `simplify_large_weights` uses a `js_max` helper so a NaN bias or inbound
  weight skips the neuron, as it does in TypeScript (`nan_bias`,
  `nan_inbound_first` captures).
- `magnitude_penalty` restates NEAT-AI-scorer
  `rust_scorer/src/scoring.rs::magnitude_penalty`. Core cannot call the scorer,
  because the scorer depends on core. The two copies must stay identical until
  the scorer moves to this one; this PR does not switch it. Both are pinned to
  NEAT-AI's `magnitude-penalty-corpus.json`, which is captured into the golden
  file.
- `CompiledNetwork::activate` runs in `f32`, so the behaviour-preservation test
  compares within `1e-6` relative.

## Evidence

This is a backend library change with no UI, so there are no screenshots. The
evidence is the tests below.

- `neat-core/tests/compact_aggressive_prune.rs`:
  - `frozen_captures_replay_with_the_same_survivors` replays the three raw
    `aggressivePrune` captures in
    `neat-core/tests/golden/compact_legacy_variants.json`: `aggressive_maximum`,
    `aggressive_frozen` and `aggressive_last_input`.
  - The other 15 tests cover typed IF roles, empty-string roles, IF targets,
    aggregate targets (MINIMUM/HYPOT), the threshold boundary, negative weights,
    NaN and infinite weights, the last inbound edge of an output, frozen
    indices, the NaN threshold, memetic rows and neuron retention.
    `the_result_is_a_pruned_copy_that_leaves_the_input_alone` asserts the
    returned creature holds exactly the surviving synapse (1e-9 is below the
    1e-3 threshold, so one edge goes) and that the input is unchanged.
- `neat-core/tests/compact_simplify_large_weights.rs`:
  - `frozen_captures_replay_bit_for_bit` replays all 23 cases in
    `neat-core/tests/golden/compact_legacy_simplify.json`. It checks the
    `changed` flag, the draw count, every bias and weight by `to_bits`, the
    memetic clearing, and the before and after penalties within `1e-12`.
  - The 23 cases are 15 hand-made ones — ABSOLUTE, ReLU with negative values, a
    tiny inbound edge, typed IF, MEAN/HYPOTv2, TANH left alone, a balanced
    creature, a skipped output, a NaN bias, a NaN first inbound edge, an
    infinite inbound edge, a self-loop, a greedy reject, an MCMC accept and an
    MCMC reject — and the 8 original per-squash fixtures `supported_ReLU`,
    `supported_LeakyReLU`, `supported_MAXIMUM`, `supported_MINIMUM`,
    `supported_IF`, `supported_HYPOT`, `supported_HYPOTv2` and `supported_MEAN`.
  - `magnitude_penalty_matches_the_shared_corpus` checks every corpus case
    within the corpus tolerance.
  - `rescaling_preserves_behaviour` compiles the creature before and after for
    the `absolute`, `relu_negative`, `tiny_inbound` and `typed_if` cases and the
    eight `supported_*` fixtures, and compares their activations at the sample
    points of the original tests. Together these rescale every squash in
    `CANDIDATE_SQUASHES`.
  - `the_rescaled_copy_differs_from_an_input_that_is_left_alone` asserts the
    returned copy differs from the input and the input is unchanged.
  - `a_non_positive_temperature_is_greedy` checks that a temperature of 0 or -1
    never draws and never accepts a worse rescale.
- `neat-core/tests/compact_remove_neuron.rs`:
  - `frozen_remove_neuron_captures_replay_exactly` replays the four #776
    `removeNeuron` captures in
    `neat-core/tests/golden/compact_legacy_metadata.json` (`finite_normal`,
    `finite_overflow`, `finite_nan`, `finite_atomic`). It starts from each
    capture's runtime `before` state, so the injected overflow values are
    included. It compares every bias and weight by `to_bits`, in order, and
    checks that a refusal takes no draw.
  - `removing_a_saturated_clipped_neuron_preserves_behaviour` is the native
    counterpart of `test/compact/UnusedClipped.ts`. It uses the same creature: a
    `CLIPPED` hidden neuron with bias 2 and inbound weights ±0.3 from inputs in
    `[0, 1)`, which always outputs 1. Folding that traced mean of 1 must leave
    both outputs unchanged. It checks a 125-point input grid through
    `compile_creature` + `activate` within `1e-6`. As in TypeScript, the trace
    collection and the neuron choice belong to the caller.
  - `every_propagate_squash_target_is_refused_as_the_unported_constant_branch`
    reads the six `removeNeuronConstantBranch` captures in the same golden file
    (`IF`, `MINIMUM`, `MAXIMUM`, `MEAN`, `HYPOT`, `HYPOTv2`). Each records
    `definesPropagate`, a successful TypeScript removal that wired in a constant
    neuron, and the target's bias left at 0.25. The test asserts the port
    returns `ConstantBranchNotPorted` for all six.
  - `every_aggregate_squash_target_is_refused_and_every_other_folds` sweeps
    every `SquashType` (the discriminants `0..=255` that round-trip through
    `From<u8>`), names the target with `squash_name_from`, and asserts the
    refusal exactly where `is_aggregate()` holds and the `0.25 + 2.0 * 1.5 =
    3.25` fold everywhere else. It asserts six refusals and more than twenty
    folds, so a vacuous sweep fails. It ties the refusal to the single
    aggregate set rather than to the six names.
  - `a_target_without_propagate_takes_the_bias_fold` is the positive control:
    `IDENTITY`, `TANH` and `ReLU` targets fold to `0.25 + 2.0 * 1.5 = 3.25`.
  - The other 8 tests (13 in the file) cover:
    - the draw-to-input mapping;
    - a target that keeps two inbound synapses;
    - where the reconnect edge lands in `(from, to)` order;
    - a dangling outward target;
    - the feeder cascade;
    - memetic clearing on removal and keeping it on refusal;
    - the width refusal for a creature with no inputs;
    - unknown and non-hidden neurons, and out-of-range draws.
  - Each mutation below turned the suite red and was reverted:
    - drop the finite guard;
    - halve the folded bias;
    - append the reconnect edge instead of inserting it in order;
    - skip a dangling target instead of refusing it;
    - reconnect at `< 1` inbound;
    - skip the cascade;
    - round instead of floor;
    - keep the memetic record;
    - drop each of `IF`, `MINIMUM`, `MAXIMUM`, `MEAN`, `HYPOT` and `HYPOTv2` from
      the constant-branch set, when it was a hand-kept list (six runs, each red
      in `every_propagate_squash_target_is_refused_as_the_unported_constant_branch`);
    - exclude `MEAN` from the `is_aggregate`-derived refusal (red in both
      `every_aggregate_squash_target_is_refused_and_every_other_folds` and
      `every_propagate_squash_target_is_refused_as_the_unported_constant_branch`);
    - misspell each of the eight non-trivial entries of `CANDIDATE_SQUASHES`
      (`ReLU`, `LeakyReLU`, `MAXIMUM`, `MINIMUM`, `IF`, `HYPOT`, `HYPOTv2`,
      `MEAN`): two simplify tests go red for every one.
- `docs/research/compaction-simplify-baseline.md` records the new capture, a
  table of the cases, the exactness classification and the reproduction command.
  `docs/research/compaction-variant-baseline.md` now names the native coverage
  of the aggressive guards.

```mermaid
flowchart LR
    TS["frozen NEAT-AI 60d6b4e5<br/>AggressivePrune.ts / SimplifyLargeWeights.ts / Score.ts"] -->|capture_legacy_compaction.ts| G["golden JSON"]
    G --> T["native replay tests"]
    C["caller (unchanged)<br/>frozen indices, uniform draw, variant scoring"] -.-> A["aggressive_prune"]
    C -.-> S["simplify_large_weights"]
    S --> P["weight_bias_penalty → magnitude_penalty"]
    A --> T
    S --> T
```

## Acceptance Criteria

<!-- vibe-spec-review inputs="diff+issue-body" -->

Verdicts come from two independent spec-reviewer passes: a full pass over
5fbf16b, then a scoped pass over 2983a18 that covered the remove-neuron fixes
and the clipping test. 958b6ad came after both passes. It adds only the width
refusal, which one of them suggested.

- **met** — Inventory and port TS AggressivePrune — evidence:
  `neat-core/tests/compact_aggressive_prune.rs::frozen_captures_replay_with_the_same_survivors`
  — reviewer: met
- **met** — Inventory and port the simplify/large-weight transforms and the
  shared primitive they need — evidence:
  `neat-core/tests/compact_simplify_large_weights.rs::frozen_captures_replay_bit_for_bit`,
  `::magnitude_penalty_matches_the_shared_corpus` — reviewer: met
- **partial** — Inventory and port CompactUnused — evidence:
  `neat-core/tests/compact_remove_neuron.rs::frozen_remove_neuron_captures_replay_exactly`
  — reviewer: partial — reason: the bias-fold branch of `removeNeuron` and the
  `removeHiddenNeuron` cascade are ported. The constant branch
  (`IF`/`MINIMUM`/`MAXIMUM`/`MEAN`/`HYPOT`/`HYPOTv2` targets, the six squashes
  that define `propagate`) is refused with `ConstantBranchNotPorted`,
  because it is keyed on the `compact-unity-slot` neuron tag, and `NeuronExport`
  cannot carry tags without a three-phase public-struct change.
- **met** — Keep speculative selection, statistics collection and authoritative
  scoring at the caller boundary; no score-based acceptance in the graph engine
  — evidence: `McmcAcceptance` and the `draw` closure are caller-supplied, and
  neither pass scores anything — reviewer: met
- **partial** — Preserve the safe/aggressive output contracts and the
  unsupported/no-op behaviour; keep the existing guards; no new thresholds —
  evidence:
  `neat-core/tests/compact_aggressive_prune.rs::the_result_is_a_pruned_copy_that_leaves_the_input_alone`
  — reviewer: partial — reason: there are no new thresholds, but there are
  documented departures. `aggressive_prune` prunes memetic rows where TypeScript
  deletes the record. `NanThreshold`, `FrozenIndexOutOfRange`, `DrawOutOfRange`
  and `DanglingTarget` are new refusals.
- **met** — Exactness classifications follow the actual semantics — evidence:
  `docs/research/compaction-simplify-baseline.md` and the module docs. The bias
  fold is classified exact only while the activation equals the folded
  constant, and that now holds for every accepted target, because the six
  `propagate` squashes (including `MEAN`, `HYPOT` and `HYPOTv2`) are refused,
  as `every_propagate_squash_target_is_refused_as_the_unported_constant_branch`
  pins against TypeScript's own captures — reviewer: met
- **met** — Tests for typed IF — evidence:
  `compact_aggressive_prune.rs::an_if_output_keeps_every_typed_tiny_edge`, the
  simplify `typed_if` capture, and
  `compact_remove_neuron.rs::every_propagate_squash_target_is_refused_as_the_unported_constant_branch`
  — reviewer: met
- **met** — Tests for aggregation — evidence:
  `compact_aggressive_prune.rs::tiny_edges_into_aggregate_hidden_neurons_are_removed`,
  the `mean_hypot` capture and the `supported_MAXIMUM`, `supported_MINIMUM`,
  `supported_HYPOT`, `supported_HYPOTv2` and `supported_MEAN` simplify captures,
  and `compact_remove_neuron.rs::every_propagate_squash_target_is_refused_as_the_unported_constant_branch`
  (MEAN, HYPOT and HYPOTv2 take TypeScript's constant branch and are refused)
  — reviewer: met
- **met** — Tests for clipping — evidence:
  `compact_remove_neuron.rs::removing_a_saturated_clipped_neuron_preserves_behaviour`
  (the `UnusedClipped.ts` creature against an independent `compile_creature`
  oracle) — reviewer: met
- **met** — Tests for non-finite statistics — evidence: the `finite_overflow`,
  `finite_nan` and `finite_atomic` replays,
  `compact_aggressive_prune.rs::non_finite_weights_are_kept`, and the
  `nan_bias`, `nan_inbound_first` and `infinite_inbound` simplify captures —
  reviewer: met (for the ported scope; the trace-mean fallback is caller-side
  statistics)
- **missing** — Tests for deadlines — reviewer: missing — reason: the only
  deadline is the `skipCompaction` check in `TrainingTeardown.ts`, which sits
  with the caller around `compactUnused`. No ported rule has a deadline to test.
- **partial** — Tests for tag/memetic semantics — evidence:
  `compact_remove_neuron.rs::removal_clears_the_memetic_record_and_a_refusal_keeps_it`,
  `compact_aggressive_prune.rs::memetic_rows_for_a_removed_edge_go_and_the_record_stays`
  — reviewer: partial — reason: memetic semantics are tested. Neuron and
  creature tags (`unused`, `approach`, `old-*`, `compact-unity-slot`) are not
  represented in core's structs, so they are neither ported nor tested.
- **partial** — Parity with the original fixtures — evidence: 3 aggressive
  captures, 23 simplify captures (the 15 hand-made cases plus the 8 original
  per-squash fixtures of `CompactCreatureSimplifyLargeWeightsSupportedSquashes.ts`),
  the 4 #776 `removeNeuron` captures (`CompactUnusedFiniteGuard.ts`), replayed
  bit for bit and in order, and the `UnusedClipped.ts` creature through
  `compact_remove_neuron.rs::removing_a_saturated_clipped_neuron_preserves_behaviour`
  — reviewer: partial — reason: the other `compactUnused` originals are not
  replayed:
  - `FixIF.ts`, `CompactConstant.ts` and `CompactKeepOrder.ts` each remove a
    `CLIPPED` neuron feeding an `IF` neuron through a typed synapse. TypeScript
    takes the constant branch; the port refuses it with `ConstantBranchNotPorted`
    (it is keyed on a neuron tag core cannot carry).
  - The 6 constant-branch `removeNeuron` captures are **refused, not replayed**:
    the test asserts the port returns `ConstantBranchNotPorted` for each, and
    checks the capture's own facts (TypeScript removed it, wired in a constant
    neuron, left the bias at 0.25).
  - `NewbornGrace.ts` needs neuron tags, and the trace-based neuron selection is
    caller policy.
  - `Cascade.ts`, `CompactUnusedOrphanCleanup.ts` and the constant-feeder case of
    `RemoveHiddenNeuronCascade.ts` are not replayed. The first two need no tags
    and could be driven through `remove_neuron_by_bias_fold`; no native test has a
    constant feeder or removes a constant neuron. They stay open under #778.
- **met** — Additive changes only; callers not switched — evidence:
  `neat-core/src/lib.rs` adds modules and re-exports only — reviewer: met
- **unrequested** — The `Cargo.lock` refresh from `bump-deps.sh` (9d134a8), with
  the `wasm-bindgen` family held at 0.2.128 (a039b6a) — reviewer: unrequested —
  reason: the routine dependency bump this repository's PR pipeline runs. The
  hold keeps the lock in step with the CLI pinned in `wasm-bundle.yml`.

## Test Plan

- Added tests:
  - `neat-core/tests/compact_aggressive_prune.rs` (16 tests);
  - `neat-core/tests/compact_simplify_large_weights.rs` (7 tests);
  - `neat-core/tests/compact_remove_neuron.rs` (13 tests).

  This fix also added the `simplify` fixtures and the `removeNeuronConstantBranch`
  captures to the two golden files and the `scripts/legacy_compaction_*.ts`
  capture scripts. The existing records in both files regenerate byte for byte
  before the additions, and the additions are deterministic across two runs.

  Doctests were added on `aggressive_prune`, `simplify_large_weights`,
  `magnitude_penalty`, `mean_magnitude_penalty` and
  `remove_neuron_by_bias_fold`. One test that existed on `Develop` was edited: the
  record-count pin in `neat-core/tests/compact_legacy_metadata.rs`
  (`creature_metadata_captures_preserve_original_validation_contracts`) moved from
  `2 + 4 + 2` (8) to `2 + 4 + 6 + 2` (14) for the six new
  `removeNeuronConstantBranch` captures. The count is still exact and no
  assertion was removed or loosened. No other existing test was edited.
- Added the `simplify` mode to `scripts/capture_legacy_compaction.ts` and added
  `scripts/legacy_compaction_simplify.ts`. The golden file was generated against
  a clean checkout of NEAT-AI at `60d6b4e5d3af20afbaaeb3420d588ad673ab8dab`.
  Every value matched an independent run of the same TypeScript.
- The `wasm-bindgen` family is held at the pinned CLI version.
  `bats tests/scripts/wasm_bundle_wasm64_dual_ship.bats` passes (14/14), and so
  does `scripts/lockfile-freshness.sh --check`.
- `./quality.sh < /dev/null` passed on the working tree of the review fix
  (exit 0, "All quality checks passed!"). That run covers
  `cargo test --workspace`, clippy, fmt, doc, the bats gates and the deno
  checks. `markdownlint-cli2` reports 0 issues on this summary and the two
  baseline notes edited with it. The only change after that run is prose in
  this summary and the baseline notes.
- Not run:
  - `scripts/check-downstream-consumers.sh`. The change only adds modules and
    re-exports, and changes no existing public item.
  - `cargo check --target wasm32-unknown-unknown`. That target is not installed
    on this host. The new modules are plain safe Rust with no wasm or `cfg`
    code.
  - The `wasm64` check.
