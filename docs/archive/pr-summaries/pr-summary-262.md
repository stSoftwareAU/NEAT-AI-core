## Summary

`README.md` is the declared source of truth but used several core terms
without a first-use definition, so a newcomer (human or AI) could not ground
them. This PR adds first-use definitions and links each first use to them.
Closes #262.

Changes:

- **New Glossary section** near the top of `README.md`:
  - **NEAT** — expanded to *NeuroEvolution of Augmenting Topologies* and linked
    to Wikipedia. Also expanded inline on the very first mention in the intro.
  - **creature** — glossed as the project's term for a single evolved
    individual: a genome compiled to a runnable network.
  - **squash** — glossed as a neuron's *activation function* (linked).
  - **Vibe Coder** — glossed as the automated agent that raises the routine
    dependency-bump and quality PRs (runs `bump-deps.sh` before `quality.sh`).
- **Linked each first use** in the body (`creature`, `squash`, `Vibe Coder`)
  back to its glossary anchor.

Standard terms link out; the project's own vocabulary carries a plain-English
gloss, per documentation check 7 (undefined terms, acronyms, playful names).

## Evidence

Documentation-only change — no web interface to screenshot. Verified with a new
bats "what" test suite that reads the published README artefact and asserts each
term is defined, plus the full existing gate:

```
$ bats tests/scripts/readme_glossary.bats
1..6
ok 1 README.md exists
ok 2 NEAT is expanded to NeuroEvolution of Augmenting Topologies
ok 3 creature is glossed as an evolved individual / genome
ok 4 squash is glossed as an activation function
ok 5 Vibe Coder is glossed as the automated PR agent
ok 6 a Glossary section exists in README.md

$ bats tests/scripts    # 178 tests, 0 failures
$ npx markdownlint-cli2 README.md   # 0 error(s)
$ codespell README.md               # clean
```

## Test Plan

- Added `tests/scripts/readme_glossary.bats` — "what" tests that parse
  `README.md` and assert on observable content:
  - NEAT is expanded to *NeuroEvolution of Augmenting Topologies*.
  - `creature`, `squash`, and `Vibe Coder` each carry a gloss.
  - A Glossary section exists.
- The suite failed against the pre-change README (5 of 6 failing) and passes
  after the README update, confirming it exercises the fix.
