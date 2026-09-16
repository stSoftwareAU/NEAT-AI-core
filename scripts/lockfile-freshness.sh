#!/bin/bash
# Issue #695 — every committed Cargo.lock must resolve against the manifest
# beside it.
#
# `wasm-bench` sits outside the root virtual workspace (Issue #607) and reaches
# `neat-core` through a path dependency, so the version it locks *is* the
# workspace version. The CI `version-increment` job rewrites that version and
# `bump-deps.sh` re-locks the root lockfile only, which left every other
# committed lockfile naming the superseded version — one bump of staleness per
# merge, and a hard failure the moment any gate reads that manifest with
# `--locked`.
#
#   --check  (default)  every lockfile in the tree resolves under `--locked`
#   --update            re-lock the siblings a version bump staled
#
# The sweep is `find . -name Cargo.lock`, so a third lockfile added later is
# covered without being wired in by hand. `cargo update --workspace` is the
# re-lock verb: it re-resolves the local path packages and leaves remote
# versions alone, so the release-age quarantine (Issue #76) that chose them
# survives the re-lock.
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/lockfile-freshness.sh [--check|--update] [--root <dir>]

  --check   (default) fail when a committed Cargo.lock no longer resolves
            against the manifest beside it
  --update  re-lock every committed Cargo.lock (local path packages only)
  --root    sweep <dir> instead of the repository root
USAGE
}

MODE="check"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

while [ $# -gt 0 ]; do
  case "$1" in
    --check) MODE="check" ;;
    --update) MODE="update" ;;
    --root)
      shift
      [ $# -gt 0 ] || {
        echo "--root needs a directory" >&2
        exit 2
      }
      ROOT="$1"
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

if [ ! -d "$ROOT" ]; then
  echo "no such directory: $ROOT" >&2
  exit 2
fi
ROOT="$(cd "$ROOT" && pwd)"

command -v cargo &>/dev/null || {
  echo "cargo is required — install: https://rustup.rs" >&2
  exit 1
}

# Sorted so the report is stable. Read with a while-loop rather than `mapfile`,
# which bash 3.2 (macOS) does not have.
LOCKFILES=()
while IFS= read -r lockfile; do
  LOCKFILES+=("$lockfile")
done < <(
  find "$ROOT" -name Cargo.lock -not -path '*/target/*' -not -path '*/.git/*' |
    LC_ALL=C sort
)

# An empty sweep is a failure, not a pass: it means the tree moved or the find
# expression stopped matching, and either way nothing was checked.
if [ "${#LOCKFILES[@]}" -eq 0 ]; then
  echo "no Cargo.lock found under ${ROOT} — nothing was checked" >&2
  exit 1
fi

FAILED=()
for lockfile in "${LOCKFILES[@]}"; do
  manifest="${lockfile%Cargo.lock}Cargo.toml"
  relative="${lockfile#"${ROOT}"/}"
  if [ ! -f "$manifest" ]; then
    echo "  ✗ ${relative}: no manifest at ${manifest#"${ROOT}"/}" >&2
    FAILED+=("$relative")
    continue
  fi
  case "$MODE" in
    check)
      echo "  lockfile: ${relative}"
      if ! cargo metadata --format-version 1 --locked \
        --manifest-path "$manifest" >/dev/null; then
        echo "  ✗ ${relative} no longer resolves ${manifest#"${ROOT}"/}" >&2
        FAILED+=("$relative")
      fi
      ;;
    update)
      echo "  re-locking: ${relative}"
      if ! cargo update --workspace --manifest-path "$manifest"; then
        echo "  ✗ ${relative} could not be re-locked" >&2
        FAILED+=("$relative")
      fi
      ;;
  esac
done

if [ "${#FAILED[@]}" -ne 0 ]; then
  echo "lockfile-freshness (${MODE}): FAILED for ${FAILED[*]}" >&2
  echo "re-lock them with: scripts/lockfile-freshness.sh --update" >&2
  exit 1
fi

echo "lockfile-freshness (${MODE}): ${#LOCKFILES[@]} lockfile(s) OK"
