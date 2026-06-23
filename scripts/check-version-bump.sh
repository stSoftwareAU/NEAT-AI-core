#!/bin/bash
# check-version-bump.sh — enforce the NEAT-AI-core breaking-change semver gate.
#
# Usage: check-version-bump.sh <base_version> <head_version> <is_breaking:true|false>
#
# Ensures a *breaking* change cannot ship on a patch-only bump (Issue #251):
#   * a downgrade (head < base) is always rejected;
#   * a breaking change requires a major-equivalent bump — pre-1.0 the MINOR
#     must increase (0.1.x -> 0.2.0), post-1.0 the MAJOR must increase;
#   * a non-breaking change only needs head >= base (over-bumping is allowed).
#
# Exits 0 when the bump satisfies the policy, non-zero (with a message) otherwise.
set -euo pipefail

die() {
  echo "check-version-bump.sh: $1" >&2
  exit 1
}

[ "$#" -eq 3 ] || die "usage: check-version-bump.sh <base_version> <head_version> <is_breaking:true|false>"

base="${1#v}"
head="${2#v}"
breaking="$3"

case "$breaking" in
  true | false) ;;
  *) die "is_breaking must be 'true' or 'false', got: $breaking" ;;
esac

for v in "$base" "$head"; do
  if ! printf '%s' "$v" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    die "malformed version (expected x.y.z): $v"
  fi
done

IFS='.' read -r b_major b_minor b_patch <<<"$base"
IFS='.' read -r h_major h_minor h_patch <<<"$head"

# Numeric comparison helper: echoes -1, 0 or 1 for a<=>b on the (major,minor,patch) tuples.
cmp_triple() {
  local am=$1 an=$2 ap=$3 bm=$4 bn=$5 bp=$6
  if [ "$am" -ne "$bm" ]; then [ "$am" -gt "$bm" ] && echo 1 || echo -1; return; fi
  if [ "$an" -ne "$bn" ]; then [ "$an" -gt "$bn" ] && echo 1 || echo -1; return; fi
  if [ "$ap" -ne "$bp" ]; then [ "$ap" -gt "$bp" ] && echo 1 || echo -1; return; fi
  echo 0
}

order=$(cmp_triple "$h_major" "$h_minor" "$h_patch" "$b_major" "$b_minor" "$b_patch")

if [ "$order" -lt 0 ]; then
  die "version downgraded: $base -> $head"
fi

if [ "$breaking" = "true" ]; then
  # Major-equivalent bump. Pre-1.0 (base major 0) the minor (or a move to >=1.0)
  # must increase; post-1.0 the major must increase.
  if [ "$b_major" -eq 0 ]; then
    if [ "$h_major" -eq 0 ] && [ "$h_minor" -eq "$b_minor" ]; then
      die "breaking change requires a major-equivalent bump (pre-1.0: minor), but $base -> $head is only a patch-level (or no) bump"
    fi
  else
    if [ "$h_major" -eq "$b_major" ]; then
      die "breaking change requires a major-equivalent bump (post-1.0: major), but $base -> $head does not increase the major"
    fi
  fi
fi

echo "check-version-bump.sh: OK ($base -> $head, breaking=$breaking)"
