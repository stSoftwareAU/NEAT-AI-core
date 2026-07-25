#!/usr/bin/env bats
# Tests that README.md does not name the private sibling repository by name
# (Issue #373, private-repo-reference audit check 3).
#
# Rationale: NEAT-AI-core is public; its private trainer sibling is not. The
# public front page must be self-contained — it must not point readers at, or
# advertise the internals of, a repository they cannot open. The wasm64 lane (d)
# paragraph previously named the private repo ("GRQ") and its internal scripts
# directly; it must describe that behaviour at concept level instead. A milestone
# merge re-introduced the private name once already, so this guard pins the
# outcome against regression.
#
# These are "what" tests: they read the published README artefact and assert on
# the observable outcome (no private-repo name token is present), the same
# artefact-content style as tests/scripts/readme_glossary.bats — not a source
# grep of implementation detail.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  README="${REPO_ROOT}/README.md"
}

@test "README.md exists" {
  [ -f "$README" ]
}

@test "README.md does not name the private repository (GRQ token)" {
  [ -f "$README" ]
  # The private repo name is an uppercase token; match it on word boundaries so
  # unrelated substrings do not trip the check. A hit means a private-repo name
  # mention has crept back into the public front page.
  run grep -nE "\bGRQ\b" "$README"
  [ "$status" -ne 0 ]
}

@test "README.md does not reference the private trainer's internal scripts" {
  [ -f "$README" ]
  # worker/learn.sh and memory_calc.sh live in the private trainer repo; naming
  # them leaks private internals into the public README.
  run grep -nE "worker/learn\.sh|memory_calc\.sh" "$README"
  [ "$status" -ne 0 ]
}

@test "README.md does not reference the stSoftwareAU/GRQ path" {
  [ -f "$README" ]
  run grep -niF "stSoftwareAU/GRQ" "$README"
  [ "$status" -ne 0 ]
}
