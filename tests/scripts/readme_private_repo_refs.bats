#!/usr/bin/env bats
# Tests that README.md does not name the private GRQ repository (Issue #373).
#
# Rationale: NEAT-AI-core is public; the GRQ repository is private. A public
# repo's front page must be fully self-contained — naming a private sibling
# repo points every public reader at something they cannot open, verify, or
# reproduce. This is check 3 of the private-repo-reference audit (textual
# private-repo name mention). The wasm64 lane (d) paragraph is reworded to
# concept level (e.g. "the production trainer's launch script") instead.
#
# These are "what" tests: they read the published README artefact and assert
# on the observable outcome (no private repo name is present), not on how the
# replacement wording is phrased.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  README="${REPO_ROOT}/README.md"
}

@test "README.md exists" {
  [ -f "$README" ]
}

@test "README.md does not name the private GRQ repository" {
  [ -f "$README" ]
  # Case-sensitive whole-word match on the repo name GRQ. The lowercase 'grq'
  # inside the research-doc filename link is a separate finding and is left as
  # is, so a case-sensitive match must not flag it.
  run grep -nE '\bGRQ\b' "$README"
  [ "$status" -ne 0 ]
}

@test "README.md does not reference the stSoftwareAU/GRQ path" {
  [ -f "$README" ]
  run grep -niF "stSoftwareAU/GRQ" "$README"
  [ "$status" -ne 0 ]
}
