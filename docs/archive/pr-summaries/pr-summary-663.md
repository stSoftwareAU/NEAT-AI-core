# Reword archived incident reports to concept level

## Summary

Three archived incident reports under `docs/archive/pr-summaries/` named the
private downstream production trainer's repository, its issue numbers and its
internal sampler fixture paths directly. NEAT-AI-core is public, so a reader
following those pointers hit an issue tracker and fixture paths they cannot
open, and learned the layout of a private production system — check 3 of the
private-repo-reference audit, the same pattern already closed for the docs in
#373–#378.

The enumerated mentions now read at concept level ("a downstream
production-trainer issue", "a production sampler fixture"), preserving the
historical narrative and every technical detail — the reported error strings,
the drift measurements, the mutation tables and the test names are untouched.

Two of the three carried the private repository name in their **file name**
(`pr-summary-<private-repo>-NNNN.md`), so the private issue slug stayed
reconstructable from the path even once the prose was reworded. Those two were
renamed to concept-level names — `pr-summary-memetic-weight-forms.md` and
`pr-summary-exact-float-parsing.md` — via `git mv`, so their history follows.
Nothing in the repository linked either path (`grep -rn` over the tree at the
old names returns no matches outside the files themselves), so no inbound
reference needed updating.

Out of scope and already tracked elsewhere: the same private issue references
in `neat-core` source comments are #661, and the archived audit-fix summaries
that quote the private tokens as part of their own reword tables are #664.

Closes #663.

## Evidence

Documentation-only change — no web interface to screenshot, and no runtime
code touched. The evidence is the guard suite over the committed artefacts.

```console
$ bats tests/scripts/archive_pr_summaries_private_repo_reference.bats
1..13
ok 1 every enumerated archived PR summary is present
...
ok 10 no archived PR summary file name embeds the private repository name
ok 11 the memetic-weight-forms report still records the reported parse failure
ok 12 the exact-float-parsing report still records the 1 ULP drift
ok 13 pr-summary-572 still records the same-role fan-in rule it pinned
```

Red before the reword (the four new/extended assertions failed: the two renamed
paths were missing, the file-name guard matched two archives, and the two
narrative-survival greps found nothing), green after it.

`./quality.sh` — `✅ All quality checks passed!` (bash syntax, shellcheck, bats,
TypeScript gate, Mermaid gate, codespell, rustfmt, clippy `-D warnings`,
`cargo test --workspace`, doctests, rustdoc, release build).

## Test Plan

Extended `tests/scripts/archive_pr_summaries_private_repo_reference.bats` — the
existing #378 guard, rather than a second file that would duplicate its
patterns:

- The three reports join the enumerated `SUMMARIES` list, so the existing
  private-name, private-slug and private-path assertions now cover them.
- **New** `no archived PR summary file name embeds the private repository name`
  — the path-level half of the finding. `find` exits 0 on an empty result, so
  the assertion is on the emptiness of the output, not on the exit status.
- **New** narrative-survival assertions, one per reworded report: the reported
  parse-failure string, the 1 ULP drift bit pattern, and the same-role fan-in
  test name. These are what stop a future reword from deleting the history
  instead of generalising it.
