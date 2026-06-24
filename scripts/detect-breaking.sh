#!/bin/bash
# detect-breaking.sh — detect a breaking-change signal in a git commit range.
#
# Usage: detect-breaking.sh <git-range>
#
# Echoes "true" when any commit in <git-range> carries a Conventional Commit
# breaking marker — a `type!:` / `type(scope)!:` subject, or a
# `BREAKING CHANGE:` / `BREAKING-CHANGE:` footer — otherwise "false".
# (The CI jobs additionally OR in the `breaking-change` PR label.) See Issue #251.
set -euo pipefail

[ "$#" -eq 1 ] || {
  echo "usage: detect-breaking.sh <git-range>" >&2
  exit 2
}
range="$1"

if git log --format='%s' "$range" | grep -Eq '^[a-zA-Z]+(\([^)]*\))?!:'; then
  echo true
  exit 0
fi
if git log --format='%B' "$range" | grep -Eq '(^|[[:space:]])BREAKING[ -]CHANGE:'; then
  echo true
  exit 0
fi
echo false
