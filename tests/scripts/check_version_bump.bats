#!/usr/bin/env bats
# Tests for scripts/check-version-bump.sh (Issue #251).
#
# The gate that ensures a *breaking* change cannot ship on a patch-only bump.
# It compares the base-branch version against the head version and the breaking
# signal, exiting non-zero when the bump is too small for a breaking change or
# when the version was downgraded.
#
# These are "what" tests: they run the real script and assert on its exit code.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  SCRIPT="${REPO_ROOT}/scripts/check-version-bump.sh"
}

@test "breaking change with a minor bump passes (pre-1.0)" {
  run "$SCRIPT" "0.1.46" "0.2.0" true
  [ "$status" -eq 0 ]
}

@test "breaking change with only a patch bump fails (pre-1.0)" {
  run "$SCRIPT" "0.1.46" "0.1.47" true
  [ "$status" -ne 0 ]
}

@test "breaking change with no version change fails" {
  run "$SCRIPT" "0.1.46" "0.1.46" true
  [ "$status" -ne 0 ]
}

@test "breaking change with a major bump passes (post-1.0)" {
  run "$SCRIPT" "1.4.2" "2.0.0" true
  [ "$status" -eq 0 ]
}

@test "breaking change with only a minor bump fails (post-1.0)" {
  run "$SCRIPT" "1.4.2" "1.5.0" true
  [ "$status" -ne 0 ]
}

@test "non-breaking change with a patch bump passes" {
  run "$SCRIPT" "0.1.46" "0.1.47" false
  [ "$status" -eq 0 ]
}

@test "non-breaking change with a minor bump passes (over-bumping is allowed)" {
  run "$SCRIPT" "0.1.46" "0.2.0" false
  [ "$status" -eq 0 ]
}

@test "non-breaking change with no version change passes" {
  run "$SCRIPT" "0.1.46" "0.1.46" false
  [ "$status" -eq 0 ]
}

@test "a downgrade is always rejected" {
  run "$SCRIPT" "0.2.0" "0.1.47" false
  [ "$status" -ne 0 ]
}

@test "a leading v is tolerated on both versions" {
  run "$SCRIPT" "v0.1.46" "v0.2.0" true
  [ "$status" -eq 0 ]
}

@test "missing arguments are rejected" {
  run "$SCRIPT" "0.1.46" "0.2.0"
  [ "$status" -ne 0 ]
}

@test "a malformed version is rejected" {
  run "$SCRIPT" "0.1" "0.2.0" true
  [ "$status" -ne 0 ]
}
