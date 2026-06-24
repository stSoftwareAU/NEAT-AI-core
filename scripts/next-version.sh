#!/bin/bash
# next-version.sh — compute the next semver per the NEAT-AI-core release policy.
#
# Usage: next-version.sh <current_version> <is_breaking:true|false>
#
# Policy (see RELEASING.md, Issue #251):
#   * breaking change  -> major-equivalent bump
#                         pre-1.0 (major == 0): bump MINOR, reset patch
#                         post-1.0:             bump MAJOR, reset minor+patch
#   * non-breaking     -> bump PATCH
#
# Prints the next version to stdout. Exits non-zero on bad input.
set -euo pipefail

die() {
  echo "next-version.sh: $1" >&2
  exit 1
}

[ "$#" -eq 2 ] || die "usage: next-version.sh <current_version> <is_breaking:true|false>"

current="${1#v}" # tolerate a leading v
breaking="$2"

case "$breaking" in
  true | false) ;;
  *) die "is_breaking must be 'true' or 'false', got: $breaking" ;;
esac

# Strict x.y.z (digits only); reject pre-release/build metadata for clarity.
if ! printf '%s' "$current" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  die "malformed version (expected x.y.z): $current"
fi

IFS='.' read -r major minor patch <<<"$current"

if [ "$breaking" = "true" ]; then
  if [ "$major" -eq 0 ]; then
    minor=$((minor + 1))
    patch=0
  else
    major=$((major + 1))
    minor=0
    patch=0
  fi
else
  patch=$((patch + 1))
fi

echo "${major}.${minor}.${patch}"
