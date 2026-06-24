#!/usr/bin/env bats
# Tests for scripts/next-version.sh (Issue #251 — semver breaking-change signal).
#
# Policy (pre-1.0): a breaking change is a *major-equivalent* bump, which
# pre-1.0 means the MINOR (0.1.x -> 0.2.0); a non-breaking change bumps the
# PATCH (0.1.x -> 0.1.(x+1)). Post-1.0 a breaking change bumps the MAJOR.
#
# These are "what" tests: they run the real script and assert on the version
# string it prints, not on its source text.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  SCRIPT="${REPO_ROOT}/scripts/next-version.sh"
}

@test "non-breaking change bumps the patch (pre-1.0)" {
  run "$SCRIPT" "0.1.46" false
  [ "$status" -eq 0 ]
  [ "$output" = "0.1.47" ]
}

@test "breaking change bumps the minor pre-1.0 and resets patch" {
  run "$SCRIPT" "0.1.46" true
  [ "$status" -eq 0 ]
  [ "$output" = "0.2.0" ]
}

@test "breaking change bumps the major post-1.0 and resets minor+patch" {
  run "$SCRIPT" "1.4.2" true
  [ "$status" -eq 0 ]
  [ "$output" = "2.0.0" ]
}

@test "non-breaking change bumps the patch post-1.0" {
  run "$SCRIPT" "1.4.2" false
  [ "$status" -eq 0 ]
  [ "$output" = "1.4.3" ]
}

@test "a leading v on the input is tolerated" {
  run "$SCRIPT" "v0.1.46" false
  [ "$status" -eq 0 ]
  [ "$output" = "0.1.47" ]
}

@test "missing arguments are rejected" {
  run "$SCRIPT" "0.1.46"
  [ "$status" -ne 0 ]
}

@test "a malformed version is rejected" {
  run "$SCRIPT" "1.2" false
  [ "$status" -ne 0 ]
}
