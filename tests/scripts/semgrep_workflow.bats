#!/usr/bin/env bats
# Tests for the Semgrep GitHub Actions workflow.
#
# Issue #330 — milestone sub-issue PRs target a shared `milestone/<slug>`
# branch. GitHub branch-filter globs treat `*` as "any chars except /", so a
# filter of ["*"] never matches `milestone/<slug>` and this scan silently
# skips those PRs; the gap only surfaces on the single rollup PR into the
# default branch.
#
# These are "what" tests — they assert on the YAML the runner will execute,
# not on commentary or surrounding prose.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WF="${REPO_ROOT}/.github/workflows/semgrep.yml"
}

@test "semgrep.yml exists and is valid YAML" {
  [ -f "$WF" ]
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 -c "import yaml; yaml.safe_load(open('$WF'))"
  [ "$status" -eq 0 ]
}

@test "semgrep.yml pull_request filter matches milestone branches" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import re, yaml
data = yaml.safe_load(open("$WF"))
# YAML parses bare 'on:' as boolean True in some loaders; tolerate both.
triggers = data.get("on") or data.get(True)
pr = triggers["pull_request"]
patterns = pr.get("branches") or []
assert patterns, f"pull_request has no branches filter: {pr}"

def matches(pattern, branch):
    # GitHub filter globbing: ** crosses '/', * does not.
    regex = ""
    i = 0
    while i < len(pattern):
        c = pattern[i]
        if c == "*":
            if pattern[i + 1 : i + 2] == "*":
                regex += ".*"
                i += 2
                continue
            regex += "[^/]*"
        else:
            regex += re.escape(c)
        i += 1
    return re.fullmatch(regex, branch) is not None

branch = "milestone/clean-up-23-jul"
assert any(matches(p, branch) for p in patterns), (
    f"no branch pattern matches {branch!r}: {patterns}"
)
# The existing default branches must still match.
for keep in ("Develop", "main"):
    assert any(matches(p, keep) for p in patterns), (
        f"no branch pattern matches {keep!r}: {patterns}"
    )
PY
  [ "$status" -eq 0 ]
}

@test "semgrep.yml runs the semgrep scan on pull requests" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WF"))
job = data["jobs"]["semgrep"]
runs = [s.get("run", "") for s in job["steps"]]
assert any("semgrep" in r for r in runs), runs
PY
  [ "$status" -eq 0 ]
}

# --- PR #393: registry-pull resilience -------------------------------------
#
# The job used a job-level `container:`, which the runner pulls during
# "Initialize containers" with a fixed 3 attempts ~3s apart. A Docker Hub
# timeout there failed the job before any step ran and surfaced as a SAST
# failure with no scan performed. The pull now lives in a step whose backoff we
# own. These tests execute that step's real script against a stubbed `docker`.

# Writes the named step's `run:` script to $1. Returns 1 if the step is absent.
extract_step_script() {
  python3 - "$1" "$2" <<'PY'
import sys, yaml
out_path, step_name = sys.argv[1], sys.argv[2]
data = yaml.safe_load(open(__import__("os").environ["WF"]))
steps = data["jobs"]["semgrep"]["steps"]
for step in steps:
    if step.get("name") == step_name:
        open(out_path, "w").write(step["run"])
        sys.exit(0)
sys.exit(1)
PY
}

# Stubs `docker` (fails the first $1 invocations, then succeeds) and `sleep`
# (returns immediately so backoff does not slow the suite), on PATH.
make_stubs() {
  local fail_count="$1"
  mkdir -p "${BATS_TEST_TMPDIR}/bin"
  echo 0 >"${BATS_TEST_TMPDIR}/calls"
  cat >"${BATS_TEST_TMPDIR}/bin/docker" <<STUB
#!/usr/bin/env bash
calls=\$(cat "${BATS_TEST_TMPDIR}/calls")
calls=\$((calls + 1))
echo "\$calls" >"${BATS_TEST_TMPDIR}/calls"
echo "docker \$*" >>"${BATS_TEST_TMPDIR}/args"
if [ "\$calls" -le "${fail_count}" ]; then
  echo 'Error response from daemon: context deadline exceeded' >&2
  exit 1
fi
exit 0
STUB
  cat >"${BATS_TEST_TMPDIR}/bin/sleep" <<'STUB'
#!/usr/bin/env bash
exit 0
STUB
  chmod +x "${BATS_TEST_TMPDIR}/bin/docker" "${BATS_TEST_TMPDIR}/bin/sleep"
  PATH="${BATS_TEST_TMPDIR}/bin:$PATH"
  export PATH
}

@test "pull step retries a failing registry and succeeds once the pull works" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  export WF
  run extract_step_script "${BATS_TEST_TMPDIR}/pull.sh" "Pull semgrep image"
  [ "$status" -eq 0 ]

  make_stubs 2 # two registry timeouts, then a good pull
  export SEMGREP_IMAGE="semgrep/semgrep@sha256:$(printf 'a%.0s' {1..64})"
  run bash "${BATS_TEST_TMPDIR}/pull.sh"
  [ "$status" -eq 0 ]
  # Three attempts total: the runner's own 3-attempt job-init pull is what
  # failed here, so a single retry would not have been enough.
  [ "$(cat "${BATS_TEST_TMPDIR}/calls")" -eq 3 ]
}

@test "pull step fails loud when the registry stays unreachable" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  export WF
  run extract_step_script "${BATS_TEST_TMPDIR}/pull.sh" "Pull semgrep image"
  [ "$status" -eq 0 ]

  make_stubs 99 # registry never recovers
  export SEMGREP_IMAGE="semgrep/semgrep@sha256:$(printf 'a%.0s' {1..64})"
  run bash "${BATS_TEST_TMPDIR}/pull.sh"
  # An unpullable scanner must never be reported as a clean scan.
  [ "$status" -ne 0 ]
  [[ "$output" == *"Could not pull"* ]]
  [[ "$output" == *"no SAST scan ran"* ]]
}

@test "semgrep.yml has no job-level container, so the pull backoff is ours" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
job = yaml.safe_load(open("$WF"))["jobs"]["semgrep"]
assert not job.get("container"), (
    "a job-level container: is pulled at job init with a fixed ~3s x3 backoff "
    "that cannot be tuned; PR #393 moved the pull into a step for that reason"
)
PY
  [ "$status" -eq 0 ]
}

@test "the scan runs semgrep inside the digest-pinned image" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import re, yaml
data = yaml.safe_load(open("$WF"))
image = (data.get("env") or {}).get("SEMGREP_IMAGE", "")
assert re.fullmatch(r"[A-Za-z0-9_.\-/:]+@sha256:[0-9a-f]{64}", image), image

steps = data["jobs"]["semgrep"]["steps"]
scan = [s for s in steps if s.get("name") == "Semgrep scan"]
assert scan, [s.get("name") for s in steps]
run = scan[0]["run"]
assert "semgrep ci --config p/default" in run, run
assert "\$SEMGREP_IMAGE" in run, "scan must use the pinned ref, not a tag"
assert scan[0]["env"]["SEMGREP_APP_TOKEN"], scan[0].get("env")
PY
  [ "$status" -eq 0 ]
}
