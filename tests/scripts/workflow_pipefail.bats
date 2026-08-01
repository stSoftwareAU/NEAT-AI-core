#!/usr/bin/env bats
# Regression tests for Issue #336 — a failing `bump-deps.sh` must fail the
# scheduled upgrade step.
#
# GitHub runs a `run:` block with no `shell:` override under `bash -e {0}` —
# `-e` but *not* `pipefail`. In `cmd | tee upgrade.log` the pipeline's status is
# `tee`'s (always 0), so a non-zero exit from `bump-deps.sh` (quarantine
# violation, `cargo audit` advisory, failed native/wasm build) is swallowed and
# the workflow goes on to open a PR asserting the bumps passed those gates.
#
# These are "what" tests: they extract the step's real script from the workflow,
# execute it under the same shell GitHub would use with a stub `bump-deps.sh`,
# and assert on the observed exit status — not on source text.
#
# The step extractor is shared via helpers.bash (Issue #477).

load helpers

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOWS_DIR="${REPO_ROOT}/.github/workflows"
  UPGRADE_WF="${WORKFLOWS_DIR}/upgrade-dependencies.yml"
  WORK="${BATS_TEST_TMPDIR}/step"
  mkdir -p "$WORK"
}

# Run the extracted step with a stub `bump-deps.sh` that exits $1.
run_step_with_stub() {
  cat >"${WORK}/bump-deps.sh" <<EOF
#!/usr/bin/env bash
echo "stub bump-deps.sh invoked: \$*"
exit $1
EOF
  chmod +x "${WORK}/bump-deps.sh"
  read -r -a shell_argv <"${WORK}/shell.cmd"
  (
    cd "$WORK" || exit 1
    VIBE_BUMP_QUARANTINE_HOURS=24 "${shell_argv[@]}" step.sh
  )
}

@test "a failing bump-deps.sh fails the scheduled upgrade step" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  extract_step "$UPGRADE_WF" "Refresh dependencies via bump-deps.sh" "$WORK"
  run run_step_with_stub 7
  [ "$status" -ne 0 ]
}

@test "a passing bump-deps.sh keeps the scheduled upgrade step green" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  extract_step "$UPGRADE_WF" "Refresh dependencies via bump-deps.sh" "$WORK"
  run run_step_with_stub 0
  [ "$status" -eq 0 ]
}

# The PR body pastes upgrade.log, so tee must keep capturing output.
@test "the scheduled upgrade step still captures bump-deps.sh output to upgrade.log" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  extract_step "$UPGRADE_WF" "Refresh dependencies via bump-deps.sh" "$WORK"
  run run_step_with_stub 0
  [ "$status" -eq 0 ]
  run cat "${WORK}/upgrade.log"
  [ "$status" -eq 0 ]
  [[ "$output" == *"stub bump-deps.sh invoked"* ]]
  [[ "$output" == *"--quarantine-hours 24"* ]]
}

# Guard the whole workflow, not just today's step: any future piped command
# here would inherit the same swallowed-exit-status bug.
@test "every piped run: block in upgrade-dependencies.yml runs under pipefail" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - "$UPGRADE_WF" <<'PY'
import re, sys, yaml

workflow = sys.argv[1]
with open(workflow) as fh:
    data = yaml.safe_load(fh)

bad = []
for job_name, job in (data.get("jobs") or {}).items():
    for step in job.get("steps") or []:
        body = step.get("run")
        if not body:
            continue
        code = "\n".join(
            line for line in body.splitlines() if not line.strip().startswith("#")
        )
        # Ignore `||` and GitHub expressions when looking for a real pipeline.
        scanned = re.sub(r"\|\||\$\{\{.*?\}\}", "", code)
        if "|" not in scanned:
            continue
        shell = step.get("shell") or job.get("defaults", {}).get("run", {}).get("shell")
        if shell == "bash" or re.search(r"^\s*set\s+-\S*o?\s*.*pipefail", code, re.M):
            continue
        bad.append(f"{job_name}: {step.get('name')!r} pipes without pipefail")

if bad:
    sys.stderr.write("Swallowed pipeline exit status:\n  " + "\n  ".join(bad) + "\n")
    sys.exit(1)
PY
  [ "$status" -eq 0 ]
}
