#!/usr/bin/env bash
# bump-deps.sh — refresh Cargo dependencies before quality.sh (Issue #38).
#
# Invoked by the Vibe Coder worker before quality.sh: it refreshes external
# Cargo dependencies ahead of the quality gate, honouring the quarantine
# window. NEAT-AI-core is the root of the internal dependency chain, so there
# are no internal pins to refresh — the script focuses on:
#
#   1. External: crates.io — `cargo update`, honouring the quarantine window
#      (`--quarantine-hours`, default `$VIBE_BUMP_QUARANTINE_HOURS` / 24h)
#      so versions published less than N hours ago are deferred to dodge
#      fast-flagged supply-chain attacks. Crates the per-crate pass cannot
#      land on their own are retried together in one grouped `cargo update`.
#   2. Advisory scan — `cargo deny check advisories` when `cargo-deny` is on
#      PATH, otherwise `cargo audit`. Fails non-zero on any reported advisory,
#      naming the offending crate + advisory ID; also fails when neither tool
#      is installed (Issue #598).
#   3. `cargo build` (native) and `cargo build --target wasm32-unknown-unknown`
#      — both must succeed against the bumped tree.
#
# Exit 0 = clean (or no-op). Non-zero = bump rejected by audit/build/etc.;
# the worker reverts per the contract.
set -euo pipefail

# Source cargo environment if available (needed for non-login shells).
# quality.sh does the same; the worker runs this script *before* quality.sh,
# so without this preamble rustup's cargo is often missing from PATH.
if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

