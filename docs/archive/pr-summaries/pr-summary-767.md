## Summary

Adds the family writer audit to `docs/CREATURE_JSON_PRESENTATION.md`
("Family writer audit (Issue #767)"). It contains:

- one row per repository, pinned to a commit SHA, giving the production write
  path or the read-only evidence, the presentation written today, the route to
  the private sample repository, and a verdict;
- a table classifying each writer's differences;
- a churn table for the committed corpus, with commands that reproduce it;
- The private sample repository `git show --numstat` figures, one per classified cause;
- the files changed, with the owning repository for each change made elsewhere;
- a re-check of every verdict in NEAT-AI `docs/RUST_EXTENSION_WRITE_PATH_AUDIT.md`.

It also adds `neat-core/examples/creature_presentation_rewrite.rs`, so the
churn table's "after" column can be reproduced from the tree. Closes #767.

## Spec

### Intent and Rationale

- Record where every persisted creature in the family is written and how its
  bytes differ from the contract. The migrations then land in the repositories
  that own each writer.
- The "after" counts come from a tiny example that calls
  `creature_presentation_json`. That is the same entry point the gate test
  uses, so the documented command and the test cannot disagree.

### Essential Design Decisions

- A verdict is given only where the code was read at the pinned SHA. The one
  hop that could not be traced (below) is stated in the NEAT-AI row rather
  than guessed.
- Compact, scorer-facing files (`baseline.json`, candidate staging) are listed
  as writes, but they are outside the contract, as the doc's "Scope" section
  already says.
- Production scale uses real commits from the private sample repository. Two causes never occur alone
  in the window searched: indentation or newline only, and exponent casing
  only. Both are recorded as "not measured", with the window and the reason.

### Undiscoverable Facts

- All 14 repositories, the private trainer and sample repository included, were readable through
  `gh` on 2026-11-20. NEAT-AI-Explore, NEAT-AI-Refinery and NEAT-AI-Snapshot
  exist, and none of them writes a creature.
- The private sample repository at `fdcd279` holds 42 samples: 31 TypeScript-shaped, 10
  Rust-`Value`-then-jq-shaped, and one, `Enceladus-Terminal.json`, in a shape
  no audited writer produces.
- NEAT-AI's checkpoint (`CheckpointWriter.ts:224`) is compact, yet every `-1`
  sample is 1-space. The private trainer's `saveValidatedCreatureExport` re-indents only on a
  score mismatch, and the hop that re-indents when the scores agree was not
  traced.
- The earlier NEAT-AI audit's "only two writers / two `tags.rs` sidecars" no
  longer holds: Forests, Ockham and Rebase now write creatures too.

## Evidence

Documentation change plus one example binary; no UI.

- The doc's reproduction block, run on the final head:
  - before: `67 67`, `106 106` and `100 100`;
  - after: empty for all three (identical files, so 0 / 0).
- `cargo test -q -p neat-core --test creature_presentation`: 22 passed,
  including `every_variant_is_normalised_to_its_canonical_bytes`.
- The private sample repository numstats were confirmed with `git show --numstat`:
  - `a2a7d391`: 162179 / 161645 (the 10-forests sample);
  - `3b63ecaa`: 161712 / 161702;
  - `36811a57`: 115 / 33 (the 12-1 sample);
  - `e776c06f`: 6 / 2.
- Write-path citations were read in shallow clones at the SHAs in the doc. I
  re-read the load-bearing lines myself, including:
  - Backpropagation `tags.rs:208-211`, Lamarck `tags.rs:359-365`, Forests
    `run.rs:163-166`, Ockham `run.rs:2843-2847`, Rebase `cli.rs:653-657`;
  - The private trainer `format_creature_json.sh:55`, `creature_provenance_guard.sh:152` and
    the `format_creature_json` / commit lines in each `worker/<Extension>/run.sh`;
  - NEAT-AI `CheckpointWriter.ts:224`.
- Issue numbers cited: #765 (Specify the persisted creature JSON presentation
  contract) and #766 (Add the contract serialiser to neat-core and pin it to
  the golden corpus), both already in the doc. #767 is this issue.

## Acceptance Criteria

