#!/usr/bin/env bats
# Regression assertions for Issue #629 — `.github/workflows/` described the
# pre-#598 advisory contract of `bump-deps.sh`: cargo-audit as a hard
# requirement, and a generated PR body asserting the bumps "passed
# `cargo audit`" even on a runner where `cargo deny check advisories` is what
# actually ran.
#
# Since #598 the script prefers `cargo deny check advisories` and falls back to
# `cargo audit`; since #621 it skips the scan and reverts the run's bumps when
# neither scanner is installed, rather than failing the run.
#
# These are "what" tests, in the shape of docs_pipeline_accuracy.bats: the
# premise is taken from the script's own observable output, then each workflow
# statement is checked against it. The PR-body assertion executes the real
# workflow step rather than reading its source text.

load helpers

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  SCRIPT="${REPO_ROOT}/bump-deps.sh"
  CI_WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
  UPGRADE_WF="${REPO_ROOT}/.github/workflows/upgrade-dependencies.yml"
  WORK="${BATS_TEST_TMPDIR}/step"
  mkdir -p "$WORK"
}

# --- Premise: the contract the workflows describe ---------------------------

@test "bump-deps.sh prefers cargo deny, falls back to cargo audit, skips without either" {
  run "$SCRIPT" --help
  [ "$status" -eq 0 ]
  # The usage text wraps, so compare against a whitespace-collapsed copy.
  local help
  help="$(printf '%s' "$output" | tr -s '[:space:]' ' ')"
  [[ "$help" == *"cargo deny check advisories"* ]]
  [[ "$help" == *"falling back to cargo audit"* ]]
  [[ "$help" == *"no advisory scanner installed the scan is skipped"* ]]
}

# --- 1. No workflow calls cargo-audit a bump-deps.sh requirement ------------

@test "no workflow describes cargo-audit as required by bump-deps.sh" {
  run python3 - "$CI_WORKFLOW" "$UPGRADE_WF" <<'PY'
import re
import sys

bad = []
for path in sys.argv[1:]:
    for number, line in enumerate(open(path, encoding="utf-8"), 1):
        low = line.lower()
        if "cargo-audit" not in low and "cargo audit" not in low:
            continue
        if re.search(r"require", low) and "bump-deps" in low:
            bad.append(f"{path}:{number}: {line.strip()}")
if bad:
    sys.exit("cargo-audit is a fallback for bump-deps.sh, not a requirement:\n"
             + "\n".join(bad))
PY
  [ "$status" -eq 0 ]
}

# --- 2. The install steps describe the real fallback contract ---------------

@test "each cargo-audit install step names the cargo-deny preference and the fallback" {
  run python3 - "$CI_WORKFLOW" "$UPGRADE_WF" <<'PY'
import sys

def step_block(path):
    """Raw lines of the first step installing cargo-audit, comments included."""
    lines = open(path, encoding="utf-8").read().splitlines()
    start = next(
        (i for i, l in enumerate(lines)
         if l.lstrip().startswith("- name:") and "cargo-audit" in l),
        None,
    )
    if start is None:
        sys.exit(f"{path}: no step installing cargo-audit")
    end = next(
        (j for j in range(start + 1, len(lines))
         if lines[j].lstrip().startswith("- name:")),
        len(lines),
    )
    return "\n".join(lines[start:end])

bad = []
for path in sys.argv[1:]:
    block = step_block(path).lower()
    if "cargo deny" not in block:
        bad.append(f"{path}: install step never names `cargo deny check advisories`")
    if "fallback" not in block and "falls back" not in block:
        bad.append(f"{path}: install step never says cargo-audit is the fallback")
if bad:
    sys.exit("\n".join(bad))
PY
  [ "$status" -eq 0 ]
}

# --- 3. The generated PR body names the advisory scan, not one scanner ------

@test "the generated PR body credits the advisory scan rather than cargo audit alone" {
  require_python3
  extract_step "$UPGRADE_WF" "Build summary" "$WORK"
  read -r -a shell_argv <"${WORK}/shell.cmd"
  (
    cd "$WORK" || exit 1
    printf '  audit tool: cargo deny check advisories\naudit: ok\n' >upgrade.log
    GITHUB_OUTPUT="${WORK}/github_output" "${shell_argv[@]}" step.sh
  )
  body="$(cat "${WORK}/github_output")"
  # The scan is a contract, so the body must claim it …
  [[ "$body" == *"advisory scan"* ]]
  # … but must not tell the reader cargo audit is what passed, when the log it
  # pastes says cargo-deny ran.
  [[ "$body" != *"passed \`cargo audit\`"* ]]
  # The log is still pasted, so the reader can see which scanner ran.
  [[ "$body" == *"audit tool: cargo deny check advisories"* ]]
}
