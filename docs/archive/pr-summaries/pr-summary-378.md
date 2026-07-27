# Reword private-repo mentions in archived PR summaries to concept level

## Summary

NEAT-AI-core is a **public** repository, but thirteen archived PR summaries
under `docs/archive/pr-summaries/` named the **private** production-trainer and
worker-orchestration repositories by name or slug — the `stSoftwareAU/GRQ` slug,
private issue references (`GRQ#3508`, `GRQ#2391`, `GRQ #3400`,
`stSoftwareAU/VibeCoding#1613` / `#1614`), private paths
(`GRQ-cluster/network.json`, `GRQ-cluster/performance.csv`), a private host
class (`GRQ-23`), and a private launcher's own shell function
(`grq_fail_loud_exit_trap`). A public reader following those pointers hits
material they cannot open and learns the layout of a private production system —
check 3 of the private-repo-reference audit.

This change rewords the enumerated mentions to **concept level**, keeping the
historical narrative intact:

| Before | After |
| --- | --- |
| "VibeCoding workflow sync" | "the worker-orchestration workflow sync" |
| "per the contract in `stSoftwareAU/VibeCoding#1613`" / "no internal pins per `stSoftwareAU/VibeCoding#1614`" | "per the worker-orchestration contract" / "no internal pins to refresh under that contract" |
| "one `GRQ-cluster` training shard's worth … (`GRQ-cluster/performance.csv` / `result.json`)" | "one production-cluster training shard's worth … committed production-run telemetry" |
| "Apple Silicon (GRQ-23, GRQ class)" | "Apple Silicon (Apple M4 Pro class)" |
| "the production GRQ scoring hot path" / "Real GRQ creatures" | "the downstream production scoring hot path" / "Real production creatures" |
| "the committed `GRQ-cluster/network.json` shape" / "the exact committed GRQ-cluster topology" | "the downstream production cluster's committed creature shape" / "the exact committed production-cluster topology" |
| "GRQ #3400 (flags)" | "a downstream production-trainer issue (flags)" |
| "`GRQ#3508`'s captured signature" / "= GRQ#3508" | "the downstream production trainer's captured OOME signature" / "= the production OOME" |
| "Silent-failure guard (GRQ#2391)" | "Silent-failure guard (the fail-loud rule, Issue #3234)" |
| "Verified against `stSoftwareAU/GRQ` `Develop`" | "Verified against the downstream production trainer's `Develop` branch" |
| "GRQ's `worker/learn.sh`" / "GRQ's production selector" / "GRQ's real `memory_calc.sh`" | "the production trainer's `worker/learn.sh`" / "the production selector" / "the production trainer's real `memory_calc.sh`" |
| "the GRQ-26 crash class" | "the constrained-host crash class" |
| "`grq_fail_loud_exit_trap` EXIT trap" | "fail-loud EXIT trap" |

Files touched (13 archives): `pr-summary-5.md`, `pr-summary-6.md`,
`pr-summary-38.md`, `pr-summary-228.md`, `pr-summary-243.md`,
`pr-summary-246.md`, `pr-summary-261.md`, `pr-summary-286.md`,
`pr-summary-287.md`, `pr-summary-288.md`, `pr-summary-296.md`,
`pr-summary-298.md`, `pr-summary-299.md`.

Closes #378.

## Scope note

Two deliberate exclusions:

- `production_exact_matches_committed_grq_topology` in `pr-summary-286.md` is
  **this** repository's own live test-function name. Renaming it in an archive
  would dangle against the test suite, so it stays — the same call
  `pr-summary-376.md` made. The guard matches on **word boundaries**, so
  snake_case identifiers that merely embed the letters are not flagged.
- `pr-summary-373.md` … `pr-summary-377.md` are the records of the earlier
  reword PRs; their before/after tables must quote the old strings to stay
  meaningful, and they were not in the issue's enumerated list.

The before/after table above and the new guard's `grep` patterns necessarily
quote the private tokens — a guard cannot match a name it does not spell, and
the record of a reword is unreadable without its "before" side. That is the same
trade the existing `private_repo_reference.bats` /
`bump_deps_private_repo_reference.bats` guards and `pr-summary-373.md` …
`pr-summary-377.md` already make.

Internal script paths (`worker/learn.sh`, `memory_calc.sh`, `node.sh`) are left
as-is: the issue scoped this sweep to repository names and issue slugs, and
`tests/scripts/private_repo_reference.bats` already governs those paths where
they appear in live sources.

## Evidence

Docs-only change — no code, no web interface to screenshot. Verification is the
new bats guard plus the existing gates.

```mermaid
flowchart LR
    A["private names in 13 archives:<br/>stSoftwareAU/GRQ, GRQ#3508,<br/>GRQ-cluster/*, GRQ-23,<br/>stSoftwareAU/VibeCoding#1613"] --> B["concept-level reword"]
    B --> C["downstream production trainer,<br/>production-cluster topology,<br/>Apple M4 Pro class,<br/>worker-orchestration contract"]
    C --> D["bats guard pins it"]
```

- `bats tests/scripts` → **289 tests, 0 failures** (9 of them new).
- `deno run --allow-read scripts/check_mermaid.ts .` → all Mermaid blocks pass
  (two diagrams were reworded).
- `./quality.sh < /dev/null` → passes cleanly.

## Test Plan

New guard `tests/scripts/archive_pr_summaries_private_repo_reference.bats`
(9 "what" tests over the committed artefacts, matching the style of the existing
`private_repo_reference.bats` / `bump_deps_private_repo_reference.bats`). It
fails against the pre-fix tree — the three private-name assertions all matched
before the reword — and passes after:

- `every enumerated archived PR summary is present` — the guard cannot silently
  pass by pointing at missing files.
- `archived PR summaries name no private production-trainer repository` —
  word-boundary `GRQ` absent across all thirteen.
- `archived PR summaries name no private worker-orchestration repository` —
  word-boundary `VibeCoding` absent.
- `archived PR summaries reference no private repository path or issue slug` —
  `stSoftwareAU/GRQ`, `stSoftwareAU/VibeCoding`, `GRQ#N`, `GRQ #N`,
  `VibeCoding#N` all absent.
- `pr-summary-299 carries no grq-prefixed private launcher identifier` — the one
  snake_case case the word-boundary rule cannot reach.
- Four narrative-survival assertions so the reword cannot degrade into deletion:
  the bump contract (`pr-summary-38`), the learn-invocation wiring
  (`pr-summary-299`), the exit-133 heap signature (`pr-summary-296`), and the
  production topology numbers (`pr-summary-286`).
