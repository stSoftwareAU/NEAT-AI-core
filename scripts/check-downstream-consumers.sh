#!/bin/bash
# check-downstream-consumers.sh — compile every registered downstream consumer
# against this checkout of neat-core (Issue #644).
#
# Why: #633 made CompiledNetwork's fields private. It was signalled correctly
# and merged with no consumer migrated. The production fleet builds the Rust
# consumers from the sibling NEAT-AI-core at head, so rust_scorer stopped
# compiling on every host within minutes; the consumer list in RELEASING.md was
# prose and named two of the six repositories that broke. This script is the
# machine-checked list: a core change that stops a registered consumer
# compiling fails here, before it can merge.
#
# Usage:
#   check-downstream-consumers.sh [--registry FILE] [--core DIR] [--ref REF]
#                                 [--workspace DIR] [--keep] [--list]
#
#   Default (CI) mode clones each registered repository at REF (default
#   Develop), shallowly, into a temporary workspace beside a symlink to the
#   candidate core, then runs `cargo check --workspace --all-targets` in each.
#
#   --workspace DIR  Use the sibling checkouts already under DIR instead of
#                    cloning — the local shape, e.g. `--workspace ..` from this
#                    repo. DIR/NEAT-AI-core must be the same directory as
#                    --core, so the consumers compile against the core you are
#                    editing and not some other clone.
#   --core DIR       The neat-core checkout under test (default: this repo).
#   --ref REF        Branch to clone each consumer at (default: Develop).
#   --keep           Keep the temporary workspace and its logs.
#   --list           Validate the registry, print its entries, and exit.
#
# Consumers are compiled with RUSTFLAGS unset: a warning a consumer already
# carries, or the `#[deprecated]` a phase-1 core PR adds, must not fail this
# gate — only a consumer that no longer compiles does.
#
# Every consumer is checked even after one fails, so a PR sees its whole blast
# radius at once instead of one repository per push.
#
# Exit codes:
#   0  every registered consumer compiles against this core
#   1  at least one consumer no longer compiles, could not be cloned, or is
#      missing from --workspace
#   2  usage or registry error
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
REGISTRY="$SCRIPT_DIR/downstream-consumers.txt"
CORE="$REPO_ROOT"
REF="Develop"
WORKSPACE=""
KEEP=0
LIST=0
POLICY='RELEASING.md, "Changing or removing public API: the three-phase flow"'

usage() {
  sed -n '2,/^set -euo pipefail/p' "${BASH_SOURCE[0]}" | sed '$d' | sed 's/^# \{0,1\}//'
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --registry) REGISTRY="${2:?--registry needs a path}"; shift 2 ;;
    --core) CORE="${2:?--core needs a path}"; shift 2 ;;
    --ref) REF="${2:?--ref needs a branch}"; shift 2 ;;
    --workspace) WORKSPACE="${2:?--workspace needs a path}"; shift 2 ;;
    --keep) KEEP=1; shift ;;
    --list) LIST=1; shift ;;
    -h | --help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

# --- registry ---------------------------------------------------------------
[[ -f "$REGISTRY" ]] || { echo "❌ registry not found: $REGISTRY" >&2; exit 2; }
entries=()
lineno=0
while IFS= read -r line || [[ -n "$line" ]]; do
  lineno=$((lineno + 1))
  line="${line%%#*}"
  line="${line//[[:space:]]/}"
  [[ -n "$line" ]] || continue
  if [[ ! "$line" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]]; then
    echo "❌ $REGISTRY:$lineno: expected owner/name, got '$line'" >&2
    exit 2
  fi
  for known in "${entries[@]+"${entries[@]}"}"; do
    if [[ "$known" == "$line" ]]; then
      echo "❌ $REGISTRY:$lineno: duplicate entry '$line'" >&2
      exit 2
    fi
  done
  entries+=("$line")
