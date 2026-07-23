#!/bin/bash
# typescript-check.sh — basic-validity gate for TypeScript sources (Issue #307).
#
# Usage: typescript-check.sh [root-dir]
#
# Type-checks every .ts file under <root-dir> (default: the repository root)
# with `deno check`, so a syntax or type error fails the build instead of
# landing on Develop unnoticed. Basic validity only — this is not a style or
# lint gate.
#
# This repository commits its own gate; there is no shared cross-repo Action.
set -euo pipefail

# Fail loud: a missing toolchain must never be reconciled as a passing gate.
# Checked first so the diagnostic survives a stripped PATH.
if ! command -v deno &>/dev/null; then
  echo "typescript-check: deno is required — install: https://docs.deno.com/runtime/getting_started/installation/" >&2
  exit 1
fi

root="${1:-$(cd "${BASH_SOURCE[0]%/*}/.." && pwd)}"

if [ ! -d "$root" ]; then
  echo "typescript-check: not a directory: $root" >&2
  exit 2
fi

files=()
while IFS= read -r file; do
  files+=("$file")
done < <(find "$root" -name '*.ts' -type f \
  -not -path '*/target/*' \
  -not -path '*/.git/*' \
  -not -path '*/node_modules/*' | sort)

if [ "${#files[@]}" -eq 0 ]; then
  echo "typescript-check: no TypeScript files found under $root — nothing to check"
  exit 0
fi

echo "typescript-check: checking ${#files[@]} TypeScript file(s) with deno check"
deno check "${files[@]}" </dev/null
echo "typescript-check: all TypeScript files passed basic validity"
