#!/usr/bin/env bash
# bump-deps.sh — refresh Cargo dependencies before quality.sh (Issue #38).
#
# Invoked by the Vibe Coder worker before quality.sh per the contract in
# stSoftwareAU/VibeCoding#1613. NEAT-AI-core is the root of the stSoftware
# dependency chain (see stSoftwareAU/VibeCoding#1614), so there are no
# internal pins to refresh — the script focuses on:
#
#   1. External: crates.io — `cargo update`, honouring the quarantine window
#      (`--quarantine-hours`, default `$VIBE_BUMP_QUARANTINE_HOURS` / 24h)
#      so versions published less than N hours ago are deferred to dodge
#      fast-flagged supply-chain attacks.
#   2. `cargo audit` — fails non-zero on any reported advisory, naming the
#      offending crate + advisory ID.
#   3. `cargo build` (native) and `cargo build --target wasm32-unknown-unknown`
#      — both must succeed against the bumped tree.
#
# Exit 0 = clean (or no-op). Non-zero = bump rejected by audit/build/etc.;
# the worker reverts per the contract.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: bump-deps.sh [options]

Refreshes Cargo dependencies, then runs cargo audit and dual native/WASM
builds. Prints a one-line summary.

Options:
  --quarantine-hours N   Skip crates.io versions newer than N hours.
                         Default: $VIBE_BUMP_QUARANTINE_HOURS, else 24.
  --skip-external        Skip cargo update (crates.io).
  --skip-audit           Skip cargo audit.
  --skip-build           Skip native + wasm32 cargo build.
  --repo DIR             Repository root (default: cwd).
  --check-published TS H Internal helper: exit 0 if TS (ISO 8601) is older
                         than H hours, else exit 1. Used by tests.
  -h, --help             Show this message.

Exit codes:
  0  clean / no-op
  1  bump produced a non-passing tree (audit / build failure)
  2  usage error
EOF
}

QUARANTINE_HOURS="${VIBE_BUMP_QUARANTINE_HOURS:-24}"
SKIP_EXTERNAL=0
SKIP_AUDIT=0
SKIP_BUILD=0
REPO_DIR="."
CHECK_PUBLISHED_TS=""
CHECK_PUBLISHED_HOURS=""
MODE="run"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --quarantine-hours) QUARANTINE_HOURS="$2"; shift 2 ;;
    --skip-external)    SKIP_EXTERNAL=1; shift ;;
    --skip-audit)       SKIP_AUDIT=1; shift ;;
    --skip-build)       SKIP_BUILD=1; shift ;;
    --repo)             REPO_DIR="$2"; shift 2 ;;
    --check-published)
      MODE="check-published"
      CHECK_PUBLISHED_TS="${2:-}"
      CHECK_PUBLISHED_HOURS="${3:-}"
      if [[ -z "$CHECK_PUBLISHED_TS" || -z "$CHECK_PUBLISHED_HOURS" ]]; then
        echo "Usage error: --check-published requires <timestamp> <hours>" >&2
        exit 2
      fi
      shift 3
      ;;
    -h|--help)          usage; exit 0 ;;
    *)
      echo "Usage error: unknown option '$1'" >&2
      usage >&2
      exit 2
      ;;
  esac
done

# --- helpers ---------------------------------------------------------------

# Returns 0 if $1 (ISO 8601) is older than $2 hours, else 1. Exit 2 on parse
# error so callers can distinguish unparsable timestamps from "still fresh".
is_older_than_hours() {
  local published_at="$1" hours="$2"
  python3 - "$published_at" "$hours" <<'PY'
import sys, datetime
ts = sys.argv[1].strip()
try:
    hours = float(sys.argv[2])
except ValueError:
    sys.exit(2)
if ts.endswith("Z"):
    ts = ts[:-1] + "+00:00"
try:
    pub = datetime.datetime.fromisoformat(ts)
except ValueError:
    sys.exit(2)
if pub.tzinfo is None:
    pub = pub.replace(tzinfo=datetime.timezone.utc)
now = datetime.datetime.now(datetime.timezone.utc)
age_hours = (now - pub).total_seconds() / 3600.0
sys.exit(0 if age_hours >= hours else 1)
PY
}

if [[ "$MODE" == "check-published" ]]; then
  set +e
  is_older_than_hours "$CHECK_PUBLISHED_TS" "$CHECK_PUBLISHED_HOURS"
  rc=$?
  set -e
  if [[ "$rc" -eq 2 ]]; then
    echo "Error: invalid timestamp '$CHECK_PUBLISHED_TS'" >&2
    exit 2
  fi
  exit "$rc"
fi

if ! [[ "$QUARANTINE_HOURS" =~ ^[0-9]+$ ]]; then
  echo "Usage error: --quarantine-hours must be a non-negative integer (got '$QUARANTINE_HOURS')" >&2
  exit 2
fi

# Look up the published-at timestamp for a specific crates.io version.
# Honours $BUMP_DEPS_PUBLISH_FIXTURE (a directory of <crate>-<version>.iso
# files) so tests can exercise the quarantine branches without network.
crate_published_at() {
  local crate="$1" version="$2"
  if [[ -n "${BUMP_DEPS_PUBLISH_FIXTURE:-}" ]]; then
    local f="${BUMP_DEPS_PUBLISH_FIXTURE}/${crate}-${version}.iso"
    if [[ -f "$f" ]]; then
      tr -d '\n' <"$f"
      printf '\n'
      return 0
    fi
    # Missing fixture → treat as ancient so the bump proceeds.
    printf '1970-01-01T00:00:00Z\n'
    return 0
  fi
  if ! command -v curl >/dev/null 2>&1; then
    echo "Error: curl required to query crates.io" >&2
    return 1
  fi
  local base="${BUMP_DEPS_CRATES_IO_URL:-https://crates.io/api/v1}"
  local payload
  if ! payload=$(curl -fsSL --user-agent "neat-ai-core-bump-deps" \
    "${base}/crates/${crate}/versions" 2>/dev/null); then
    echo "Error: crates.io request for ${crate} failed" >&2
    return 1
  fi
  printf '%s' "$payload" | python3 -c '
import json, sys
target = sys.argv[1]
data = json.load(sys.stdin)
for v in data.get("versions", []):
    if v.get("num") == target:
        print(v.get("created_at", ""))
        break
' "$version"
}

