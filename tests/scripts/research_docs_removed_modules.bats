#!/usr/bin/env bats
# Regression assertion for Issue #498 (BP-b7a22f313002) — two research docs
# presented modules that Issues #414 (`pc_inference` / `pc_learning`) and #415
# (`wasm_dataset`) deleted as if they were still part of the crate, and the
# lane (d) doc described a delivery path for the deleted offload that
# contradicted the README's own removal record. These tests pin the rule: a
# research doc may still discuss a removed module — history matters — but every
# mention must sit next to its removal record, and no doc may describe the
# removed work as still pending.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  RESEARCH_DIR="${REPO_ROOT}/docs/research"
  LANE_D="${RESEARCH_DIR}/wasm64-lane-d-learn-wiring-verification.md"
  IKARUGA="${RESEARCH_DIR}/ikaruga-neat-ai-comparison.md"
  README="${REPO_ROOT}/README.md"
  SRC_DIR="${REPO_ROOT}/neat-core/src"
  # Names of the modules/symbols deleted by Issues #414 and #415.
  # Module names and the prose that names the same removed capability.
  REMOVED_PATTERN='wasm_dataset|pc_inference|pc_learning|PredictiveCoding|[Pp]redictive.[Cc]oding'
}

# Emit "file:line: text" for every mention of a removed module that has no
# removal record (the removing issue number, or the word "removed") within
# four lines either side.
unmarked_mentions() {
  awk -v pat="$REMOVED_PATTERN" -v fname="$1" '
    { line[NR] = $0 }
    END {
      for (i = 1; i <= NR; i++) {
        if (line[i] !~ pat) continue
        marked = 0
        for (j = i - 4; j <= i + 4; j++) {
          if (j >= 1 && j <= NR && line[j] ~ /#41[45]|[Rr]emoved/) marked = 1
        }
        if (!marked) printf "%s:%d: %s\n", fname, i, line[i]
      }
    }' "$1"
}

# --- The premise: the modules really are gone from the tree -----------------

@test "the modules removed by #414 and #415 are absent from neat-core/src" {
  [ ! -e "${SRC_DIR}/wasm_dataset.rs" ]
  [ ! -e "${SRC_DIR}/pc_inference.rs" ]
  [ ! -e "${SRC_DIR}/pc_learning.rs" ]
  run grep -rq 'PredictiveCodingEngine' "$SRC_DIR"
  [ "$status" -ne 0 ]
}

# --- Every mention carries its removal record ------------------------------

@test "no research doc names a removed module without its removal record" {
  offenders=""
  for doc in "$RESEARCH_DIR"/*.md; do
    offenders="${offenders}$(unmarked_mentions "$doc")"
  done
  [ -z "$offenders" ] || {
    echo "unmarked mentions of removed modules:"
    echo "$offenders"
    false
  }
}

@test "README names the removed offload only beside its removal record" {
  offenders="$(unmarked_mentions "$README")"
  [ -z "$offenders" ] || {
    echo "unmarked mentions of removed modules:"
    echo "$offenders"
    false
  }
}

# --- The lane (d) doc agrees with the README -------------------------------

@test "the lane (d) doc points at the README removal record" {
  run grep -q 'README.md#training-data-offload' "$LANE_D"
  [ "$status" -eq 0 ]
}

@test "the lane (d) doc does not present the offload as a pending delivery" {
  # The upstream adoption issue closed unadopted; nothing is waiting on it.
  run grep -qE 'lands and is released|is addressed by lane' "$LANE_D"
  [ "$status" -ne 0 ]
}

# --- The Ikaruga inventory matches the tree --------------------------------

@test "the Ikaruga comparison lists no predictive-coding module as current" {
  # An inventory row is a table line whose first cell is the module name.
  run grep -qE '^\| `?pc_(inference|learning)' "$IKARUGA"
  [ "$status" -ne 0 ]
}
