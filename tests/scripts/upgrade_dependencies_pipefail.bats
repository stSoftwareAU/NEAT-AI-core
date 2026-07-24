#!/usr/bin/env bats
# Regression assertion for Issue #336 — the scheduled dependency-upgrade
# workflow pipes bump-deps.sh through `tee upgrade.log`. GitHub's default
# `run` shell is `bash -e {0}` WITHOUT `pipefail`, so the pipeline's exit
# status is tee's (always 0) and a non-zero exit from bump-deps.sh — the
# documented "the bump is unsafe" signal (release-age quarantine violation,
# cargo audit advisory, or a failed native/wasm build) — is swallowed. The
# workflow then files a PR asserting the bumps passed cargo audit and the
# dual build, a claim that may be false. The step must enable pipefail so a
# failed bump-deps.sh fails the step. These are behavioural "what" tests: they
# execute the step's actual run body against a failing stub and assert on the
# observed exit status.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  UPGRADE_WF="${REPO_ROOT}/.github/workflows/upgrade-dependencies.yml"
  WORK="$(mktemp -d)"
}

teardown() {
  [ -n "${WORK:-}" ] && rm -rf "$WORK"
}

# Extract and dedent the `run:` block-scalar body of the
# "Refresh dependencies via bump-deps.sh" step from the workflow YAML.
# Steps are indented 6 spaces (`      - name:`), keys 8 spaces, block body 10.
extract_refresh_run_body() {
  awk '
    /^      - name: Refresh dependencies via bump-deps\.sh/ { instep = 1 }
    instep && /^        run: \|/ { inrun = 1; next }
    inrun {
      if ($0 ~ /^[[:space:]]*$/) { print ""; next }         # keep blank lines
      if ($0 ~ /^      - / || $0 ~ /^        [^ ]/) {        # next step / key
        inrun = 0; instep = 0; next
      }
      sub(/^          /, "")                                 # dedent 10 spaces
      print
    }
  ' "$UPGRADE_WF"
}

@test "upgrade-dependencies.yml exists" {
  [ -f "$UPGRADE_WF" ]
}

@test "the Refresh step run body is extractable and invokes bump-deps.sh via tee" {
  body="$(extract_refresh_run_body)"
  [ -n "$body" ]
  printf '%s\n' "$body" | grep -q 'bump-deps.sh'
  printf '%s\n' "$body" | grep -q 'tee upgrade.log'
}

@test "a failing bump-deps.sh fails the Refresh step (pipefail is enabled)" {
  # Stub bump-deps.sh so it fails the way an unsafe bump would.
  cat > "$WORK/bump-deps.sh" <<'STUB'
#!/usr/bin/env bash
echo "stub: unsafe bump — cargo audit advisory" >&2
exit 1
STUB
  chmod +x "$WORK/bump-deps.sh"

  extract_refresh_run_body > "$WORK/step.sh"

  # Reproduce GitHub's default (unspecified-shell) semantics: `bash -e {0}`
  # WITHOUT pipefail. The fix must re-enable pipefail from inside the run body.
  # `run` captures the exit status without tripping bats' own errexit.
  # shellcheck disable=SC2016  # $1 is a positional for the inner bash, must not expand here
  run env VIBE_BUMP_QUARANTINE_HOURS=24 bash -c 'cd "$1" && bash -e step.sh' _ "$WORK"

  [ "$status" -ne 0 ]
}

@test "a succeeding bump-deps.sh still passes the Refresh step" {
  cat > "$WORK/bump-deps.sh" <<'STUB'
#!/usr/bin/env bash
echo "stub: bump applied, cargo audit clean, dual build ok"
exit 0
STUB
  chmod +x "$WORK/bump-deps.sh"

  extract_refresh_run_body > "$WORK/step.sh"

  # shellcheck disable=SC2016  # $1 is a positional for the inner bash, must not expand here
  run env VIBE_BUMP_QUARANTINE_HOURS=24 bash -c 'cd "$1" && bash -e step.sh' _ "$WORK"

  [ "$status" -eq 0 ]
  [ -f "$WORK/upgrade.log" ]
}
