#!/usr/bin/env bats
# Regression assertion for Issue #499 (BP-178e60a42f74) — `RELEASING.md` is the
# single source of truth for the release policy (Issue #251), but its
# breaking-change log broke its own completeness rule and contradicted itself:
# the `0.4.0` (Issue #414) and `0.5.0` (Issue #415) module removals were never
# logged, and the `0.6.0` entry asserted that `calculate_error_batch_4way` was
# "live and unchanged" after the `0.7.0` entry had removed it.
#
# These tests pin three rules on the log, checked against the crate itself:
#   1. every major-equivalent bump has an entry — no gap in the minor sequence;
#   2. no entry claims a removed symbol is live, and every symbol an entry does
#      claim is live still resolves in `neat-core/src`;
#   3. the deprecate -> migrate -> delete flow is documented here, not only in
#      the archived PR summaries.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  RELEASING="${REPO_ROOT}/RELEASING.md"
  SRC_DIR="${REPO_ROOT}/neat-core/src"
  LOG="${BATS_TEST_TMPDIR}/breaking-change-log.md"
  # The log body: from its H2 heading to the next H2.
  awk '/^## Breaking-change log/ { f = 1; next } /^## / { f = 0 } f' \
    "$RELEASING" >"$LOG"
}

# Minor numbers of the logged `0.<minor>.0` entries, ascending.
logged_minors() {
  grep -oE '^### `0\.[0-9]+\.0`' "$LOG" | grep -oE '[0-9]+\.0`$' |
    cut -d. -f1 | sort -n
}

# Symbols an entry heading declares removed: every backticked identifier on a
# `### ` line that says "removed".
removed_symbols() {
  grep -E '^### .*removed' "$LOG" |
    grep -oE '`[a-z_][A-Za-z0-9_]*`' | tr -d '`' | sort -u
}

# Sentences in the log that claim something is live.
live_sentences() {
  # Join each paragraph onto one line, then split on sentence boundaries so a
  # claim is judged by its own sentence rather than its whole paragraph.
  awk 'BEGIN { RS = ""; FS = "\n" }
       { p = ""; for (i = 1; i <= NF; i++) p = p (i > 1 ? " " : "") $i
         gsub(/\. /, ".\n", p); print p }' "$LOG" | grep -E '\blive\b'
}

# Does a documented symbol pattern resolve in neat-core/src? Accepts the two
# shorthands the log uses: a `*` glob and a `{a,b}` alternation.
resolves_in_src() {
  local pattern="$1"
  pattern="${pattern//\*/[A-Za-z0-9_]*}"
  pattern="${pattern//\{/(}"
  pattern="${pattern//\}/)}"
  pattern="${pattern//,/|}"
  grep -rqE "$pattern" "$SRC_DIR"
}

# --- Completeness: every major-equivalent bump is recorded ------------------

@test "the breaking-change log has an entry for every minor between its oldest and newest" {
  local minors first last missing=""
  minors="$(logged_minors)"
  [ -n "$minors" ]
  first="$(echo "$minors" | head -1)"
  last="$(echo "$minors" | tail -1)"
  for ((m = first; m <= last; m++)); do
    echo "$minors" | grep -qx "$m" || missing="${missing} 0.${m}.0"
  done
  [ -z "$missing" ] || {
    echo "breaking-change log skips:${missing}"
    false
  }
}

@test "the 0.4.0 entry records the PredictiveCodingEngine removal (Issue #414)" {
  local entry
  entry="$(awk '/^### `0\.4\.0`/ { f = 1 } f && /^### `0\.3\.0`/ { f = 0 } f' "$LOG")"
  [ -n "$entry" ]
  echo "$entry" | grep -q '#414'
  echo "$entry" | grep -q 'pc_inference'
  echo "$entry" | grep -q 'pc_learning'
  # The premise: those modules really are gone.
  [ ! -e "${SRC_DIR}/pc_inference.rs" ]
  [ ! -e "${SRC_DIR}/pc_learning.rs" ]
}

@test "the 0.5.0 entry records the wasm_dataset removal (Issue #415)" {
  local entry
  entry="$(awk '/^### `0\.5\.0`/ { f = 1 } f && /^### `0\.4\.0`/ { f = 0 } f' "$LOG")"
  [ -n "$entry" ]
  echo "$entry" | grep -q '#415'
  echo "$entry" | grep -q 'wasm_dataset'
  [ ! -e "${SRC_DIR}/wasm_dataset.rs" ]
}

# --- Self-consistency: nothing removed is described as live -----------------

@test "no entry claims a removed symbol is live" {
  local offenders=""
  while read -r symbol; do
    [ -n "$symbol" ] || continue
    local hits
    hits="$(live_sentences | grep -F "\`${symbol}\`" || true)"
    [ -z "$hits" ] || offenders="${offenders}${symbol}: ${hits}"$'\n'
  done < <(removed_symbols)
  [ -z "$offenders" ] || {
    echo "removed symbols described as live:"
    echo "$offenders"
    false
  }
}

@test "every symbol the log calls live resolves in neat-core/src" {
  local offenders=""
  while read -r token; do
    [ -n "$token" ] || continue
    resolves_in_src "$token" || offenders="${offenders} ${token}"
  done < <(live_sentences | grep -oE '`[a-z_][A-Za-z0-9_*{},]*`' | tr -d '`' | sort -u)
  [ -z "$offenders" ] || {
    echo "documented as live but absent from neat-core/src:${offenders}"
    false
  }
}

# --- The three-phase removal flow is documented here ------------------------

@test "RELEASING.md documents the deprecate-migrate-delete flow" {
  run grep -qiE '^## Removing public API' "$RELEASING"
  [ "$status" -eq 0 ]
}

@test "the removal flow records the -D warnings interaction with #[deprecated]" {
  grep -q -- '-D warnings' "$RELEASING"
  grep -q 'allow(deprecated)' "$RELEASING"
}

@test "the removal flow records that there is no CHANGELOG.md" {
  grep -q 'CHANGELOG.md' "$RELEASING"
}

@test "the removal flow records the deletion preconditions and the rebase rule" {
  # Zero source hits across the consumer repos before deleting (verified in #409).
  grep -qiE 'code search|consumer repo' "$RELEASING"
  # Sibling removals take successive minors rather than colliding on one.
  grep -qiE 'rebase' "$RELEASING"
}
