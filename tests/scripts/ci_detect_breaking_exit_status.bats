#!/usr/bin/env bats
# Issue #634 — both ci.yml callers of scripts/detect-breaking.sh must fail loud
# when the detector fails or answers with something other than true/false.
#
# `if [ "$(scripts/detect-breaking.sh "$RANGE")" = "true" ]` discards the
# script's exit status: a bad range, a git failure or the option-shaped-range
# guard (#608) yields an empty substitution, the comparison is false, and the
# lane proceeds as "not breaking" — so a breaking change could ship on a patch
# bump because the detector failed rather than because it answered `false`.
#
# These cases drive the real step bodies extracted from ci.yml against a stub
# detect-breaking.sh, so they fail if either guard is removed.

load helpers

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  CI_WF="${REPO_ROOT}/.github/workflows/ci.yml"
  WORK="${BATS_TEST_TMPDIR}/step"
  mkdir -p "$WORK"
}

# make_repo <dir> <detector-stdout> <detector-exit> [base-ref] — a throwaway
# checkout holding a stub detect-breaking.sh with the given behaviour, plus
# no-op stubs for every other helper the step shells out to. With <base-ref>
# the repo also carries refs/remotes/origin/<base-ref>, which the version gate
# needs before it will read a base version and reach the detector at all.
make_repo() {
  local repo="$1" out="$2" code="$3" base_ref="${4:-}"
  mkdir -p "$repo/scripts" "$repo/bin"

  cat >"$repo/Cargo.toml" <<'TOML'
[package]
name = "stub"
version = "0.1.0"
TOML

  cat >"$repo/scripts/detect-breaking.sh" <<SH
#!/usr/bin/env bash
printf '%s' '${out}'
[ -n '${out}' ] && echo
echo "stub detect-breaking.sh: exiting ${code}" >&2
exit ${code}
SH

  # Marker files record which helpers ran, so a test can assert the step
  # stopped at the detector rather than carrying on with a bogus signal.
  cat >"$repo/scripts/check-version-bump.sh" <<'SH'
#!/usr/bin/env bash
touch check-version-bump.ran
SH
  cat >"$repo/scripts/version-bump-needed.sh" <<'SH'
#!/usr/bin/env bash
touch version-bump-needed.ran
echo false
SH
  cat >"$repo/scripts/next-version.sh" <<'SH'
#!/usr/bin/env bash
echo 0.1.1
SH
  cat >"$repo/bump-deps.sh" <<'SH'
#!/usr/bin/env bash
touch bump-deps.ran
SH
  cat >"$repo/bin/cargo" <<'SH'
#!/usr/bin/env bash
exit 0
SH
  chmod +x "$repo"/scripts/*.sh "$repo/bump-deps.sh" "$repo/bin/cargo"

  git -C "$repo" init -q
  git -C "$repo" add -A
  git -C "$repo" -c user.email=t@example.com -c user.name=Test \
    commit -qm "chore: stub workspace"
  if [ -n "$base_ref" ]; then
    git -C "$repo" update-ref "refs/remotes/origin/${base_ref}" HEAD
  fi
}

# Run the extracted step body inside <dir> under the shell GitHub would use.
run_step() {
  local repo="$1"
  local shell_argv
  read -r -a shell_argv <"${WORK}/shell.cmd"
  (
    cd "$repo" || exit 1
    PATH="${repo}/bin:${PATH}" \
      PR_BREAKING_LABEL=false \
      VIBE_BUMP_QUARANTINE_HOURS=24 \
      "${shell_argv[@]}" "${WORK}/step.sh"
  )
}

# --- version-increment: "Bump crate version (if needed)…" -------------------

@test "version-increment step fails when detect-breaking.sh exits non-zero" {
  require_python3
  extract_step "$CI_WF" "Bump crate version" "$WORK"
  repo="${BATS_TEST_TMPDIR}/inc-fail"
  make_repo "$repo" "" 2
  run run_step "$repo"
  [ "$status" -ne 0 ]
  [ ! -e "${repo}/version-bump-needed.ran" ]
}

@test "version-increment step fails when detect-breaking.sh prints an unexpected value" {
  require_python3
  extract_step "$CI_WF" "Bump crate version" "$WORK"
  repo="${BATS_TEST_TMPDIR}/inc-junk"
  make_repo "$repo" "maybe" 0
  run run_step "$repo"
  [ "$status" -ne 0 ]
  [ ! -e "${repo}/version-bump-needed.ran" ]
}

@test "version-increment step treats a false answer as not breaking" {
  require_python3
  extract_step "$CI_WF" "Bump crate version" "$WORK"
  repo="${BATS_TEST_TMPDIR}/inc-false"
  make_repo "$repo" "false" 0
  run run_step "$repo"
  [ "$status" -eq 0 ]
  [[ "$output" == *"Breaking change signalled: false"* ]]
}

@test "version-increment step treats a true answer as breaking" {
  require_python3
  extract_step "$CI_WF" "Bump crate version" "$WORK"
  repo="${BATS_TEST_TMPDIR}/inc-true"
  make_repo "$repo" "true" 0
  run run_step "$repo"
  [ "$status" -eq 0 ]
  [[ "$output" == *"Breaking change signalled: true"* ]]
}

# --- version-gate: "Enforce breaking ⇒ major-equivalent bump" ---------------

@test "version gate step fails when detect-breaking.sh exits non-zero" {
  require_python3
  extract_step "$CI_WF" "Enforce breaking" "$WORK"
  repo="${BATS_TEST_TMPDIR}/gate-fail"
  make_repo "$repo" "" 2 Develop
  run run_step "$repo"
  [ "$status" -ne 0 ]
  [ ! -e "${repo}/check-version-bump.ran" ]
}

@test "version gate step fails when detect-breaking.sh prints an unexpected value" {
  require_python3
  extract_step "$CI_WF" "Enforce breaking" "$WORK"
  repo="${BATS_TEST_TMPDIR}/gate-junk"
  make_repo "$repo" "yes" 0 Develop
  run run_step "$repo"
  [ "$status" -ne 0 ]
  [ ! -e "${repo}/check-version-bump.ran" ]
}

@test "version gate step treats a false answer as not breaking" {
  require_python3
  extract_step "$CI_WF" "Enforce breaking" "$WORK"
  repo="${BATS_TEST_TMPDIR}/gate-false"
  make_repo "$repo" "false" 0 Develop
  run run_step "$repo"
  [ "$status" -eq 0 ]
  [[ "$output" == *"breaking=false"* ]]
  [ -e "${repo}/check-version-bump.ran" ]
}

@test "version gate step treats a true answer as breaking" {
  require_python3
  extract_step "$CI_WF" "Enforce breaking" "$WORK"
  repo="${BATS_TEST_TMPDIR}/gate-true"
  make_repo "$repo" "true" 0 Develop
  run run_step "$repo"
  [ "$status" -eq 0 ]
  [[ "$output" == *"breaking=true"* ]]
  [ -e "${repo}/check-version-bump.ran" ]
}

# --- shape guard: no caller may go back to swallowing the exit status -------

@test "no ci.yml call site captures detect-breaking.sh inside a test expression" {
  stripped="$(strip_comments "$CI_WF")"
  if printf '%s\n' "$stripped" | grep -Eq '\[[[:space:]]+"\$\(.*detect-breaking\.sh'; then
    printf 'detect-breaking.sh output captured inside [ … ], discarding its exit status:\n%s\n' \
      "$(printf '%s\n' "$stripped" | grep -n 'detect-breaking\.sh')" >&2
    return 1
  fi
}
