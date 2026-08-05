#!/usr/bin/env bats
# Regression assertions for Issue #502 (BP-4d74b443d1bd) — two CI blind spots
# lived only in the PR-summary archive:
#
#   1. no PR gate builds `wasm32-unknown-unknown`, so an orphaned
#      `use core::arch::wasm32::{…}` behind `#[cfg(target_arch = "wasm32")]` is
#      green on every host gate and only fails at bundle-build time, after
#      merge (Issues #422, #423, #448);
#   2. a gate conditioned on a file another repo owns self-skips forever, and
#      GitHub rejects `timeout-minutes:` on a reusable-workflow caller job
#      (Issues #379, #333).
#
# Each test pairs a premise assertion against the committed pipeline files with
# a doc assertion against AGENTS.md, so they fail both when the prose drifts and
# when the pipeline changes underneath the prose.

load helpers

setup() {
  require_python3
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  AGENTS="${REPO_ROOT}/AGENTS.md"
  QUALITY="${REPO_ROOT}/quality.sh"
  WORKFLOWS="${REPO_ROOT}/.github/workflows"
  WASM_CHECK='cargo check -p neat-core --target wasm32-unknown-unknown'
}

# Emit the "CI / secrets" section of AGENTS.md, from its heading to the next
# level-2 heading, so a claim elsewhere in the file cannot satisfy a test that
# is about that section.
ci_secrets_section() {
  python3 - "$AGENTS" <<'PY'
import sys

lines = open(sys.argv[1], encoding="utf-8").read().splitlines()
start = next(
    (i for i, l in enumerate(lines) if l.strip().lower() == "## ci / secrets"),
    None,
)
if start is None:
    sys.exit("AGENTS.md has no '## CI / secrets' section")
end = next(
    (i for i in range(start + 1, len(lines)) if lines[i].startswith("## ")),
    len(lines),
)
print("\n".join(lines[start:end]))
PY
}

# --- 1. wasm32 is ungated on PRs -------------------------------------------

@test "no pull_request workflow and no quality.sh step builds for wasm32" {
  # The premise the doc assertions rest on. `rustup target add` alone does not
  # count as a build, so only the compile invocations are searched for.
  run python3 - "$WORKFLOWS" <<'PY'
import pathlib
import re
import sys

import yaml

build = re.compile(
    r"cargo\s+(build|check|clippy|test|rustc)\b[^\n]*wasm32-unknown-unknown"
)
offenders = []
for path in sorted(pathlib.Path(sys.argv[1]).glob("*.yml")):
    data = yaml.safe_load(path.read_text(encoding="utf-8"))
    triggers = data.get("on") or data.get(True) or {}
    names = triggers if isinstance(triggers, (dict, list)) else [triggers]
    if "pull_request" not in names:
        continue
    for job in (data.get("jobs") or {}).values():
        for step in job.get("steps") or []:
            if build.search(step.get("run") or ""):
                offenders.append(f"{path.name}: {step.get('name')}")
if offenders:
    sys.exit("a PR workflow already builds wasm32 — update AGENTS.md:\n"
             + "\n".join(offenders))
PY
  [ "$status" -eq 0 ]

  ! grep -q 'wasm32' "$QUALITY"
}

@test "wasm-bundle.yml builds wasm32 only on push to Develop" {
  run python3 - "${WORKFLOWS}/wasm-bundle.yml" <<'PY'
import sys

import yaml

data = yaml.safe_load(open(sys.argv[1], encoding="utf-8"))
triggers = data.get("on") or data.get(True)
assert set(triggers) == {"push"}, f"expected a push-only trigger, got {triggers}"
assert triggers["push"]["branches"] == ["Develop"], triggers["push"]
body = yaml.safe_dump(data)
assert "wasm32-unknown-unknown" in body, "wasm-bundle.yml never names the target"
PY
  [ "$status" -eq 0 ]
}

@test "AGENTS.md CI section says wasm32 is ungated on PRs and names the manual check" {
  run ci_secrets_section
  [ "$status" -eq 0 ]
  local section="$output"

  printf '%s\n' "$section" | grep -q -- "$WASM_CHECK"
  printf '%s\n' "$section" | grep -q 'wasm-bundle\.yml'
  # The negative claim itself — an agent must be told the gate is absent, not
  # merely handed a command.
  printf '%s\n' "$section" | grep -Eqi 'wasm32.{0,3} is not gated on PRs'
}

