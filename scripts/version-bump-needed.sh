#!/bin/bash
# version-bump-needed.sh — decide whether the PR still owes a version bump.
#
# Usage: version-bump-needed.sh <base_version> <head_version> <is_breaking:true|false>
#
# Echoes "false" only when the head version already satisfies the release policy
# for the detected breaking signal — it differs from the base version and passes
# `check-version-bump.sh` — otherwise "true".
#
# The `version-increment` job used to skip the bump whenever an auto-bump commit
# was already on the branch. That is not proof the version is big enough: a
# milestone branch collects patch bumps from its sub-PRs, then acquires a
# breaking commit, and Develop can catch up to the same version — leaving the PR
# on a no-op bump that the `version-gate` job then rejected (PR #619). The
# decision therefore keys off the versions and the breaking signal, never off
# commit subjects.
set -euo pipefail

die() {
  echo "version-bump-needed.sh: $1" >&2
  exit 1
}

[ "$#" -eq 3 ] || die "usage: version-bump-needed.sh <base_version> <head_version> <is_breaking:true|false>"

base="$1"
head="$2"
breaking="$3"

case "$breaking" in
  true | false) ;;
  *) die "is_breaking must be 'true' or 'false', got: $breaking" ;;
esac

# No readable base version (base branch missing): nothing to compare against, so
# the caller must bump rather than silently ship the current version.
if [ -z "$base" ] || [ -z "$head" ] || [ "$base" = "$head" ]; then
  echo true
  exit 0
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if bash "$script_dir/check-version-bump.sh" "$base" "$head" "$breaking" >/dev/null 2>&1; then
  echo false
else
  echo true
fi
