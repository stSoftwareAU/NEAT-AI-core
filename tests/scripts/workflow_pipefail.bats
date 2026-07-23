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

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOWS_DIR="${REPO_ROOT}/.github/workflows"
  UPGRADE_WF="${WORKFLOWS_DIR}/upgrade-dependencies.yml"
  WORK="${BATS_TEST_TMPDIR}/step"
  mkdir -p "$WORK"
}

# Write the named step's script to $WORK/step.sh and the argv GitHub would
# launch it with to $WORK/shell.cmd.
extract_step() {
  python3 - "$1" "$2" "$WORK" <<'PY'
import os, sys, yaml

workflow, needle, out = sys.argv[1:4]
with open(workflow) as fh:
    data = yaml.safe_load(fh)

def declared_shell(step, job):
    for scope in (step, job.get("defaults", {}).get("run", {}),
                  (data.get("defaults") or {}).get("run", {})):
        if scope.get("shell"):
            return scope["shell"]
    return None

for job in (data.get("jobs") or {}).values():
    for step in job.get("steps") or []:
        if needle not in (step.get("name") or "") or "run" not in step:
            continue
        body = step["run"]
        assert "${{" not in body, "step body interpolates a GitHub expression"
        shell = declared_shell(step, job)
        # GitHub: no `shell:` → `bash -e {0}`; `shell: bash` → `bash
        # --noprofile --norc -eo pipefail {0}`.
        argv = {
            None: "bash -e",
            "bash": "bash --noprofile --norc -eo pipefail",
        }.get(shell)
        assert argv, f"unsupported shell for this harness: {shell!r}"
        with open(os.path.join(out, "step.sh"), "w") as fh:
            fh.write(body)
        with open(os.path.join(out, "shell.cmd"), "w") as fh:
            fh.write(argv)
        sys.exit(0)

sys.exit(f"no step named like {needle!r} with a run: block in {workflow}")
PY
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
  extract_step "$UPGRADE_WF" "Refresh dependencies via bump-deps.sh"
  run run_step_with_stub 7
  [ "$status" -ne 0 ]
}

@test "a passing bump-deps.sh keeps the scheduled upgrade step green" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  extract_step "$UPGRADE_WF" "Refresh dependencies via bump-deps.sh"
  run run_step_with_stub 0
  [ "$status" -eq 0 ]
}

# The PR body pastes upgrade.log, so tee must keep capturing output.
@test "the scheduled upgrade step still captures bump-deps.sh output to upgrade.log" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  extract_step "$UPGRADE_WF" "Refresh dependencies via bump-deps.sh"
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
