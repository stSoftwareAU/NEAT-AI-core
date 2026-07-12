#!/usr/bin/env bats
# Tests that README.md defines its core terms on first use (Issue #262).
#
# Rationale: README.md is the declared source of truth. It uses an acronym
# (NEAT), a domain object (creature), an activation term (squash), and a
# playful internal name (Vibe Coder) without first-use definitions, so a
# newcomer — human or AI — cannot ground them. This is documentation check 7
# (undefined terms, acronyms, and playful names): standard terms should link
# out, project terms should carry a plain-English gloss.
#
# These are "what" tests: they read the published README artefact and assert
# on the observable outcome (each term is defined), not on how the definition
# is phrased or laid out.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  README="${REPO_ROOT}/README.md"
}

@test "README.md exists" {
  [ -f "$README" ]
}

@test "NEAT is expanded to NeuroEvolution of Augmenting Topologies" {
  [ -f "$README" ]
  # Match case-insensitively; the acronym must be spelled out at least once.
  run grep -iq "NeuroEvolution of Augmenting Topologies" "$README"
  [ "$status" -eq 0 ]
}

@test "creature is glossed as an evolved individual / genome" {
  [ -f "$README" ]
  run python3 - "$README" <<'PY'
import re, sys
text = open(sys.argv[1], encoding="utf-8").read().lower()
# A gloss line for creature must mention it is an individual genome/network.
if re.search(r"\*\*creature\*\*.*(genome|individual|network)", text):
    sys.exit(0)
sys.stderr.write("no plain-English gloss found for 'creature'\n")
sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

@test "squash is glossed as an activation function" {
  [ -f "$README" ]
  run python3 - "$README" <<'PY'
import re, sys
text = open(sys.argv[1], encoding="utf-8").read().lower()
if re.search(r"\*\*squash\*\*.*activation function", text):
    sys.exit(0)
sys.stderr.write("no gloss found for 'squash' as an activation function\n")
sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

@test "Vibe Coder is glossed as the automated PR agent" {
  [ -f "$README" ]
  run python3 - "$README" <<'PY'
import re, sys
text = open(sys.argv[1], encoding="utf-8").read().lower()
# The gloss must describe the Vibe Coder as an automated agent that raises PRs.
if re.search(r"\*\*vibe coder\*\*.*(automat|agent).*pr", text):
    sys.exit(0)
sys.stderr.write("no gloss found for 'Vibe Coder' as the automated PR agent\n")
sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

@test "a Glossary section exists in README.md" {
  [ -f "$README" ]
  run grep -Eiq "^#+[[:space:]]+Glossary" "$README"
  [ "$status" -eq 0 ]
}