done <"$REGISTRY"
if (( ${#entries[@]} == 0 )); then
  echo "❌ $REGISTRY lists no consumers — an empty registry protects nothing" >&2
  exit 2
fi

if (( LIST )); then
  printf '%s\n' "${entries[@]}"
  exit 0
fi

# --- core under test ---------------------------------------------------------
if [[ ! -f "$CORE/neat-core/Cargo.toml" ]]; then
  echo "❌ --core $CORE is not a NEAT-AI-core checkout (no neat-core/Cargo.toml)" >&2
  exit 2
fi
CORE="$(cd "$CORE" && pwd -P)"

# --- workspace ---------------------------------------------------------------
if [[ -n "$WORKSPACE" ]]; then
  [[ -d "$WORKSPACE" ]] || { echo "❌ --workspace $WORKSPACE is not a directory" >&2; exit 2; }
  WORKSPACE="$(cd "$WORKSPACE" && pwd -P)"
  ws_core="$WORKSPACE/NEAT-AI-core"
  if [[ ! -d "$ws_core" ]] || [[ "$(cd "$ws_core" && pwd -P)" != "$CORE" ]]; then
    echo "❌ $ws_core must be the core under test ($CORE): every consumer resolves neat-core through ../../NEAT-AI-core" >&2
    exit 2
  fi
  MODE=workspace
else
  WORKSPACE="$(mktemp -d "${TMPDIR:-/tmp}/neat-core-downstream.XXXXXX")"
  ln -s "$CORE" "$WORKSPACE/NEAT-AI-core"
  MODE=clone
  if (( ! KEEP )); then
    trap 'rm -rf "$WORKSPACE"' EXIT
  fi
fi
LOG_DIR="$WORKSPACE/.downstream-logs"
mkdir -p "$LOG_DIR"

# A warning the consumer already carries — or the #[deprecated] this PR adds —
# is not a break. Only "does not compile" fails the gate.
unset RUSTFLAGS

echo "🧩 ${#entries[@]} registered downstream consumer(s), core under test: $CORE ($MODE mode, workspace $WORKSPACE)"

# --- materialise every consumer before checking any, so sibling-to-sibling
# path dependencies (Forests → Rebase) resolve regardless of registry order.
failed=()
for entry in "${entries[@]}"; do
  name="${entry##*/}"
  dir="$WORKSPACE/$name"
  if [[ "$MODE" == clone ]]; then
    [[ -d "$dir" ]] && continue
    echo "⬇️  $entry@$REF"
    if ! git clone --quiet --depth 1 --branch "$REF" "https://github.com/$entry.git" "$dir" >"$LOG_DIR/$name.clone.log" 2>&1; then
      echo "❌ $entry: clone of $REF failed"
      sed 's/^/    /' "$LOG_DIR/$name.clone.log" | tail -n 5
      failed+=("$entry (clone failed)")
    fi
  elif [[ ! -f "$dir/Cargo.toml" ]]; then
    echo "❌ $entry: no checkout at $dir — clone it beside NEAT-AI-core, or remove it from $REGISTRY if it no longer takes the path dependency"
    failed+=("$entry (missing from workspace)")
  fi
done

# --- compile each consumer against the core under test ----------------------
for entry in "${entries[@]}"; do
  name="${entry##*/}"
  dir="$WORKSPACE/$name"
  [[ -f "$dir/Cargo.toml" ]] || continue # reported above
  log="$LOG_DIR/$name.log"
  started=$SECONDS
  echo "🔧 $entry: cargo check --workspace --all-targets"
  if (cd "$dir" && cargo check --workspace --all-targets) >"$log" 2>&1; then
    echo "✅ $entry compiles against this core ($((SECONDS - started))s)"
  else
    echo "❌ $entry no longer compiles against this core ($((SECONDS - started))s) — log: $log"
    grep -E '^error' "$log" | head -n 20 | sed 's/^/    /'
    failed+=("$entry")
  fi
done

if (( ${#failed[@]} > 0 )); then
  echo
  echo "❌ ${#failed[@]} of ${#entries[@]} registered consumers broke:"
  printf '   %s\n' "${failed[@]}"
  cat <<MSG

   This is a working system: a change that breaks a known dependency does not
   merge, however good the idea. Add the alternative in core first, migrate
   every registered consumer to it, and only then remove or privatise the old
   surface — $POLICY.
MSG
  exit 1
fi
echo "✅ all ${#entries[@]} registered consumers compile against this core"
