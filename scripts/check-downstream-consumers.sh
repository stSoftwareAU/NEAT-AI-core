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
#   In clone mode a consumer that pins `neat-core` by git tag rather than by
#   path has a `[patch."<its git url>"] neat-core = { path = "<candidate>/neat-core" }`
#   appended to its root manifest before `cargo check`, so the gate compiles it
#   against the candidate core and not against the release it pins (Issue #681).
#   The patch is keyed by the URL the consumer actually declares, so a pin the
#   gate cannot override fails the gate instead of quietly compiling the
#   released core. A consumer still on the `../../NEAT-AI-core/neat-core` path
#   dependency declares no such URL and is left exactly as it was.
#
#   --workspace DIR  Use the sibling checkouts already under DIR instead of
#                    cloning — the local shape, e.g. `--workspace ..` from this
#                    repo. DIR/NEAT-AI-core must be the same directory as
#                    --core, so the consumers compile against the core you are
#                    editing and not some other clone. The checkouts under DIR
#                    are yours, so nothing is written to them: this mode is for
#                    consumers that still carry the path dependency.
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

# --- [patch] injection ------------------------------------------------------
# The core's own repository URL as an ERE, with the optional `.git` suffix and
# nothing after it: an unterminated match would also claim a future
# NEAT-AI-core-utils and emit a [patch] cargo rejects outright.
CORE_REPO_URL_RE='https://github[.]com/stSoftwareAU/NEAT-AI-core(\.git)?'

# The distinct git URLs through which the checkout $1 declares neat-core, one
# per line. Empty for a consumer still on the path dependency.
consumer_core_git_urls() {
  local found
  found="$(find "$1" -name Cargo.toml -not -path '*/target/*' \
    -exec grep -hoE "git[[:space:]]*=[[:space:]]*\"${CORE_REPO_URL_RE}\"" {} + 2>/dev/null || true)"
  [[ -n "$found" ]] || return 0
  printf '%s\n' "$found" | sed -e 's/.*"\(.*\)"/\1/' | sort -u
  return 0
}

# Append a `[patch]` override to the root manifest of consumer checkout $2, so
# every git-tag pin of neat-core it declares resolves to the candidate core.
# Returns non-zero — having said why — when the override cannot be trusted to
# take effect, because a gate that compiled the released core would be green
# for a core it never looked at.
inject_core_patch() {
  local entry="$1" dir="$2" manifest="$2/Cargo.toml" urls url count=0
  urls="$(consumer_core_git_urls "$dir")"
  if [[ -z "$urls" ]]; then
    return 0 # still on the path dependency — nothing to override
  fi
  if grep -q "^\[patch\..*NEAT-AI-core" "$manifest"; then
    echo "❌ $entry: its root manifest already declares a [patch] for neat-core — this gate cannot prove which core it compiled"
    return 1
  fi
  {
    printf '\n# Appended by scripts/check-downstream-consumers.sh (Issue #681):\n'
    printf '# compile against the candidate core, not the release this consumer pins.\n'
    while IFS= read -r url; do
      [[ -n "$url" ]] || continue
      printf '[patch."%s"]\nneat-core = { path = "%s/neat-core" }\n' "$url" "$CORE"
      count=$((count + 1))
    done <<EOF
$urls
EOF
  } >>"$manifest" || {
    echo "❌ $entry: could not append the [patch] override to $manifest"
    return 1
  }
  patched+=("$entry")
  echo "🩹 $entry: [patch] $count git pin(s) of neat-core → $CORE/neat-core"
  return 0
}

# True when consumer $1 had a [patch] appended, so cargo must have used it.
was_patched() {
  local known
  for known in ${patched[@]+"${patched[@]}"}; do
    [[ "$known" != "$1" ]] || return 0
  done
  return 1
}

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
# The candidate path is written into a TOML string below; a quote or backslash
# in it would produce a manifest cargo reads as something else entirely.
case "$CORE" in
  *[\"\\]*)
    echo "❌ --core $CORE contains a quote or a backslash — it cannot be written into a [patch] override" >&2
    exit 2
    ;;
esac

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
skipped=()
patched=()
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
    elif ! inject_core_patch "$entry" "$dir"; then
      # The clone is there but the gate cannot point it at the candidate core,
      # so it must not be compiled and reported green against a released one.
      # The clone is left intact for --keep to inspect; it is skipped by name.
      skipped+=("$entry")
      failed+=("$entry ([patch] injection failed)")
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
  skip=0
  for known in ${skipped[@]+"${skipped[@]}"}; do
    [[ "$known" != "$entry" ]] || skip=1
  done
  ((!skip)) || continue # reported above
  log="$LOG_DIR/$name.log"
  started=$SECONDS
  echo "🔧 $entry: cargo check --workspace --all-targets"
  if (cd "$dir" && cargo check --workspace --all-targets) >"$log" 2>&1; then
    # A [patch] cargo did not use is the silent failure this gate cannot
    # afford: the consumer compiled, but against the release it pins rather
    # than against the candidate core, and reporting that green would be a
    # pass for a core nothing looked at.
    if was_patched "$entry" && grep -q "neat-core.*was not used in the crate graph" "$log"; then
      echo "❌ $entry: cargo ignored the [patch] override and compiled the release it pins ($((SECONDS - started))s) — log: $log"
      grep "was not used in the crate graph" "$log" | head -n 3 | sed 's/^/    /'
      failed+=("$entry ([patch] not used)")
    else
      echo "✅ $entry compiles against this core ($((SECONDS - started))s)"
    fi
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
