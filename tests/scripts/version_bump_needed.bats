#!/usr/bin/env bats
# Tests for scripts/version-bump-needed.sh (PR #619).
#
# The `version-increment` job used to skip the semver bump whenever an auto-bump
# commit was already on the branch, which left a milestone branch carrying a
# breaking commit on a patch-only version once Develop caught up — exactly the
# state the `version-gate` job rejects. The decision now keys off the versions
# and the breaking signal, which this script owns.
#
# These are "what" tests: they run the real script and assert on its output.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  SCRIPT="${REPO_ROOT}/scripts/version-bump-needed.sh"
}

@test "regression: breaking change on an unchanged version still needs a bump" {
  run "$SCRIPT" "0.10.10" "0.10.10" true
  [ "$status" -eq 0 ]
  [ "$output" = "true" ]
}

@test "regression: breaking change on a patch-only bump still needs a bump" {
  run "$SCRIPT" "0.10.10" "0.10.11" true
  [ "$status" -eq 0 ]
  [ "$output" = "true" ]
}

@test "breaking change already on a minor bump needs no further bump" {
  run "$SCRIPT" "0.10.10" "0.11.0" true
  [ "$status" -eq 0 ]
  [ "$output" = "false" ]
}

@test "non-breaking change already ahead of base needs no further bump" {
  run "$SCRIPT" "0.10.10" "0.10.11" false
  [ "$status" -eq 0 ]
  [ "$output" = "false" ]
}

@test "non-breaking change level with base needs a bump" {
  run "$SCRIPT" "0.10.10" "0.10.10" false
  [ "$status" -eq 0 ]
  [ "$output" = "true" ]
}

@test "a branch behind base needs a bump" {
  run "$SCRIPT" "0.10.10" "0.10.9" false
  [ "$status" -eq 0 ]
  [ "$output" = "true" ]
}

@test "post-1.0 breaking change on a minor bump still needs a bump" {
  run "$SCRIPT" "1.2.3" "1.3.0" true
  [ "$status" -eq 0 ]
  [ "$output" = "true" ]
}

@test "post-1.0 breaking change on a major bump needs no further bump" {
  run "$SCRIPT" "1.2.3" "2.0.0" true
  [ "$status" -eq 0 ]
  [ "$output" = "false" ]
}

@test "an unreadable base version falls back to bumping" {
  run "$SCRIPT" "" "0.10.11" false
  [ "$status" -eq 0 ]
  [ "$output" = "true" ]
}

@test "a malformed head version falls back to bumping" {
  run "$SCRIPT" "0.10.10" "not-a-version" false
  [ "$status" -eq 0 ]
  [ "$output" = "true" ]
}

@test "a bad breaking flag is rejected" {
  run "$SCRIPT" "0.10.10" "0.10.11" maybe
  [ "$status" -ne 0 ]
}

@test "wrong argument count is rejected" {
  run "$SCRIPT" "0.10.10" "0.10.11"
  [ "$status" -ne 0 ]
}
