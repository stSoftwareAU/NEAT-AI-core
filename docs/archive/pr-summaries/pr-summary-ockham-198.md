# `prune_neuron` rewrites an `IF` left short a role exactly (Ockham #198)

## Summary

`prune_neuron` ran cleanup under `IfRepair::Downgrade` — the one **inexact**
rewrite this module has — so an `IF` a removal left short of a role came back as
the `IDENTITY` sum of everything still reaching it and could no longer branch at
all. It now asks for `IfRepair::Rewrite`, the exact repair `prune_synapse` has
used since Issue #591: the `IF` is flattened onto the arm a statically decided
condition always takes, or given back the emptied arm on a zero-weight support
edge.

`cleanup_creature`'s default policy is untouched, so the `prune_fixtures`
captures keep a caller that reproduces them byte for byte
(`neat-core/tests/prune_cleanup.rs`).

Requested by stSoftwareAU/NEAT-AI-Ockham#198, under its milestone #194
("guarantee every hidden neuron and synapse prunes").

## What changed

- **`prune_neuron.rs`** — `cleanup_creature_with(&cut, CleanupOptions {
  if_repair: IfRepair::Rewrite })`, and `shortfall_costs_nothing`: the one route
  by which an aggregate shortfall still reaches `TransformClass::Exact`. A target
  named on `uncompensated` got no fold, which normally ends any exactness claim;
  an `IF` whose condition **the creature itself decided the same way before and
  after the cut** is the exception, because a condition term only picks an arm,
  and a term out of the arm the pick discards is read on no record at all.
- **`prune_synapse.rs`** — asks the same rule through `transform_class`, the
  one home of "which conjuncts make up `Exact`". The two entry points must not
  disagree about what the same lost term cost, and this one clause is the whole
  difference; every rewrite they produce was already identical.
- **`prune_cleanup.rs`** — `static_condition_branch(creature, uuid)`, the branch
  decision (`> 0` chooses the positive arm; an untyped edge belongs to it) lifted
  out of `Engine::rewrite_if_neurons` into one `pub(crate)` home, so the rewrite
  and the proof above read the rule from the same place.
- **the ABI key** `downgradedIfNeurons` can no longer be filled by any request
  this ABI accepts, and it is skipped when empty, so in practice it is never
  emitted. `a_downgraded_if_list_still_crosses_under_its_wire_name` pins the
  name positively — the golden record can only assert absence, which a rename or
  a deletion would satisfy too.
- **the golden record** regenerated: `if_repair_coalesces_roles` crosses the wire
  as a `staticIfNeurons` flatten. `downgradedIfNeurons` is now unreachable
  through the JSON ABI, so its coverage assertion is inverted — every golden case
  must carry it **empty**, which is what catches an entry point going back to the
  downgrade.
- **docs** — the four pruning module headers, `README.md`'s cleanup, neuron and
  synapse sections, `docs/research/pruning-parity-matrix.md`'s grading table, and
  a `RELEASING.md` breaking-change-log entry with the `0.15.9 → 0.16.0` minor
  bump. Documented runtime behaviour callers rely on moved, which RELEASING.md
  counts as breaking even though no public item did.

## Where the request and the code differ

Ockham #198's first acceptance criterion asks for `Exact` "when the removed
neuron was structurally constant". Taken literally that is unsound, and the
tests here show why: `a_static_condition_feeder_that_flips_the_branch_is_only_approximate`
removes a structurally constant condition feeder and the pruned creature
demonstrably answers different numbers on the probe records
(`assert_different_function`). Calling that `Exact` would contradict this
module's own contract — "the same number on every record".

So the rule implemented is the provable one: `Exact` where the condition was
decided by the creature **and the arm the forward pass reads did not move**,
`Approximate` otherwise. It delivers the criterion's intent for every case where
the intent is true, and refuses it where it is not.

## Evidence

- `cargo test -p neat-core` — green (285 lib, 44 `prune_neuron`, 39
  `prune_synapse`, 48 `prune_cleanup`, 19 `prune_parity`, 12 `prune_json`).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean.
- `./quality.sh < /dev/null` — "All quality checks passed!", including the
  Mermaid gate, `deno test --allow-read tests/wasm_prune_parity_test.ts`
  (15 passed), bats, doctests, rustdoc and the release build.
- `neat-core/tests/prune_parity.rs` passes **unchanged and unmodified**: it
  grades the captured pairs against each other and never drives them through
  `prune_neuron`.
- The downstream consumer this change was requested by compiles and its full
  suite passes against the candidate core: `./quality.sh` in NEAT-AI-Ockham is
  green with this branch as the sibling checkout, with no Ockham source change.

## Test plan

`neat-core/tests/prune_neuron.rs`

- `an_if_left_short_of_a_role_is_rewritten_by_the_prune` (was
  `..._is_downgraded_by_the_prune`)
- `a_static_condition_feeder_that_leaves_the_branch_where_it_was_prunes_exactly`
- `a_static_condition_feeder_that_flips_the_branch_is_only_approximate`
- `emptying_a_branch_the_condition_never_reaches_prunes_exactly`
- `an_output_carrying_the_if_squash_is_rewritten_in_place_by_a_neuron_prune`

`neat-core/tests/prune_synapse.rs`

- `a_condition_edge_that_never_moved_the_branch_is_cut_exactly`
- `a_condition_edge_that_flips_the_branch_is_only_approximate`
- `an_output_carrying_the_if_squash_is_rewritten_in_place`

`neat-core/tests/prune_json.rs`

- `a_downgraded_if_list_still_crosses_under_its_wire_name`
- `the_golden_record_is_what_the_native_abi_answers_today` (regenerated record)
- `the_golden_record_covers_the_shapes_the_wasm_bundle_is_graded_on` (inverted
  `downgradedIfNeurons` assertion)