# --- stages ----------------------------------------------------------------

external_changed=0
external_msg="skipped"
audit_msg="skipped"
build_msg="skipped"

bump_external() {
  if ! command -v cargo >/dev/null 2>&1; then
    echo "Error: cargo not available" >&2
    external_msg="error"
    return 1
  fi
  local dry_log
  dry_log="$({ cd "$REPO_DIR" && cargo update --dry-run 2>&1; } || true)"
  local applied=0 deferred=0 failed=0
  while IFS= read -r line; do
    # Match lines like:
    #   Updating clap v4.5.20 -> v4.5.21
    #   Bumping  serde v1.0.210 -> v1.0.211
    if [[ "$line" =~ (Updating|Bumping)[[:space:]]+([a-zA-Z0-9_-]+)[[:space:]]+v([0-9A-Za-z.+-]+)[[:space:]]+-\>[[:space:]]+v([0-9A-Za-z.+-]+) ]]; then
      local crate="${BASH_REMATCH[2]}"
      local new_v="${BASH_REMATCH[4]}"
      local published_at
      if ! published_at="$(crate_published_at "$crate" "$new_v")"; then
        echo "  skip: $crate $new_v (publish time lookup failed)"
        failed=$((failed + 1))
        continue
      fi
      if is_older_than_hours "$published_at" "$QUARANTINE_HOURS"; then
        if (cd "$REPO_DIR" && cargo update -p "$crate" --precise "$new_v") >/dev/null 2>&1; then
          applied=$((applied + 1))
          echo "  bump: $crate -> $new_v"
        else
          failed=$((failed + 1))
          echo "  fail: $crate -> $new_v (cargo update rejected)"
        fi
      else
        deferred=$((deferred + 1))
        echo "  defer: $crate $new_v (within ${QUARANTINE_HOURS}h quarantine, published $published_at)"
      fi
    fi
  done <<<"$dry_log"
  if [[ "$applied" -gt 0 ]]; then
    external_changed=1
  fi
  if [[ "$applied" -eq 0 && "$deferred" -eq 0 && "$failed" -eq 0 ]]; then
    external_msg="no updates"
  else
    external_msg="${applied} bumped, ${deferred} deferred, ${failed} failed"
  fi
  echo "external: $external_msg"
}

run_audit() {
  if ! cargo audit --version >/dev/null 2>&1; then
    echo "Error: cargo audit not available — install with 'cargo install cargo-audit --locked'" >&2
    audit_msg="error"
    return 1
  fi
  local audit_log
  if audit_log="$(cd "$REPO_DIR" && cargo audit 2>&1)"; then
    audit_msg="ok"
    echo "audit: ok"
    return 0
  fi
  # Surface the first advisory ID + offending crate so the worker log shows
  # exactly why the bump was rejected.
  local first
  first=$(printf '%s\n' "$audit_log" | awk '
    /^[[:space:]]*ID:[[:space:]]/    { id = $2 }
    /^[[:space:]]*Crate:[[:space:]]/ {
      if (id != "") { print "audit: FAILED — " $2 " (" id ")"; exit }
    }
  ')
  if [[ -z "$first" ]]; then
    first="audit: FAILED (see cargo audit output above)"
  fi
  audit_msg="failed"
  printf '%s\n' "$audit_log" >&2
  printf '%s\n' "$first" >&2
  return 1
}

run_build() {
  # Native build first — fastest signal on a typical dev box.
  if ! (cd "$REPO_DIR" && cargo build --workspace) >&2; then
    build_msg="native FAILED"
    echo "build: native FAILED" >&2
    return 1
  fi
  # WASM build — neat-core is consumed by NEAT-AI's wasm_activation, so the
  # bumped tree must compile cleanly for `wasm32-unknown-unknown` too.
  if ! command -v rustup >/dev/null 2>&1; then
    build_msg="wasm SKIPPED (rustup missing)"
    echo "build: native ok, wasm SKIPPED (rustup missing)"
    return 0
  fi
  if ! rustup target list --installed 2>/dev/null | grep -q '^wasm32-unknown-unknown$'; then
    build_msg="wasm SKIPPED (target not installed)"
    echo "build: native ok, wasm SKIPPED (run 'rustup target add wasm32-unknown-unknown')"
    return 0
  fi
  if ! (cd "$REPO_DIR" && cargo build --workspace --target wasm32-unknown-unknown) >&2; then
    build_msg="wasm FAILED"
    echo "build: native ok, wasm FAILED" >&2
    return 1
  fi
  build_msg="ok (native + wasm)"
  echo "build: ok (native + wasm)"
}

# --- main ------------------------------------------------------------------

if [[ "$SKIP_EXTERNAL" -eq 0 ]]; then bump_external; fi
if [[ "$SKIP_AUDIT"    -eq 0 ]]; then run_audit;     fi
if [[ "$SKIP_BUILD"    -eq 0 ]]; then run_build;     fi

if [[ "$external_changed" -eq 0 ]]; then
  echo "bump-deps: no bumps (external=${external_msg}; audit=${audit_msg}; build=${build_msg})"
else
  echo "bump-deps: external=${external_msg}; audit=${audit_msg}; build=${build_msg}"
fi
