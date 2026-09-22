# Reword private trainer issue-number references in neat-core source comments to concept level

## Summary

NEAT-AI-core is a **public** repository, but the live rustdoc and comment text
of its primary crate named the **private** trainer repository's issue slugs
(three of them) and its internal artefact paths (a production sampler fixture
directory, one of its sample creatures, and a reported Backprop log) directly. Those
are the module- and function-level docs a public reader — or docs.rs — surfaces
first, so they pointed every external reader at material they cannot open and
disclosed the existence and internal layout of a private production system
(check 3 of the private-repo-reference audit, `severity:medium`).

This change rewords each mention to **concept level**, following the pattern
that closed #374/#375/#376/#378, with **no logic or behaviour change** —
comments, doc comments and Markdown prose only:

| Before | After |
| --- | --- |
| the exact-float-parsing heading, suffixed with a private issue slug | "Exact float parsing" |
| the memetic-weight-forms heading, suffixed with a private issue slug | "Two memetic weight forms" |
| a contract list citing two private issue slugs as the provenance of both contracts | "… Two further contracts came from downstream production defects rather than issues here: both memetic weight forms, and exact float parsing." |
| the fittest creature described by the private sampler fixture's name | "a production sampler fixture's fittest creature" |
| a private sampler fixture creature's full internal path | "a production sampler fixture creature" |
| the exact failure attributed to a named private Backprop log file | "The exact failure from the reported production Backprop log" |
| a private issue slug as the subject of the same-role fan-in reading | "A downstream production report reads as though same-role fan-in …" |
| a Cargo.toml comment citing a private issue slug beside `float_roundtrip` | "# Exact float parsing - `float_roundtrip` is not optional" |
| the private trainer named as the owner of the 4 272-neuron, 22 928-synapse production creature | "on the downstream production trainer's 4 272-neuron, 22 928-synapse production creature" |

The technical explanations the issue asked to keep — float round-trip
exactness, the two memetic-weight wire forms, the same-role fan-in rule — are
untouched.

Files touched: `neat-core/src/creature.rs`,
`neat-core/src/creature_validate.rs`,
`neat-core/tests/creature_float_roundtrip.rs`,
`neat-core/tests/creature_memetic_weight_forms.rs`,
`neat-core/tests/creature_validate_synapse_rules.rs`, `neat-core/Cargo.toml`,
`neat-core/benches/BASELINE.md`, plus the new guard
`tests/scripts/neat_core_private_repo_reference.bats`.

Closes #661.

## Scope note — `BASELINE.md:118`

The enumerated `BASELINE.md:118` hit is
`tests/bench_fixtures.rs::production_exact_matches_committed_grq_topology` — a
**snake_case identifier** naming this repository's own live test function, not a
private issue slug or path. It is deliberately left unchanged, consistent with
the decisions already recorded in `pr-summary-376.md` (scope note) and
`pr-summary-378.md`: renaming it would dangle the references in
`pr-summary-286.md` and `pr-summary-376.md`, and the repo's existing private-repo
guards already match the private name on word boundaries (`grep -w`) precisely so
embedded-letter identifiers are not flagged. The new guard uses the same
word-boundary convention and documents the exception in its header.

## Evidence

Docs/comments-only change with no web interface to screenshot. Verification is
the new bats guard (red before, green after) plus the existing Rust suites and
the full local gate.

```mermaid
flowchart LR
    A["private names in live neat-core docs:<br/>three private issue slugs,<br/>the private sampler fixture dir and a sample creature,<br/>a reported Backprop log, the trainer's production creature"] --> B["concept-level reword<br/>(comments and doc text only)"]
    B --> C["a production sampler fixture,<br/>the reported production Backprop log,<br/>a downstream production report,<br/>the downstream production trainer"]
    C --> D["tests/scripts/neat_core_private_repo_reference.bats<br/>pins the outcome"]
```

- **Guard red against the unreworded sources**: `bats
  tests/scripts/neat_core_private_repo_reference.bats` before the reword →
  `not ok 2`, `not ok 3`, `not ok 4` (only the existence test passed).
- **Guard green after the reword**: same command → `1..4`, all four `ok`.
- **No private-repo token remains** in the enumerated files: a recursive,
  case-insensitive, word-boundary grep of `neat-core/` for the private
  repository's name → no matches (the only remaining case-insensitive hits are
  the two snake_case test-name occurrences covered by the scope note above).
- **Rust tests over the touched files still pass**:
  `cargo test -p neat-core --test creature_float_roundtrip --test
  creature_memetic_weight_forms --test creature_validate_synapse_rules` →
  all pass (32 in the synapse-rules suite alone; 0 failed across the three).
- **`cargo fmt --check`** → clean; the reflowed doc comments are canonical.
- **Full gate**: `./quality.sh < /dev/null` → **✅ All quality checks passed!**

## Test Plan

- Added `tests/scripts/neat_core_private_repo_reference.bats` — four "what"
  tests over the committed artefacts, asserting the observable outcome for the
  seven enumerated files: all present; no private-repo token on word boundaries;
  no private repository path or issue slug in any of its spellings; and none of
  the private trainer's internal artefact paths — its sampler fixture directory,
  its sample creature files, or its Backprop logs. Observed failing against the
  pre-reword sources and passing after — it is a real regression guard, not a
  tautology. It runs inside `./quality.sh` via the existing `bats tests/scripts`
  gate.
- Re-ran the three Rust test suites whose doc comments changed to confirm no
  behaviour moved.
