#!/usr/bin/env bats
# Tests for the scheduled Deno dependency-update workflow (Issue #646).
#
# `upgrade-dependencies.yml` refreshes the Cargo half of the tree every Monday
# through `bump-deps.sh`; the JSR half was bumped only by hand, so a `@std/*`
# release could sit unnoticed between the quarantine window closing and somebody
# remembering to run `deno outdated`. This workflow closes that gap with Deno's
# own tooling — the release-age quarantine lives in `deno.json`
# (`minimumDependencyAge`), never in a hand-rolled age gate here.
#
# Two failure modes are what these tests actually guard, and both are silent:
#
#   1. `deno outdated` only sees dependencies declared in `deno.json`. A tree
#      whose sources carry inline `jsr:@std/assert@1` specifiers has nothing for
#      the workflow to update, so the run is green forever and bumps nothing.
#   2. `deno.json` marks the lockfile **frozen**. Without `--frozen=false` the
#      update rewrites `deno.json` and then refuses to write `deno.lock`,
#      leaving a tree whose every Deno gate fails with "lockfile is out of
#      date" — while the step itself still exits 0.
#
# These are "what" tests: the refresh step is extracted from the workflow and
# executed for real, under the shell GitHub would use, against a throwaway
# workspace built from the repository's own committed `deno.json`/`deno.lock`.

load helpers

# A published @std/assert release old enough to clear the P1D quarantine and far
# enough behind that `--latest` always has somewhere to move it.
OLD_PIN="1.0.10"

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/deno-outdated.yml"
  WORK="${BATS_TEST_TMPDIR}/step"
  mkdir -p "$WORK"
}

require_deno() {
  command -v deno &>/dev/null || skip "deno required to execute the refresh step"
}

# A throwaway workspace carrying the repository's *own* `deno.json` and
# `deno.lock` with the @std/assert import pinned back, so a real update run has
# something to move. The config under test is the committed one, not a copy
# written by the test.
fixture_workspace() {
  cp "${REPO_ROOT}/deno.json" "${WORK}/deno.json"
  python3 - "${WORK}/deno.json" "$OLD_PIN" <<'PY'
import json
import sys

path, old = sys.argv[1], sys.argv[2]
with open(path, encoding="utf-8") as fh:
    config = json.load(fh)
imports = config.get("imports") or {}
assert "@std/assert" in imports, (
    f"deno.json declares no @std/assert import, so `deno outdated` can see "
    f"nothing to update: {imports}"
)
imports["@std/assert"] = f"jsr:@std/assert@{old}"
with open(path, "w", encoding="utf-8") as fh:
    json.dump(config, fh, indent=2)
PY
  printf 'import { assertEquals } from "@std/assert";\nassertEquals(1, 1);\n' \
    >"${WORK}/mod.ts"
  # A lockfile matching the down-pinned config: the committed one pins the
  # current release, which the frozen gate would reject before the step ran.
  (cd "$WORK" && deno install --frozen=false >/dev/null 2>&1)
}

# Execute the extracted step in the fixture, with $1 (optional) prepended to
# PATH so a stub `deno` can stand in for the real one.
run_refresh_step() {
  local stub_dir="${1:-}"
  read -r -a shell_argv <"${WORK}/shell.cmd"
  (
    cd "$WORK" || exit 1
    [ -n "$stub_dir" ] && export PATH="${stub_dir}:${PATH}"
    "${shell_argv[@]}" step.sh
  )
}

# Write a stub `deno` into $WORK/stub whose body is $1.
stub_deno() {
  mkdir -p "${WORK}/stub"
  {
    echo '#!/usr/bin/env bash'
    echo "$1"
  } >"${WORK}/stub/deno"
  chmod +x "${WORK}/stub/deno"
  echo "${WORK}/stub"
}

@test "deno-outdated workflow file exists and is valid YAML" {
  require_python3
  [ -f "$WORKFLOW" ]
  run python3 -c "import yaml; yaml.safe_load(open('$WORKFLOW'))"
  [ "$status" -eq 0 ]
}

@test "the update runs on the weekly cron and on demand, never on a pull request" {
  require_python3
  run python3 - "$WORKFLOW" <<'PY'
import sys

import yaml

data = yaml.safe_load(open(sys.argv[1], encoding="utf-8"))
# PyYAML resolves the bare `on:` key to the boolean True.
triggers = data.get("on") or data.get(True)
assert triggers is not None, data
assert "workflow_dispatch" in triggers, triggers
assert "pull_request" not in triggers, (
    "a dependency bump must not run on every PR — it opens PRs"
)
crons = [entry["cron"] for entry in triggers["schedule"]]
assert crons == ["0 6 * * 1"], (
    f"expected the Monday 06:00 slot upgrade-dependencies.yml uses, got {crons}"
)
PY
  [ "$status" -eq 0 ]
}

@test "the job holds only the write scopes opening a PR needs" {
  require_python3
  run python3 - "$WORKFLOW" <<'PY'
import sys

import yaml

data = yaml.safe_load(open(sys.argv[1], encoding="utf-8"))
jobs = data.get("jobs") or {}
assert len(jobs) == 1, f"expected a single job, got {sorted(jobs)}"
(job,) = jobs.values()
perms = job.get("permissions") or data.get("permissions")
assert perms is not None, "no permissions block at job or workflow level"
writes = sorted(scope for scope, level in perms.items() if level == "write")
assert writes == ["contents", "pull-requests"], (
    f"expected exactly contents+pull-requests write, got {writes}"
)
PY
  [ "$status" -eq 0 ]
}

