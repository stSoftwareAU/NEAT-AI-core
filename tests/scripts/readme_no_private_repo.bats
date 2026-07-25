#!/usr/bin/env bats
# Tests that README.md names no private sibling repository (Issue #373).
#
# Rationale: NEAT-AI-core is public; the GRQ repository is private. The repo's
# front page must be fully self-contained for a public reader — naming a
# private sibling repo points every public reader at something they cannot
# open, verify, or reproduce, and advertises the private project's internals.
# This is check 3 of the private-repo-reference audit (textual private-repo
# name mention).
#
# These are "what" tests: they read the published README artefact and assert on
# the observable outcome (no private repo name appears), not on how the prose
# that replaced it is phrased.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  README="${REPO_ROOT}/README.md"
}

@test "README.md exists" {
  [ -f "$README" ]
}

@test "README.md prose does not name the private GRQ repository" {
  [ -f "$README" ]
  # The private repo name is always written in prose as the uppercase token
  # 'GRQ' ('GRQ', 'GRQ-side', 'GRQ's). Match it case-sensitively as a whole
  # word so those prose mentions are caught. The lowercase 'grq' slug inside
  # the deferred research-doc filename is out of scope here — renaming that
  # local artefact is tracked as a separate private-repo-reference finding.
  run grep -nw "GRQ" "$README"
  [ "$status" -ne 0 ]
}