<!-- vibe-spec-review inputs="diff+issue-body" -->

- **met** — Every repository listed above has a row with a commit SHA and either a `file:line` write path or cited read-only evidence. A repository that does not exist or cannot be read is recorded as "not audited" with the reason, never given a verdict. — evidence: `docs/CREATURE_JSON_PRESENTATION.md` "Audited revisions" and "Write paths" tables — reviewer: met — reason: the reviewer's gaps are fixed in `8e26f15`: full NEAT-AI-core SHA, Ockham `incumbent.rs:272`, Lamarck `width.rs:83,94`, Backpropagation candidate shape, and the untraced `-1` hop stated
- **met** — Each writer's differences are classified into the categories above. — evidence: `docs/CREATURE_JSON_PRESENTATION.md` "Difference classes per writer" — reviewer: partial — reason: the reviewer found Rebase, the private trainer `creature_provenance_guard.sh` and `saveValidatedCreatureExport` unclassified, Forests' `memetic` removal unflagged and the jq cell ambiguous; all five are fixed in `8e26f15`
- **met** — The churn table gives added and deleted counts before and after for every committed variant shape, with commands that reproduce them from the tree. — evidence: "Churn on the committed corpus" table and command block; `neat-core/examples/creature_presentation_rewrite.rs` — reviewer: met
- **met** — Every "after" count is zero, and the table names the test in `neat-core/tests/creature_presentation.rs` that enforces it. — evidence: `neat-core/tests/creature_presentation.rs::every_variant_is_normalised_to_its_canonical_bytes` — reviewer: met
- **met** — The files changed in this repository are listed, and each change owned elsewhere names its owning repository. — evidence: "Files changed" subsection — reviewer: met
- **met** — `./quality.sh` passes (Markdown lint, spelling, Mermaid). — evidence: `./quality.sh < /dev/null` exit 0, "All quality checks passed!" — reviewer: missing — reason: the reviewer could not run the gate; it was run here on the tree committed as `8e26f15` and passed
- **unrequested** — `neat-core/examples/creature_presentation_rewrite.rs` — reviewer: unrequested — reason: the reviewer judged it traceable to "Give the exact commands"; without it the "after" column has no command
- **unrequested** — the Mermaid flowchart in the audit section and the H1 now naming #767 — reviewer: unrequested — reason: the diagram shows the write routes (the repository asks for diagrams on data flow); the heading names the issue the section records

## Standards Review

<!-- vibe-standards-review inputs="diff+CODING-STANDARDS.md" -->

- **clean** — Every area checked was clean.
  - The repository has no `CODING-STANDARDS.md`, so the review was against
    `AGENTS.md`.
  - Rules checked:
    - Australian English (review-enforced);
    - oracle rule 3 (before counts re-derived);
    - this repo's `file:line` citations;
    - the Mermaid gate;
    - width check via `validate_creature_width` (the example calls
      `creature_presentation_json`);
    - ownership fence;
    - no public-API change;
    - lints (no `unwrap`, fail-loud exits).
  - No existing test was edited.
  - Optional notes, all addressed:
    - short NEAT-AI-core SHA, now full;
    - `write_golden` wording, now "outside `#[cfg(test)]`";
    - TDD for the example: it is a thin wrapper over the tested
      `creature_presentation_json`, and `cargo test` and clippy compile it.

## Test Plan

- No test was added or edited.
- `neat-core/examples/creature_presentation_rewrite.rs` (new):
  - `cargo clippy -p neat-core --examples -- -D warnings` is clean;
  - it prints contract bytes for all three variants (empty numstat);
  - with no argument it exits 2 with a usage line; with a missing file it
    exits 1, naming the path.
- `cargo test -q -p neat-core --test creature_presentation`: 22 passed, on
  the final head.
- `./quality.sh < /dev/null`: exit 0, "All quality checks passed!" (81
  `test result: ok`), on the tree committed as `8e26f15`.

Not done or not checked:

- the `-1` re-indent hop when the scores agree;
- what writes `Enceladus-Terminal.json`;
- the wasm32 build (`rustup` is not installed here);
- the doc's row cells for other repositories rest on reads at the pinned SHAs
  and go stale as those repositories move.