@test "the refresh step updates an outdated JSR dependency and rewrites the frozen lockfile" {
  require_python3
  require_deno
  extract_step "$WORKFLOW" "Refresh Deno dependencies" "$WORK"
  fixture_workspace

  run run_refresh_step
  echo "$output"
  [ "$status" -eq 0 ]

  # The import map entry moved off the down-pinned version…
  run python3 - "${WORK}/deno.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as fh:
    print(json.load(fh)["imports"]["@std/assert"])
PY
  [ "$status" -eq 0 ]
  [ "$output" != "jsr:@std/assert@${OLD_PIN}" ]

  # …and the lockfile it left behind satisfies the frozen gate, so the PR the
  # workflow opens is not red on arrival.
  run bash -c 'cd "$1" && deno check mod.ts' _ "$WORK"
  echo "$output"
  [ "$status" -eq 0 ]
}

@test "the refresh step captures the update log for the PR body" {
  require_python3
  require_deno
  extract_step "$WORKFLOW" "Refresh Deno dependencies" "$WORK"
  fixture_workspace

  run run_refresh_step
  [ "$status" -eq 0 ]
  run cat "${WORK}/deno-outdated.log"
  [ "$status" -eq 0 ]
  [[ "$output" == *"@std/assert"* ]]
}

@test "a failing deno fails the refresh step rather than opening an empty PR" {
  require_python3
  extract_step "$WORKFLOW" "Refresh Deno dependencies" "$WORK"
  local stub
  stub="$(stub_deno 'echo "stub deno: $*" >&2; exit 3')"
  run run_refresh_step "$stub"
  [ "$status" -ne 0 ]
}

@test "a lockfile left out of date by the update fails the refresh step" {
  require_python3
  extract_step "$WORKFLOW" "Refresh Deno dependencies" "$WORK"
  # `deno outdated` succeeds, but the lockfile it leaves behind no longer
  # matches the config — the exact silent breakage `--frozen=false` prevents.
  local stub
  stub="$(stub_deno '
case "$1" in
  outdated) echo "Updated 1 dependency: jsr:@std/assert"; exit 0 ;;
  *) echo "error: The lockfile is out of date." >&2; exit 1 ;;
esac')"
  run run_refresh_step "$stub"
  echo "$output"
  [ "$status" -ne 0 ]
}

@test "create-pull-request authenticates with ACTIONS_PUSH and falls back to GITHUB_TOKEN" {
  require_python3
  run python3 - "$WORKFLOW" <<'PY'
import re
import sys

import yaml

data = yaml.safe_load(open(sys.argv[1], encoding="utf-8"))
steps = [
    step
    for job in (data.get("jobs") or {}).values()
    for step in job.get("steps") or []
    if "peter-evans/create-pull-request" in str(step.get("uses", ""))
]
assert len(steps) == 1, f"expected one create-pull-request step, got {len(steps)}"
token = (steps[0].get("with") or {}).get("token", "")
# A bare GITHUB_TOKEN makes GitHub suppress the created PR's own workflow
# triggers, so CI never fires on the bump (Issue #168 / #1636).
assert re.fullmatch(
    r"\$\{\{\s*secrets\.ACTIONS_PUSH\s*\|\|\s*secrets\.GITHUB_TOKEN\s*\}\}",
    token.strip(),
), f"token must prefer ACTIONS_PUSH and fall back to GITHUB_TOKEN, got {token!r}"
PY
  [ "$status" -eq 0 ]
}

@test "the pull request commits the dependency files only, never the update log" {
  require_python3
  run python3 - "$WORKFLOW" <<'PY'
import sys

import yaml

data = yaml.safe_load(open(sys.argv[1], encoding="utf-8"))
(step,) = [
    step
    for job in (data.get("jobs") or {}).values()
    for step in job.get("steps") or []
    if "peter-evans/create-pull-request" in str(step.get("uses", ""))
]
with_ = step.get("with") or {}
paths = [p for p in str(with_.get("add-paths", "")).split() if p]
assert paths == ["deno.json", "deno.lock"], (
    f"expected add-paths to cover the dependency files only, got {paths}"
)
assert with_.get("base") == "Develop", with_.get("base")
PY
  [ "$status" -eq 0 ]
}

@test "the Deno toolchain is pinned exactly, matching the other Deno jobs" {
  require_python3
  run python3 - "$WORKFLOW" "${REPO_ROOT}/.github/workflows/ci.yml" <<'PY'
import sys

import yaml


def deno_versions(path):
    data = yaml.safe_load(open(path, encoding="utf-8"))
    return [
        (step.get("with") or {}).get("deno-version")
        for job in (data.get("jobs") or {}).values()
        for step in job.get("steps") or []
        if "denoland/setup-deno" in str(step.get("uses", ""))
    ]


(version,) = deno_versions(sys.argv[1])
# A floating `v2.x` would change the updater underneath the bump it proposes.
assert version is not None and "x" not in str(version), (
    f"deno-version must be an exact release, got {version!r}"
)
others = set(v for v in deno_versions(sys.argv[2]) if v)
assert version in others, (
    f"deno-version {version!r} does not match ci.yml's {sorted(others)}"
)
PY
  [ "$status" -eq 0 ]
}
