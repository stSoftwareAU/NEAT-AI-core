## Summary

`neat-core/tests/prune_total.rs` pins the **total-prunability** contract:
every hidden neuron and every listed `(from, to, role)` triple of a valid
creature prunes to `Ok`, and what comes back passes `creature_validate` and the
topology gate, carries no more than `MAX_SUPPORT_CONSTANTS` constants, and is
deterministic. Driven by stSoftwareAU/NEAT-AI-Ockham#195, whose screening loop
cannot be asked to guess which of its candidates the shared helpers will refuse.

No `Err(Cleanup)` was surfaced, so `prune_cleanup.rs` has **no behavioural
change** — the sweep is the evidence that no valid fixture reaches
`InexactMerge` or `NotStable`, which is what the issue set out to establish.

## What it sweeps

19 fixtures, deduplicated by creature across three homes so an overlap counts
once, and each home counted separately so one going empty cannot hide behind
the others:

1. every `before()` of `PRUNE_PARITY_CASES` — the 8 Issue #588 captures;
2. every creature carried by `prune_golden_cases()` — the committed boundary
   record, which contributes `restored_if_role` and `proxy_compensation`. The
   dedup proved its other creatures byte-identical to parity captures, which is
   why no copy of them exists in the test;
3. 9 inline creatures for shapes neither home holds, including
   `surplus_constants` — four support constants, so the `MAX_SUPPORT_CONSTANTS`
   bound is exercised on its boundary rather than holding vacuously.

`prune_cleanup.rs`'s own fixtures are out of scope: they address
`cleanup_creature` directly rather than the two prune entry points.

That is 90 `prune_neuron` and 196 `prune_synapse` requests, each run with no
statistics and with a mean-only `PruneStats`, and each repeated to prove
determinism. Edges out of `input-N` and into output neurons are ordinary
candidates.

## Evidence

- `cargo test -p neat-core --test prune_total` — 9 passed.
- `cargo test -p neat-core` — all 76 test binaries `ok`.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  clean; `./quality.sh < /dev/null` — all checks passed.
- NEAT-AI-Ockham, built against this branch through its sibling `path`
  dependency: `./quality.sh < /dev/null` passed, 675 lib + 70 integration tests
  green, and the `neat-core` version gate is satisfied (this change is additive,
  so it is a patch under `RELEASING.md`).

**The sweep is red-capable.** Two mutations, applied one at a time and reverted:

| Mutation to `prune_cleanup.rs` | Result |
|---|---|
| `fold_zero_inward_hidden` returns `Ok(false)` | both sweeps fail — `parity/cascade_orphan_feeders neuron h-c (no stats): refused … Cleaned creature is invalid (NO_INWARD_CONNECTIONS): hidden neuron h-a has no inward connections` |
| surplus-constant merge skipped | `inline/surplus_constants synapse input-0 -> h-1 (Standard, no stats): 4 constants survived, above the 3 cap` |

## Also in this change

- `README.md` gains `### Total prunability …`, a section governing **both**
  prune entry points, with a Mermaid diagram and a pointer from the
  hidden-neuron section. It is explicit that `Err(Cleanup)` stays a reachable
  outcome for a hand-built creature — as the flowcharts above it show — and is
  a core defect only on a *valid* one.
- One rustdoc paragraph on `CleanupError::InexactMerge` saying the same, so the
  variant's own documentation no longer reads as a blanket legitimate refusal.

No public item is added, removed, renamed or narrowed.
