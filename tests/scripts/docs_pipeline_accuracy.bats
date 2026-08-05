#!/usr/bin/env bats
# Regression assertions for Issue #500 (BP-ccf465f45672) — AGENTS.md and
# README.md misdescribed the automated dependency/CI pipeline in three places:
# an ex-`cargo upgrade --incompatible` PR step that was deliberately replaced by
# the quarantine-aware `bump-deps.sh`, a Dependabot "security-updates channel"
# the committed config does not configure, and a `quality.sh` invocation no
# workflow makes.
#
# These are "what" tests: each one reads the published doc artefact and the
# committed pipeline file it describes, then asserts the two agree. They fail if
# the docs drift back, and they also fail if the pipeline changes underneath the
# docs — which is the point, since an agent acting on the stale prose would
# reintroduce the quarantine bypass `ci.yml` warns against.

load helpers

setup() {
  require_python3
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  README="${REPO_ROOT}/README.md"
  AGENTS="${REPO_ROOT}/AGENTS.md"
  CI_WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
  DEPENDABOT_FILE="${REPO_ROOT}/.github/dependabot.yml"
}

# --- 1. AGENTS.md: the PR pipeline bumps deps via bump-deps.sh --------------

@test "ci.yml refreshes dependencies through bump-deps.sh, not cargo upgrade" {
  # The premise the doc assertions below rest on: verify it against the
  # workflow rather than trusting the prose.
  run strip_comments "$CI_WORKFLOW"
  [ "$status" -eq 0 ]
  printf '%s\n' "$output" | grep -q 'bump-deps\.sh'
  ! printf '%s\n' "$output" | grep -q 'cargo upgrade'
}

@test "AGENTS.md describes the PR dep refresh as bump-deps.sh" {
  run python3 - "$AGENTS" <<'PY'
import re, sys
text = open(sys.argv[1], encoding="utf-8").read()
line = next((l for l in text.splitlines() if "PR pipeline:" in l), None)
if line is None:
    sys.exit("no 'PR pipeline:' bullet found in AGENTS.md")
if "bump-deps.sh" not in line:
    sys.exit(f"PR-pipeline bullet does not name bump-deps.sh: {line}")
PY
  [ "$status" -eq 0 ]
}

@test "AGENTS.md never attributes cargo upgrade to the PR pipeline" {
  # `cargo upgrade --incompatible` survives only in quality.sh, the local gate.
  # Any AGENTS.md mention of it must say so on the same line.
  run python3 - "$AGENTS" <<'PY'
import sys
bad = [
    l for l in open(sys.argv[1], encoding="utf-8").read().splitlines()
    if "cargo upgrade" in l and "quality.sh" not in l
]
if bad:
    sys.exit("cargo upgrade mentioned without the local-only quality.sh "
             "qualifier:\n" + "\n".join(bad))
PY
  [ "$status" -eq 0 ]
}

# --- 2. README: Dependabot is a weekly version-updates channel --------------

@test "the committed dependabot.yml configures weekly version updates only" {
  run python3 - "$DEPENDABOT_FILE" <<'PY'
import sys, yaml
data = yaml.safe_load(open(sys.argv[1], encoding="utf-8"))
entry = next(u for u in data["updates"] if u["package-ecosystem"] == "cargo")
assert entry["schedule"]["interval"] == "weekly", entry["schedule"]
assert entry["cooldown"]["default-days"] == 7, entry["cooldown"]
PY
  [ "$status" -eq 0 ]
}

@test "README describes dependabot.yml as the weekly cargo version-updates channel" {
  run python3 - "$README" <<'PY'
import re, sys
text = open(sys.argv[1], encoding="utf-8").read().lower()
for needle in ("version-updates", "weekly", "cooldown"):
    if needle not in text:
        sys.exit(f"README never mentions {needle!r} for the Dependabot channel")
PY
  [ "$status" -eq 0 ]
}

@test "README does not claim the committed file enables a security-updates channel" {
  # Dependabot *security* updates are a repository-level setting, invisible in
  # the tree — so no sentence may tie them to dependabot.yml as configuration
  # without saying they are a repository setting.
  run python3 - "$README" <<'PY'
import sys
text = open(sys.argv[1], encoding="utf-8").read()

# Prose is checked a paragraph at a time (a claim and its qualifier may sit on
# different lines); fenced blocks — Mermaid nodes especially — a line at a time,
# since neighbouring nodes carry no qualifying context.
units, buf, in_fence = [], [], False
for line in text.splitlines():
    if line.startswith("```"):
        in_fence = not in_fence
        units.append("\n".join(buf)); buf = []
        continue
    if in_fence:
        units.append(line)
    elif line.strip():
        buf.append(line)
    else:
        units.append("\n".join(buf)); buf = []
units.append("\n".join(buf))

bad = []
for para in units:
    low = para.lower()
    if "dependabot" in low and "security" in low and "repository" not in low:
        bad.append(para.strip())
if bad:
    sys.exit("README ties Dependabot security updates to the committed config "
             "without noting they are a repository setting:\n" + "\n".join(bad))
PY
  [ "$status" -eq 0 ]
}

@test "README links GitHub's Dependabot security-updates documentation" {
  run grep -q 'docs.github.com/[^)]*dependabot-security-updates' "$README"
  [ "$status" -eq 0 ]
}

# --- 3. README: no workflow runs quality.sh ---------------------------------

@test "no workflow invokes quality.sh" {
  # Premise for the assertions below: quality.sh is the local/pre-commit gate;
  # CI re-implements the equivalent steps in the `quality` job.
  local wf
  for wf in "${REPO_ROOT}"/.github/workflows/*.yml; do
    run strip_comments "$wf"
    [ "$status" -eq 0 ]
    if printf '%s\n' "$output" | grep -q 'quality\.sh'; then
      printf 'quality.sh invoked by %s\n' "$wf" >&2
      return 1
    fi
  done
}

@test "README does not claim bump-deps.sh runs before quality.sh in CI" {
  run python3 - "$README" <<'PY'
import re, sys
bad = [
    l for l in open(sys.argv[1], encoding="utf-8").read().splitlines()
    if re.search(r"bump-deps\.sh.*before.*quality\.sh", l)
]
if bad:
    sys.exit("README claims CI runs bump-deps.sh before quality.sh:\n"
             + "\n".join(bad))
PY
  [ "$status" -eq 0 ]
}

@test "README credits the CI quality job with applying the same gates" {
  run python3 - "$README" <<'PY'
import re, sys
text = open(sys.argv[1], encoding="utf-8").read()
if not re.search(r"`quality`\s+job", text):
    sys.exit("README never explains that the CI `quality` job applies the "
             "gates quality.sh runs locally")
PY
  [ "$status" -eq 0 ]
}
