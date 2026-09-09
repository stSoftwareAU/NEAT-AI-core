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

root="${1:-$(cd "${BASH_SOURCE[0]%/*}/.." && pwd)}"

if [ ! -d "$root" ]; then
  echo "typescript-check: not a directory: $root" >&2
  exit 2
fi

# The test above is not sufficient on its own: `[ -d "-P" ]` is true whenever a
# directory named `-P` exists in the cwd, and `find "-P"` would then parse it as
# find's own `-P` option and walk the cwd instead of the named tree. Resolve to
# an absolute path once, through a `cd --` that cannot be optioned — the same
# shape the default above already has (Issue #608).
root="$(cd -- "$root" && pwd)"

# Fail loud: a missing toolchain must never be reconciled as a passing gate.
# Probed after the argument is validated so a bad root is always reported as a
# usage error (exit 2), toolchain or no toolchain. Both checks use shell
# builtins only, so the diagnostics survive a stripped PATH.
if ! command -v deno &>/dev/null; then
  echo "typescript-check: deno is required — install: https://docs.deno.com/runtime/getting_started/installation/" >&2
  exit 1
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

# `deno check` discovers its config from the *cwd*, not from the files it is
# handed, so a run from anywhere but <root> resolves nothing through the import
# map and every mapped specifier fails as "not a dependency" (Issue #646). Name
# the config the tree actually commits, and pass no --config when there is none
# (the temp-tree cases in typescript_check.bats).
config=""
for candidate in "$root/deno.json" "$root/deno.jsonc"; do
  if [ -f "$candidate" ]; then
    config="$candidate"
    break
  fi
done

echo "typescript-check: checking ${#files[@]} TypeScript file(s) with deno check"
# Run from inside the tree being checked: Deno discovers deno.json — and with it
# the import map the helpers resolve `@std/assert` through — by walking up from
# the *current directory*, not from the files named on the command line. Called
# from anywhere else, every bare specifier failed as "not a dependency"
# (Issue #647). The paths in `files` are absolute, so the `cd` does not move
# them. Pass --config explicitly too when the tree commits one, so discovery
# does not depend on cwd alone (Issue #646).
cd -- "$root"
if [ -n "$config" ]; then
  echo "typescript-check: using config $config"
  deno check --config "$config" "${files[@]}" </dev/null
else
  deno check "${files[@]}" </dev/null
fi
echo "typescript-check: all TypeScript files passed basic validity"
