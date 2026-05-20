#!/usr/bin/env bats
# Regression assertion for Issue #76 — every CI dependency-bump path must go
# through bump-deps.sh so the VIBE_BUMP_QUARANTINE_HOURS release-age quarantine
# is honoured. Direct `cargo upgrade` / `cargo update` invocations bypass the
# quarantine and re-open the supply-chain window the script was written to
# close.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  CI_WF="${REPO_ROOT}/.github/workflows/ci.yml"
  UPGRADE_WF="${REPO_ROOT}/.github/workflows/upgrade-dependencies.yml"
}

# Strip YAML comments and fenced-script comments so an assertion such as
# "no unguarded `cargo upgrade`" cannot be defeated by leaving the original
# command in a comment for nostalgia.
strip_comments() {
  # Remove everything from the first `#` on each line that is not inside a
  # quoted string. Good-enough heuristic for our workflow YAML.
  sed -E 's/[[:space:]]*#.*$//' "$1"
}

@test "ci.yml version-increment job invokes bump-deps.sh" {
  [ -f "$CI_WF" ]
  run grep -q 'bump-deps.sh' "$CI_WF"
  [ "$status" -eq 0 ]
}

@test "ci.yml does not call cargo upgrade directly (bypasses quarantine)" {
  [ -f "$CI_WF" ]
  stripped="$(strip_comments "$CI_WF")"
  if printf '%s\n' "$stripped" | grep -E '^[^#]*cargo[[:space:]]+upgrade([[:space:]]|$)' >/dev/null; then
    printf 'Unguarded cargo upgrade found in ci.yml:\n%s\n' \
      "$(printf '%s\n' "$stripped" | grep -nE 'cargo[[:space:]]+upgrade([[:space:]]|$)')" >&2
    return 1
  fi
}

@test "ci.yml does not call bare cargo update (bypasses quarantine)" {
  [ -f "$CI_WF" ]
  stripped="$(strip_comments "$CI_WF")"
  # `cargo update -p <crate> --precise <ver>` is fine — that is exactly what
  # bump-deps.sh emits internally after the quarantine check. We only forbid
  # the *bare* form `cargo update` (no -p).
  if printf '%s\n' "$stripped" | grep -E '(^|[[:space:]])cargo[[:space:]]+update([[:space:]]*$|[[:space:]]+(--dry-run|--locked|--offline|--manifest-path|--workspace)?[[:space:]]*$)' >/dev/null; then
    printf 'Bare cargo update found in ci.yml:\n%s\n' \
      "$(printf '%s\n' "$stripped" | grep -nE 'cargo[[:space:]]+update')" >&2
    return 1
  fi
}

@test "upgrade-dependencies.yml invokes bump-deps.sh" {
  [ -f "$UPGRADE_WF" ]
  run grep -q 'bump-deps.sh' "$UPGRADE_WF"
  [ "$status" -eq 0 ]
}

@test "upgrade-dependencies.yml does not call cargo upgrade directly" {
  [ -f "$UPGRADE_WF" ]
  stripped="$(strip_comments "$UPGRADE_WF")"
  if printf '%s\n' "$stripped" | grep -E '^[^#]*cargo[[:space:]]+upgrade([[:space:]]|$)' >/dev/null; then
    printf 'Unguarded cargo upgrade found in upgrade-dependencies.yml:\n%s\n' \
      "$(printf '%s\n' "$stripped" | grep -nE 'cargo[[:space:]]+upgrade([[:space:]]|$)')" >&2
    return 1
  fi
}

@test "upgrade-dependencies.yml does not call bare cargo update" {
  [ -f "$UPGRADE_WF" ]
  stripped="$(strip_comments "$UPGRADE_WF")"
  if printf '%s\n' "$stripped" | grep -E '(^|[[:space:]])cargo[[:space:]]+update([[:space:]]*$|[[:space:]]+(--dry-run|--locked|--offline|--manifest-path|--workspace)?[[:space:]]*$)' >/dev/null; then
    printf 'Bare cargo update found in upgrade-dependencies.yml:\n%s\n' \
      "$(printf '%s\n' "$stripped" | grep -nE 'cargo[[:space:]]+update')" >&2
    return 1
  fi
}

@test "ci.yml passes the VIBE_BUMP_QUARANTINE_HOURS knob to bump-deps.sh" {
  [ -f "$CI_WF" ]
  run grep -q 'VIBE_BUMP_QUARANTINE_HOURS' "$CI_WF"
  [ "$status" -eq 0 ]
}

@test "upgrade-dependencies.yml passes the VIBE_BUMP_QUARANTINE_HOURS knob" {
  [ -f "$UPGRADE_WF" ]
  run grep -q 'VIBE_BUMP_QUARANTINE_HOURS' "$UPGRADE_WF"
  [ "$status" -eq 0 ]
}