usage() {
  cat <<'EOF'
Usage: bump-deps.sh [options]

Refreshes Cargo dependencies, then runs the advisory scan (cargo deny check
advisories, falling back to cargo audit) and dual native/WASM builds. Prints a
one-line summary.

Options:
  --quarantine-hours N   Skip crates.io versions newer than N hours.
                         Default: $VIBE_BUMP_QUARANTINE_HOURS, else 24.
  --skip-external        Skip cargo update (crates.io).
  --skip-audit           Skip the advisory scan.
  --skip-build           Skip native + wasm32 cargo build.
  --repo DIR             Repository root (default: cwd).
  --check-published TS H Internal helper: exit 0 if TS (ISO 8601) is older
                         than H hours, else exit 1. Used by tests.
  -h, --help             Show this message.

Exit codes:
  0  clean / no-op
  1  bump produced a non-passing tree (advisory / build failure, or no
     advisory scanner installed)
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

# --- lockfile helpers ------------------------------------------------------

# Print "<name> <version>" for every package in the repo's Cargo.lock, sorted.
# A missing lockfile prints nothing, so every "is it landed?" query below
# answers no rather than silently claiming success.
lock_snapshot() {
  local lock="${REPO_DIR}/Cargo.lock"
  [[ -f "$lock" ]] || return 0
  awk '
    /^name[[:space:]]*=/    { gsub(/"/, ""); name = $3 }
    /^version[[:space:]]*=/ { gsub(/"/, ""); if (name != "") { print name " " $3; name = "" } }
  ' "$lock" | LC_ALL=C sort
}

# Exit 0 when <crate> is locked at <version>. The snapshot is materialised
# before the match rather than piped: `grep -q` exits at the first hit, and a
# SIGPIPE'd `sort` upstream would make `pipefail` report a locked crate as
# unlocked.
crate_locked_at() {
  local snapshot
  snapshot="$(lock_snapshot)"
  grep -qxF "$1 $2" <<<"$snapshot"
}

# Echo the `cargo update -p` spec for <crate>, pinned to the version it is
# locked at, so a crate locked at two majors (syn 2.x and 3.x) is unambiguous.
# Prefers <from> — the version the dry run planned to replace — and falls back
# to the sole locked version when an earlier bump already moved it. Returns 1
# when the crate is locked at several versions and none is <from>, because no
# unambiguous spec exists.
crate_pkg_spec() {
  local crate="$1" from="$2" versions count
  if crate_locked_at "$crate" "$from"; then
    printf '%s@%s\n' "$crate" "$from"
    return 0
  fi
  local snapshot
  snapshot="$(lock_snapshot)"
  versions="$(awk -v c="$crate" '$1 == c { print $2 }' <<<"$snapshot")"
  count="$(printf '%s' "$versions" | grep -c . || true)"
  if [[ "$count" -eq 1 ]]; then
    printf '%s@%s\n' "$crate" "$versions"
    return 0
  fi
  return 1
}

# --- stages ----------------------------------------------------------------

external_changed=0
external_msg="skipped"
audit_msg="skipped"
build_msg="skipped"

# The quarantine-approved bump plan, parallel arrays filled by bump_external
# and read by grouped_retry.
BUMP_NAMES=()
BUMP_FROMS=()
BUMP_TARGETS=()
# "<name> <version>" lines for crates the quarantine deferred, at the version
# they must stay on. The grouped retry checks these did not move.
BUMP_DEFERRED_LOCK=""

# Retry every crate the per-crate pass could not land, in one `cargo update`.
# Some families (js-sys / wasm-bindgen / web-sys) only resolve when moved
# together, and each is rejected on its own. Takes the indices to retry.
#
# The grouped update carries no `--precise` (cargo accepts only one), so the
# quarantine is re-checked afterwards against the lockfile: every crate in the
# group must end on its approved target or on the version it started from, and
# no crate the quarantine deferred may have moved. Anything else reverts the
# whole group. Movement of out-of-group transitive crates is expected — cargo
# must be free to move a dependency to satisfy the versions the group asked
# for — and is not a quarantine breach, so it does not trip the check.
grouped_retry() {
  [[ $# -gt 0 ]] || return 0
  local -a idx=("$@")
  local -a specs=() members=() froms=()
  local i spec
  for i in "${idx[@]}"; do
    if crate_locked_at "${BUMP_NAMES[i]}" "${BUMP_TARGETS[i]}"; then continue; fi
    if ! spec="$(crate_pkg_spec "${BUMP_NAMES[i]}" "${BUMP_FROMS[i]}")"; then
      echo "  skip: ${BUMP_NAMES[i]} ${BUMP_TARGETS[i]} (locked at several versions, no unambiguous package spec)"
      continue
    fi
    members+=("$i")
    froms+=("${spec##*@}")
    specs+=(-p "$spec")
  done
  [[ "${#members[@]}" -gt 0 ]] || return 0

  local lock="${REPO_DIR}/Cargo.lock"
  if [[ ! -f "$lock" ]]; then
    echo "  skip: grouped retry of ${#members[@]} crate(s) (no ${lock} to verify against)"
    return 0
  fi
  local backup
  backup="$(mktemp "${TMPDIR:-/tmp}/bump-deps-lock.XXXXXX")"
  cp "$lock" "$backup"
  if ! (cd "$REPO_DIR" && cargo update "${specs[@]}") >&2; then
    echo "  retry: grouped cargo update rejected ${#members[@]} crate(s)"
    cp "$backup" "$lock"
    rm -f "$backup"
    return 0
  fi

  local approved=1 j line
  for j in "${!members[@]}"; do
    i="${members[j]}"
    if crate_locked_at "${BUMP_NAMES[i]}" "${BUMP_TARGETS[i]}"; then continue; fi
    if crate_locked_at "${BUMP_NAMES[i]}" "${froms[j]}"; then continue; fi
    echo "  revert: grouped retry moved ${BUMP_NAMES[i]} off its approved target ${BUMP_TARGETS[i]}"
    approved=0
    break
  done
  if [[ "$approved" -eq 1 && -n "$BUMP_DEFERRED_LOCK" ]]; then
    while IFS= read -r line; do
      [[ -n "$line" ]] || continue
      if ! crate_locked_at "${line%% *}" "${line##* }"; then
        echo "  revert: grouped retry moved quarantined ${line%% *} off ${line##* }"
        approved=0
        break
      fi
    done <<<"$BUMP_DEFERRED_LOCK"
  fi

  if [[ "$approved" -eq 0 ]]; then
    cp "$backup" "$lock"
  fi
  rm -f "$backup"
}

bump_external() {
  if ! command -v cargo >/dev/null 2>&1; then
    echo "Error: cargo not available" >&2
    external_msg="error"
    return 1
  fi
  local dry_log
  dry_log="$({ cd "$REPO_DIR" && cargo update --dry-run 2>&1; } || true)"

  BUMP_NAMES=()
  BUMP_FROMS=()
  BUMP_TARGETS=()
  BUMP_DEFERRED_LOCK=""
  local applied=0 deferred=0 failed=0
  local line crate from_v new_v published_at
  # Pass 0 — read the plan and split it into quarantine-approved and deferred.
  while IFS= read -r line; do
    # Match lines like:
    #   Updating clap v4.5.20 -> v4.5.21
    #   Bumping  serde v1.0.210 -> v1.0.211
    if [[ "$line" =~ (Updating|Bumping)[[:space:]]+([a-zA-Z0-9_-]+)[[:space:]]+v([0-9A-Za-z.+-]+)[[:space:]]+-\>[[:space:]]+v([0-9A-Za-z.+-]+) ]]; then
      crate="${BASH_REMATCH[2]}"
      from_v="${BASH_REMATCH[3]}"
      new_v="${BASH_REMATCH[4]}"
      if ! published_at="$(crate_published_at "$crate" "$new_v")"; then
        echo "  skip: $crate $new_v (publish time lookup failed)"
        failed=$((failed + 1))
        continue
      fi
      if is_older_than_hours "$published_at" "$QUARANTINE_HOURS"; then
        BUMP_NAMES+=("$crate")
        BUMP_FROMS+=("$from_v")
        BUMP_TARGETS+=("$new_v")
      else
        deferred=$((deferred + 1))
        BUMP_DEFERRED_LOCK+="$crate $from_v"$'\n'
        echo "  defer: $crate $new_v (within ${QUARANTINE_HOURS}h quarantine, published $published_at)"
      fi
    fi
  done <<<"$dry_log"

  local i spec
  local -a retry=()
  if [[ "${#BUMP_NAMES[@]}" -gt 0 ]]; then
    # Pass 1 — one crate at a time, spec pinned to the locked version.
    for i in "${!BUMP_NAMES[@]}"; do
      # An earlier bump may already have dragged this crate to its target.
      if crate_locked_at "${BUMP_NAMES[i]}" "${BUMP_TARGETS[i]}"; then continue; fi
      if ! spec="$(crate_pkg_spec "${BUMP_NAMES[i]}" "${BUMP_FROMS[i]}")"; then
        retry+=("$i")
        continue
      fi
      if ! (cd "$REPO_DIR" && cargo update -p "$spec" --precise "${BUMP_TARGETS[i]}") >/dev/null 2>&1; then
        retry+=("$i")
      fi
    done

    # Pass 2 — retry the rejects together.
    if [[ "${#retry[@]}" -gt 0 ]]; then
      grouped_retry "${retry[@]}"
    fi

    # Pass 3 — reconcile against the lockfile: a crate is bumped only when the
    # lock actually holds its approved target, whichever pass landed it.
    for i in "${!BUMP_NAMES[@]}"; do
      if crate_locked_at "${BUMP_NAMES[i]}" "${BUMP_TARGETS[i]}"; then
        applied=$((applied + 1))
        echo "  bump: ${BUMP_NAMES[i]} -> ${BUMP_TARGETS[i]}"
      else
        failed=$((failed + 1))
        echo "  fail: ${BUMP_NAMES[i]} -> ${BUMP_TARGETS[i]} (cargo update rejected)"
      fi
    done
  fi

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

# Both scanners feed the same report, so each parser emits one
# "<crate>|<version>|<id>|<title>" record per advisory and run_audit owns the
# single copy of the output format.

# Parse `cargo deny --format json check advisories` diagnostics on stdin. The
# program is read from a quoted heredoc rather than a heredoc on stdin, which
# would swallow the diagnostics this function is here to read.
deny_advisories() {
  python3 -c "$(cat <<'PY'
import json
import sys

for raw in sys.stdin:
    raw = raw.strip()
    if not raw.startswith("{"):
        continue
    try:
        obj = json.loads(raw)
    except ValueError:
        continue
    if obj.get("type") != "diagnostic":
        continue
    fields = obj.get("fields") or {}
    if fields.get("severity") != "error":
        continue
    advisory = fields.get("advisory") or {}
    ident = advisory.get("id")
    crate = advisory.get("package")
    version = ""
    graphs = fields.get("graphs") or []
    if graphs:
        krate = graphs[0].get("Krate") or {}
        # cargo-deny omits `advisory.package` for some diagnostic kinds; the
        # crate the graph is rooted at is the offending one either way.
        crate = crate or krate.get("name")
        version = krate.get("version") or ""
    title = advisory.get("title") or fields.get("message") or ""
    if ident and crate:
        print("%s|%s|%s|%s" % (crate, version, ident, title))
PY
  )"
}

# Parse `cargo audit` text output on stdin into the same record shape. cargo
# audit prints Crate/Version/Title before ID, so the record is emitted when the
# ID arrives rather than assuming a field order.
audit_advisories() {
  awk -F': *' '
    /^[[:space:]]*Crate:[[:space:]]/   { crate = $2 }
    /^[[:space:]]*Version:[[:space:]]/ { version = $2 }
    /^[[:space:]]*Title:[[:space:]]/   { title = $2 }
    /^[[:space:]]*ID:[[:space:]]/      {
      if (crate != "") { print crate "|" version "|" $2 "|" title }
      crate = ""; version = ""; title = ""
    }
  '
}

run_audit() {
  # cargo-deny is the repo's own advisory tool (quality.sh and the ci.yml
  # `deny` job both run it, and deny.toml lives here); cargo-audit is the
  # fallback CI still installs. Requiring cargo-audit alone is what silently
  # disabled every bump on the worker (Issue #598).
  local tool
  if command -v cargo-deny >/dev/null 2>&1; then
    tool="cargo-deny"
  elif command -v cargo-audit >/dev/null 2>&1; then
    tool="cargo-audit"
  else
    echo "Error: no advisory scanner on PATH — install with 'cargo install cargo-deny --locked' (preferred) or 'cargo install cargo-audit --locked'" >&2
    audit_msg="error"
    return 1
  fi
  local audit_log records
  if [[ "$tool" == "cargo-deny" ]]; then
    echo "  audit tool: cargo deny check advisories"
    if audit_log="$(cd "$REPO_DIR" && cargo deny --format json check advisories 2>&1)"; then
      audit_msg="ok"
      echo "audit: ok"
      return 0
    fi
    records="$(deny_advisories <<<"$audit_log")"
  else
    echo "  audit tool: cargo audit"
    if audit_log="$(cd "$REPO_DIR" && cargo audit 2>&1)"; then
      audit_msg="ok"
      echo "audit: ok"
      return 0
    fi
    records="$(audit_advisories <<<"$audit_log")"
  fi
  audit_msg="failed"
  # Surface the offending crates so the worker log shows exactly why the bump
  # was rejected, then the one-line verdict naming the first of them. An
  # unparsable log still fails loud, with the raw output to read.
  if [[ -z "$records" ]]; then
    printf '%s\n' "$audit_log" >&2
    printf 'audit: FAILED (see %s output above)\n' "$tool" >&2
    return 1
  fi
  local crate version ident title
  while IFS='|' read -r crate version ident title; do
    [[ -n "$crate" ]] || continue
    printf '  advisory: %s %s — %s (%s)\n' "$crate" "$version" "$title" "$ident" >&2
  done <<<"$records"
  IFS='|' read -r crate version ident title <<<"$(head -n 1 <<<"$records")"
  printf 'audit: FAILED — %s (%s)\n' "$crate" "$ident" >&2
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
