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
#      Crates the plan never named but `cargo update` moved anyway are
#      age-checked against the same window, so no version reaches Cargo.lock
#      unverified (Issue #627).
#   2. Advisory scan — `cargo deny check advisories` when `cargo-deny` is on
#      PATH, otherwise `cargo audit`. Fails non-zero on any reported advisory,
#      naming the offending crate + advisory ID. With neither tool installed
#      the scan is skipped with a warning and any bump this run applied is
#      reverted, so nothing lands unscanned and a missing tool never fails the
#      run (Issue #621).
#   3. `cargo build` (native) and `cargo build --target wasm32-unknown-unknown`
#      — both must succeed against the bumped tree.
#
# Exit 0 = clean, no-op, or partial success: a crate that cannot be bumped
# safely — rejected by cargo, inside the quarantine window, or of unknown
# release age — is reported as a **deferral**, left at its current version,
# and the run carries on. Non-zero = the tree the run produced must not be
# kept: cargo missing, an advisory, a build failure, or a Cargo.lock that
# could not be restored; the worker reverts per the contract.
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
one-line summary. Crates that cannot be bumped safely are deferred, not
failed; with no advisory scanner installed the scan is skipped and the run's
bumps are reverted rather than landing unscanned.

Options:
  --quarantine-hours N   Skip crates.io versions newer than N hours.
                         Default: $VIBE_BUMP_QUARANTINE_HOURS, else 24.
  --skip-external        Skip cargo update (crates.io).
  --skip-audit           Skip the advisory scan.
  --skip-build           Skip native + wasm32 cargo build.
  --repo DIR             Repository root (default: cwd). Must be an existing
                         directory; anything else exits 2, and the value is
                         resolved to an absolute path so it can never be
                         parsed as a cd option (Issue #608).
  --check-published TS H Internal helper: exit 0 if TS (ISO 8601) is older
                         than H hours, else exit 1. Used by tests.
  -h, --help             Show this message.

Exit codes:
  0  clean / no-op / partial success — deferred crates are not failures
  1  the resulting tree must not be kept: advisory found, build failure,
     cargo missing, or Cargo.lock could not be restored
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

# --repo reaches `cd "$REPO_DIR"` below, where a `-`-prefixed value is parsed as
# a cd option: `cd -P` takes no operand and lands in $HOME, so the cargo passes
# would run outside the repository and find nothing to bump — and because the
# dry-run capture is wrapped in `|| true`, that was reported as "no updates"
# rather than surfacing (Issue #608).
#
# An existence test alone does NOT close this: `[[ -d "-P" ]]` is true whenever a
# directory named `-P` exists in the cwd, and `cd "-P"` still consumes it as an
# option. So resolve the value to an absolute path once, through a `cd --` that
# cannot be optioned, and let every later `cd "$REPO_DIR"` inherit the result.
if [[ ! -d "$REPO_DIR" ]]; then
  echo "Usage error: --repo must be an existing directory (got '$REPO_DIR')" >&2
  exit 2
fi
REPO_DIR="$(cd -- "$REPO_DIR" && pwd)"

# Look up the published-at timestamp for a specific crates.io version.
# Honours $BUMP_DEPS_PUBLISH_FIXTURE (a directory of <crate>-<version>.iso
# files) so tests can exercise the quarantine branches without network.
crate_published_at() {
  local crate="$1" version="$2" created=""
  if [[ -n "${BUMP_DEPS_PUBLISH_FIXTURE:-}" ]]; then
    local f="${BUMP_DEPS_PUBLISH_FIXTURE}/${crate}-${version}.iso"
    if [[ -f "$f" ]]; then
      created="$(tr -d '\n' <"$f")"
    else
      # Missing fixture → treat as ancient so the bump proceeds.
      created="1970-01-01T00:00:00Z"
    fi
  else
    if ! command -v curl >/dev/null 2>&1; then
      echo "Error: curl required to query crates.io" >&2
      return 1
    fi
    local base="${BUMP_DEPS_CRATES_IO_URL:-https://crates.io/api/v1}"
    local payload
    # Bounded retries and hard timeouts: a transient crates.io blip must not
    # silently drop a crate from the plan, and a stalled connection must never
    # hang an unattended run (Issue #621).
    if ! payload=$(curl -fsSL --user-agent "neat-ai-core-bump-deps" \
      --connect-timeout 10 --max-time 30 \
      --retry 2 --retry-delay 1 \
      "${base}/crates/${crate}/versions" 2>/dev/null); then
      echo "Error: crates.io request for ${crate} failed" >&2
      return 1
    fi
    created=$(printf '%s' "$payload" | python3 -c '
import json, sys
target = sys.argv[1]
data = json.load(sys.stdin)
for v in data.get("versions", []):
    if v.get("num") == target:
        print(v.get("created_at", ""))
        break
' "$version")
  fi
  # An answer naming no publish time is a failed lookup, not an ancient one:
  # the release age is unknown, so the crate is held rather than bumped blind.
  if [[ -z "${created//[[:space:]]/}" ]]; then
    echo "Error: no publish time found for ${crate} ${version}" >&2
    return 1
  fi
  printf '%s\n' "$created"
}

# Release-age lookups memoised for the whole run (Issue #627). A resolution
# change can move dozens of out-of-plan crates, and the breach check runs after
# every update — without a memo the same version is re-queried each time. A
# file rather than a shell variable: the breach check runs inside a command
# substitution, so a subshell's cache would die with the subshell.
AGE_CACHE_FILE=""

# crate_published_at with that memo. Prints the publish timestamp and returns
# 0; returns 1 when the release age could not be established. The failure is
# memoised too, so an unreachable registry is not re-queried per update.
crate_published_at_cached() {
  local crate="$1" version="$2" cached created
  if [[ -n "$AGE_CACHE_FILE" && -f "$AGE_CACHE_FILE" ]]; then
    cached="$(awk -v c="$crate" -v v="$version" '$1 == c && $2 == v { print $3; exit }' "$AGE_CACHE_FILE")"
    if [[ -n "$cached" ]]; then
      # "-" is the memoised failure; a timestamp never looks like it.
      [[ "$cached" == "-" ]] && return 1
      printf '%s\n' "$cached"
      return 0
    fi
  fi
  if ! created="$(crate_published_at "$crate" "$version")"; then
    if [[ -n "$AGE_CACHE_FILE" ]]; then
      printf '%s %s -\n' "$crate" "$version" >>"$AGE_CACHE_FILE"
    fi
    return 1
  fi
  if [[ -n "$AGE_CACHE_FILE" ]]; then
    printf '%s %s %s\n' "$crate" "$version" "$created" >>"$AGE_CACHE_FILE"
  fi
  printf '%s\n' "$created"
}

# --- lockfile helpers ------------------------------------------------------

# The one definition of the lockfile path; every stage below reads this.
LOCK_FILE="${REPO_DIR}/Cargo.lock"

# Print "<name> <version>" for every package in the repo's Cargo.lock, sorted.
# A missing lockfile prints nothing, so every "is it landed?" query below
# answers no rather than silently claiming success.
lock_snapshot() {
  [[ -f "$LOCK_FILE" ]] || return 0
  awk '
    /^name[[:space:]]*=/    { gsub(/"/, ""); name = $3 }
    /^version[[:space:]]*=/ { gsub(/"/, ""); if (name != "") { print name " " $3; name = "" } }
  ' "$LOCK_FILE" | LC_ALL=C sort
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

# Snapshot of Cargo.lock as the run found it, filled in by main before any
# stage touches it. It is what lets a stage that cannot verify a bump drop the
# bump instead of failing the whole run.
RUN_LOCK_BACKUP=""

# Restore the lockfile to the state the run started in, when a later stage
# cannot vouch for what the run changed. $1 says why. Returns 1 — fatal, the
# one "lockfile left in a broken state" case — when the restore cannot be done.
#
# The trigger is the lockfile itself, not the bump counter: `cargo update` also
# rewrites out-of-plan transitive entries, which the quarantine check allows
# once their release age clears and which leave `external_changed` at 0.
# Comparing against the snapshot is
# what makes "nothing lands unscanned" true of every byte of the file, not
# just of the crates the plan named. A run that found no lockfile cannot have
# changed one — every update pass refuses to run without it.
revert_run_bumps() {
  local reason="$1"
  [[ -n "$RUN_LOCK_BACKUP" && -f "$RUN_LOCK_BACKUP" ]] || return 0
  cmp -s "$RUN_LOCK_BACKUP" "$LOCK_FILE" && return 0
  if ! cp "$RUN_LOCK_BACKUP" "$LOCK_FILE"; then
    echo "Error: could not restore ${LOCK_FILE} — the lockfile may be inconsistent" >&2
    return 1
  fi
  external_changed=0
  # Restate the counts: a run whose lockfile went back bumped nothing.
  external_msg="0 bumped, ${EXTERNAL_DEFERRED} deferred${EXTERNAL_UNKNOWN_MSG} (lockfile restored — ${reason})"
  echo "external: reverted — ${reason}"
}

# --- stages ----------------------------------------------------------------

external_changed=0
external_msg="skipped"
EXTERNAL_UNKNOWN_MSG=""
# The counts behind external_msg, so a revert can restate them rather than
# leaving a summary that still claims bumps the run then dropped.
EXTERNAL_DEFERRED=0
audit_msg="skipped"
build_msg="skipped"

# The quarantine-approved bump plan, parallel arrays filled by bump_external
# and read by grouped_retry.
BUMP_NAMES=()
BUMP_FROMS=()
BUMP_TARGETS=()
# "<name> <version>" lines for crates the quarantine deferred, at the version
# they must stay on. Both update passes check these did not move.
BUMP_DEFERRED_LOCK=""
# Why a planned bump did not land, by plan index. Unset means the plain
# "cargo update rejected" case; a quarantine revert fills in its own reason.
BUMP_DEFER_REASON=()

# Name the first quarantine breach a `cargo update` left in the lockfile, or
# print nothing when the lock is clean. $1 is the lock_snapshot taken before
# that update — this is the single definition of the contract both the
# per-crate pass and the grouped retry are held to (Issue #614).
#
# A breach is either a crate the quarantine deferred moving off the version it
# was held at, or a crate in the bump plan landing anywhere other than its
# approved target or the version(s) it held before the update.
#
# Movement of out-of-plan transitive crates is still allowed — cargo must be
# free to move a dependency to satisfy the versions it was asked for — but
# only onto a version that has itself cleared the release-age window. The plan
# never named those crates, so nothing else checks them, and a crate dragged
# onto a version published minutes ago would otherwise land in Cargo.lock
# unverified (Issue #627). An age the run could not establish counts as a
# breach: refusing the update beats keeping a version nobody vouched for.
quarantine_breach() {
  local before="$1" after i name version clean line crate published_at
  after="$(lock_snapshot)"
  while IFS= read -r line; do
    [[ -n "$line" ]] || continue
    if ! grep -qxF "$line" <<<"$after"; then
      printf 'moved quarantined %s off %s\n' "${line%% *}" "${line##* }"
      return 0
    fi
  done <<<"$BUMP_DEFERRED_LOCK"
  # Guarded: bash 3.2 treats "${!arr[@]}" on an empty array as unbound. The
  # guard skips only this loop — the out-of-plan check below still runs.
  for i in ${BUMP_NAMES[@]+"${!BUMP_NAMES[@]}"}; do
    name="${BUMP_NAMES[i]}"
    # Every version the crate is now locked at must be its approved target or
    # one it already held. Checked version by version rather than "is the
    # target present?": a crate locked at two majors (syn 2.x and 3.x) can
    # land its target and be dragged on the *other* major in one update.
    clean=1
    while IFS= read -r version; do
      [[ -n "$version" ]] || continue
      [[ "$version" == "${BUMP_TARGETS[i]}" ]] && continue
      if ! grep -qxF "$name $version" <<<"$before"; then
        clean=0
        break
      fi
    done <<<"$(awk -v c="$name" '$1 == c { print $2 }' <<<"$after")"
    if [[ "$clean" -eq 1 ]]; then continue; fi
    printf 'moved %s off its approved target %s\n' "$name" "${BUMP_TARGETS[i]}"
    return 0
  done
  # Everything else the update left in the lock: the "<name> <version>" lines
  # present after it and absent before it. Both snapshots come out of
  # lock_snapshot already `LC_ALL=C sort`ed, so one comm names the movement
  # rather than a grep per package — a real Cargo.lock carries hundreds.
  local approved="" moved
  for i in ${BUMP_NAMES[@]+"${!BUMP_NAMES[@]}"}; do
    approved+="${BUMP_NAMES[i]} ${BUMP_TARGETS[i]}"$'\n'
  done
  moved="$(LC_ALL=C comm -13 <(printf '%s\n' "$before") <(printf '%s\n' "$after"))"
  # The one exemption, by exact "<name> <version>" rather than by crate name:
  # a planned crate on its approved target, age-checked when the plan was read
  # (the loop above has already rejected it anywhere else). Exempting the name
  # instead would wave through a second major of a planned or deferred crate
  # that nothing has age-checked.
  while read -r crate version; do
    [[ -n "$crate" ]] || continue
    if grep -qxF "$crate $version" <<<"$approved"; then continue; fi
    if ! published_at="$(crate_published_at_cached "$crate" "$version")"; then
      printf 'moved out-of-plan %s to %s (release age unknown)\n' "$crate" "$version"
      return 0
    fi
    if ! is_older_than_hours "$published_at" "$QUARANTINE_HOURS"; then
      printf 'moved out-of-plan %s to %s (within %sh quarantine, published %s)\n' \
        "$crate" "$version" "$QUARANTINE_HOURS" "$published_at"
      return 0
    fi
  done <<<"$moved"
  # A clean lock: no breach named, and never a non-zero status the callers'
  # `breach="$(quarantine_breach …)"` assignment would trip `set -e` on.
  return 0
}

# Retry every crate the per-crate pass could not land, in one `cargo update`.
# Some families (js-sys / wasm-bindgen / web-sys) only resolve when moved
# together, and each is rejected on its own. Takes the indices to retry.
#
# The grouped update carries no `--precise` (cargo accepts only one), so the
# lockfile is checked against `quarantine_breach` afterwards: any planned crate
# off both its approved target and the version it started from, or any deferred
# crate that moved, reverts the whole group.
grouped_retry() {
  [[ $# -gt 0 ]] || return 0
  local -a idx=("$@")
  local -a specs=() members=()
  local i spec
  for i in "${idx[@]}"; do
    if crate_locked_at "${BUMP_NAMES[i]}" "${BUMP_TARGETS[i]}"; then continue; fi
    if ! spec="$(crate_pkg_spec "${BUMP_NAMES[i]}" "${BUMP_FROMS[i]}")"; then
      echo "  skip: ${BUMP_NAMES[i]} ${BUMP_TARGETS[i]} (locked at several versions, no unambiguous package spec)"
      continue
    fi
    members+=("$i")
    specs+=(-p "$spec")
  done
  [[ "${#members[@]}" -gt 0 ]] || return 0

  if [[ ! -f "$LOCK_FILE" ]]; then
    echo "  skip: grouped retry of ${#members[@]} crate(s) (no ${LOCK_FILE} to verify against)"
    return 0
  fi
  local backup before breach
  backup="$(mktemp "${TMPDIR:-/tmp}/bump-deps-lock.XXXXXX")"
  cp "$LOCK_FILE" "$backup"
  before="$(lock_snapshot)"
  if ! (cd "$REPO_DIR" && cargo update "${specs[@]}") >&2; then
    echo "  retry: grouped cargo update rejected ${#members[@]} crate(s)"
    cp "$backup" "$LOCK_FILE"
    rm -f "$backup"
    return 0
  fi

  breach="$(quarantine_breach "$before")"
  if [[ -n "$breach" ]]; then
    echo "  revert: grouped retry $breach"
    cp "$backup" "$LOCK_FILE"
    for i in "${members[@]}"; do
      BUMP_DEFER_REASON[i]="reverted — grouped retry $breach"
    done
  fi
  rm -f "$backup"
}

bump_external() {
  if ! command -v cargo >/dev/null 2>&1; then
    echo "Error: cargo not available" >&2
    external_msg="error"
    return 1
  fi
  # A dry run that fails read no plan at all. Report that rather than the
  # "no updates" a swallowed failure used to look like (Issue #621): nothing
  # was changed, so the run stays exit 0, but the log names the cause.
  local dry_log dry_rc=0
  dry_log="$({ cd "$REPO_DIR" && cargo update --dry-run 2>&1; })" || dry_rc=$?
  if [[ "$dry_rc" -ne 0 ]]; then
    printf '%s\n' "$dry_log" >&2
    external_msg="plan unavailable (cargo update --dry-run failed)"
    echo "external: $external_msg"
    return 0
  fi

  BUMP_NAMES=()
  BUMP_FROMS=()
  BUMP_TARGETS=()
  BUMP_DEFERRED_LOCK=""
  BUMP_DEFER_REASON=()
  local applied=0 deferred=0 unknown=0
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
      if ! published_at="$(crate_published_at_cached "$crate" "$new_v")"; then
        # Unknown release age is treated exactly like an unexpired one: the
        # crate is held at the version it is on, and every update in this run
        # is checked against that hold.
        # Counted apart from a quarantine hold: waiting out the release-age
        # window is routine, but a lookup nobody could answer is a host or
        # registry fault, and a run where every crate lands here has silently
        # stopped bumping anything.
        unknown=$((unknown + 1))
        BUMP_DEFERRED_LOCK+="$crate $from_v"$'\n'
        echo "  defer: $crate $new_v (publish time lookup failed)"
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

  local i spec backup before breach
  local -a retry=()
  if [[ "${#BUMP_NAMES[@]}" -gt 0 ]]; then
    # Pass 1 — one crate at a time, spec pinned to the locked version. Even
    # with `--precise`, cargo may move *other* crates to satisfy the pin, so
    # each update is snapshotted and verified: a drag past the quarantine is
    # reverted and named rather than left in the lock (Issue #614). One
    # snapshot per crate rather than one for the whole pass — it costs a copy
    # of a file cargo has just rewritten anyway, and it is what lets the
    # report name the update that caused the drag and keep the bumps that
    # landed cleanly before it.
    for i in "${!BUMP_NAMES[@]}"; do
      # No lockfile, no verification — so no update either. An unverifiable
      # bump is refused and named, never run quietly (as grouped_retry does).
      if [[ ! -f "$LOCK_FILE" ]]; then
        echo "  skip: ${BUMP_NAMES[i]} ${BUMP_TARGETS[i]} (no ${LOCK_FILE} to verify against)"
        BUMP_DEFER_REASON[i]="no ${LOCK_FILE} to verify against"
        continue
      fi
      # An earlier bump may already have dragged this crate to its target.
      if crate_locked_at "${BUMP_NAMES[i]}" "${BUMP_TARGETS[i]}"; then continue; fi
      if ! spec="$(crate_pkg_spec "${BUMP_NAMES[i]}" "${BUMP_FROMS[i]}")"; then
        retry+=("$i")
        continue
      fi
      backup="$(mktemp "${TMPDIR:-/tmp}/bump-deps-lock.XXXXXX")"
      cp "$LOCK_FILE" "$backup"
      before="$(lock_snapshot)"
      if ! (cd "$REPO_DIR" && cargo update -p "$spec" --precise "${BUMP_TARGETS[i]}") >/dev/null 2>&1; then
        # A rejected update leaves nothing verified behind it either.
        cp "$backup" "$LOCK_FILE"
        retry+=("$i")
      else
        breach="$(quarantine_breach "$before")"
        if [[ -n "$breach" ]]; then
          echo "  revert: per-crate update of ${BUMP_NAMES[i]} $breach"
          cp "$backup" "$LOCK_FILE"
          BUMP_DEFER_REASON[i]="reverted — $breach"
        fi
      fi
      rm -f "$backup"
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
        # Not landed is not failed: the crate stays on the version it was
        # already on, which is a safe outcome the run reports and moves past.
        deferred=$((deferred + 1))
        echo "  defer: ${BUMP_NAMES[i]} -> ${BUMP_TARGETS[i]} (${BUMP_DEFER_REASON[i]:-cargo update rejected})"
      fi
    done
  fi

  if [[ "$applied" -gt 0 ]]; then
    external_changed=1
  fi
  EXTERNAL_DEFERRED="$deferred"
  EXTERNAL_UNKNOWN_MSG=""
  if [[ "$unknown" -gt 0 ]]; then
    EXTERNAL_UNKNOWN_MSG=", ${unknown} release age unknown"
  fi
  if [[ "$applied" -eq 0 && "$deferred" -eq 0 && "$unknown" -eq 0 ]]; then
    external_msg="no updates"
  else
    external_msg="${applied} bumped, ${deferred} deferred${EXTERNAL_UNKNOWN_MSG}"
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
    # Failing here is what disabled bumps for this repo (Issues #598, #621): a
    # scanner missing from an unattended host says nothing about the tree. The
    # scan is skipped loudly and this run's bumps are dropped, so nothing
    # lands unscanned and the run still reports a truthful no-op.
    echo "Warning: no advisory scanner on PATH — install with 'cargo install cargo-deny --locked' (preferred) or 'cargo install cargo-audit --locked'" >&2
    echo "audit: SKIPPED (no advisory scanner on PATH)"
    audit_msg="SKIPPED (no scanner)"
    revert_run_bumps "no advisory scanner to verify them" || return 1
    return 0
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

# One trap for every scratch file the run creates; both paths are read at exit
# time, so the trap is armed before either is filled in.
cleanup_run_files() {
  [[ -n "$AGE_CACHE_FILE" ]] && rm -f "$AGE_CACHE_FILE"
  [[ -n "$RUN_LOCK_BACKUP" ]] && rm -f "$RUN_LOCK_BACKUP"
  return 0
}
trap cleanup_run_files EXIT

AGE_CACHE_FILE="$(mktemp "${TMPDIR:-/tmp}/bump-deps-age-cache.XXXXXX")"

if [[ -f "$LOCK_FILE" ]]; then
  RUN_LOCK_BACKUP="$(mktemp "${TMPDIR:-/tmp}/bump-deps-run-lock.XXXXXX")"
  cp "$LOCK_FILE" "$RUN_LOCK_BACKUP"
fi

if [[ "$SKIP_EXTERNAL" -eq 0 ]]; then bump_external; fi
if [[ "$SKIP_AUDIT"    -eq 0 ]]; then run_audit;     fi
if [[ "$SKIP_BUILD"    -eq 0 ]]; then run_build;     fi

if [[ "$external_changed" -eq 0 ]]; then
  echo "bump-deps: no bumps (external=${external_msg}; audit=${audit_msg}; build=${build_msg})"
else
  echo "bump-deps: external=${external_msg}; audit=${audit_msg}; build=${build_msg}"
fi
