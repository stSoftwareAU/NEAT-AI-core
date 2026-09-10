#!/usr/bin/env bats
# Regression assertion for Issue #662 (BP-d74b58af2374) — README.md, the root
# `Cargo.toml` and `tests/scripts/rust_build_profiles.bats` each cited a private
# `stSoftwareAU/VibeCoding` issue slug as the provenance of the Rust
# build-profile decision. NEAT-AI-core is public and that orchestration
# repository is not, so a public reader following the slug from the front page,
# the crate manifest or the committed test suite hits material they cannot open
# — private-repo-reference audit, check 3.
#
# This guard pins all three artefacts at concept level: the dev/release
# build-profile contract is stated inline, with no private repo token and no
# private issue slug. Same shape as the fix that closed #377 for bump-deps.sh.
#
# These are "what" tests over the committed artefacts — they read the published
# files and assert on the observable outcome, the same artefact-content style as
# tests/scripts/bump_deps_private_repo_reference.bats.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  README="${REPO_ROOT}/README.md"
  MANIFEST="${REPO_ROOT}/Cargo.toml"
  PROFILE_GATE="${REPO_ROOT}/tests/scripts/rust_build_profiles.bats"
  ARTEFACTS=("$README" "$MANIFEST" "$PROFILE_GATE")
}

@test "every build-profile artefact is present" {
  for artefact in "${ARTEFACTS[@]}"; do
    [ -f "$artefact" ] || {
      echo "missing artefact: $artefact"
      return 1
    }
  done
}

# --- No private repository name or slug --------------------------------------

@test "build-profile artefacts name no private orchestration repository" {
  # The private repo name is a token; match on word boundaries so unrelated
  # substrings do not trip the check.
  run grep -niw 'VibeCoding' "${ARTEFACTS[@]}"
  [ "$status" -ne 0 ]
}

@test "build-profile artefacts reference no private repository path or issue slug" {
  run grep -nE 'stSoftwareAU/VibeCoding|VibeCoding ?#[0-9]' "${ARTEFACTS[@]}"
  [ "$status" -ne 0 ]
}

# --- The concept-level build-profile contract survives -----------------------

@test "README still states the dev/release build-profile contract" {
  run grep -niE 'dev builds compile as fast as' "$README"
  [ "$status" -eq 0 ]
  run grep -niE 'most optimised artefact possible' "$README"
  [ "$status" -eq 0 ]
}

@test "Cargo.toml still states why dev builds keep only line tables" {
  run grep -niE 'dev builds compile' "$MANIFEST"
  [ "$status" -eq 0 ]
}

@test "the build-profile gate still states the contract it enforces" {
  run grep -niE 'dev builds compile as fast as' "$PROFILE_GATE"
  [ "$status" -eq 0 ]
}