@test "AGENTS.md records the WASI bit-diff method for numeric wasm changes" {
  run ci_secrets_section
  [ "$status" -eq 0 ]
  local section="$output"

  printf '%s\n' "$section" | grep -q 'wasm32-wasip1'
  printf '%s\n' "$section" | grep -q 'target-feature=+simd128,+relaxed-simd'
}

@test "the SIMD sections cross-reference the wasm32 check" {
  # The blind spot bites while editing SIMD code, so the pointer has to be
  # reachable from there — not only from the CI section.
  run python3 - "$AGENTS" "$WASM_CHECK" <<'PY'
import sys

path, needle = sys.argv[1], sys.argv[2]
section, hits = None, []
for line in open(path, encoding="utf-8").read().splitlines():
    if line.startswith("## "):
        section = line[3:].strip()
    if needle in line and section and section.lower() != "ci / secrets":
        hits.append(section)
simd = [s for s in hits if "simd" in s.lower() or "wasm" in s.lower()]
if not simd:
    sys.exit(f"no SIMD/wasm section cites {needle!r}; cited by: {hits}")
PY
  [ "$status" -eq 0 ]
}

# --- 2. gates are repo-owned and unconditional ------------------------------

@test "the Mermaid gate is repo-owned and runs unconditionally" {
  # Premise: the gate script lives here, and neither the CI step nor the
  # quality.sh call is guarded by a condition.
  [ -f "${REPO_ROOT}/scripts/check_mermaid.ts" ]

  run python3 - "${WORKFLOWS}/markdown-lint.yml" <<'PY'
import sys

import yaml

data = yaml.safe_load(open(sys.argv[1], encoding="utf-8"))
steps = [
    s
    for job in (data.get("jobs") or {}).values()
    for s in job.get("steps") or []
    if "check_mermaid.ts" in (s.get("run") or "")
]
assert steps, "markdown-lint.yml never runs scripts/check_mermaid.ts"
guarded = [s.get("name") for s in steps if s.get("if")]
assert not guarded, f"Mermaid steps carry an if: guard: {guarded}"
PY
  [ "$status" -eq 0 ]

  # In quality.sh the invocation must sit at top level under `set -e`, not
  # inside an `if`/`command -v` guard that would let a missing tool pass.
  run python3 - "$QUALITY" <<'PY'
import sys

for line in open(sys.argv[1], encoding="utf-8").read().splitlines():
    if "check_mermaid.ts" in line and not line.lstrip().startswith("#"):
        assert line == line.lstrip(), f"conditionally nested invocation: {line!r}"
        break
else:
    sys.exit("quality.sh never runs scripts/check_mermaid.ts")
PY
  [ "$status" -eq 0 ]
}

@test "reusable-workflow callers carry no timeout-minutes and the callee does" {
  run python3 - "${WORKFLOWS}/ci.yml" "${WORKFLOWS}/security.yml" <<'PY'
import sys

import yaml

caller = yaml.safe_load(open(sys.argv[1], encoding="utf-8"))
callee = yaml.safe_load(open(sys.argv[2], encoding="utf-8"))

callers = {
    name: job
    for name, job in (caller.get("jobs") or {}).items()
    if job.get("uses")
}
assert callers, "ci.yml calls no reusable workflow"
bad = [n for n, j in callers.items() if "timeout-minutes" in j]
assert not bad, f"GitHub rejects timeout-minutes on caller jobs: {bad}"

for name, job in (callee.get("jobs") or {}).items():
    budget = job.get("timeout-minutes")
    assert isinstance(budget, int) and budget > 0, (
        f"security.yml job {name!r} has no job-level timeout-minutes"
    )
PY
  [ "$status" -eq 0 ]
}

@test "AGENTS.md CI section states the repo-owned, unconditional gate rule" {
  run ci_secrets_section
  [ "$status" -eq 0 ]
  local section="$output"

  printf '%s\n' "$section" | grep -Eqi 'repo-owned and unconditional'
  printf '%s\n' "$section" | grep -q 'check_mermaid\.ts'
  printf '%s\n' "$section" | grep -Eqi 'timeout-minutes.*(caller|reusable)'
  printf '%s\n' "$section" | grep -q 'security\.yml'
}
