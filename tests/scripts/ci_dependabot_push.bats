#!/usr/bin/env bats
# Issue #745 — GitHub withholds Actions secrets from a run Dependabot
# triggers, so the "Commit version and dependency changes" step in
# version-increment saw an empty PUSH_TOKEN and its push failed outright
# (exit 128, "Invalid username or token"), skipping every job that
# `needs: [version-increment]` and leaving a Dependabot PR unable to pass
# (PR #744).
#
# Contract under test, driven against the real step body extracted from
# ci.yml:
#   - Dependabot actor, no token, staged changes: the step exits 0, leaves a
#     warning annotation, pushes nothing, and commits nothing — the gates
#     behind this job then validate the PR head exactly as Dependabot wrote
#     it, which scripts/check-version-bump.sh already tolerates for a
#     non-breaking change.
#   - Any other actor with no token: a missing secret, not a Dependabot
#     quirk — the step fails loud with an error annotation.
#   - A token present: the step pushes regardless of actor, so the Dependabot
#     branch never silently swallows a real push.
#   - No staged changes: the step is a no-op, whatever the actor or token.

load helpers

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  CI_WF="${REPO_ROOT}/.github/workflows/ci.yml"
  WORK="${BATS_TEST_TMPDIR}/step"
  mkdir -p "$WORK"
}

# make_repo <dir> — a throwaway git repo with one commit and a tracked file,
# plus a `git` shim on PATH ahead of the real git: a `push` invocation is
# recorded to push.log instead of executed (there is no remote to push to
# here), everything else runs through the real git.
make_repo() {
  local repo="$1"
  # The git shim lives outside the repo's working tree, so it never shows up
  # as an untracked file `git add -A` would stage inside the step under test.
  mkdir -p "$repo" "$repo.bin"

  echo "original contents" >"$repo/tracked.txt"
  git -C "$repo" init -q
  git -C "$repo" config user.email t@example.com
  git -C "$repo" config user.name Test
  git -C "$repo" add -A
  git -C "$repo" -c user.email=t@example.com -c user.name=Test \
    commit -qm "chore: initial commit"

  local real_git
  real_git="$(command -v git)"
  cat >"$repo.bin/git" <<SH
#!/usr/bin/env bash
if [ "\$1" = "push" ]; then
  shift
  printf '%s\n' "\$*" >>"${repo}/push.log"
  exit 0
fi
exec "${real_git}" "\$@"
SH
  chmod +x "$repo.bin/git"
}

# stage_change <repo> — make an uncommitted change so `git add -A` has
# something to stage.
stage_change() {
  echo "modified contents" >"$1/tracked.txt"
}

# run_step <repo> <push-token> <actor> — run the extracted step body inside
# <repo> under the shell GitHub would use, with the git shim first on PATH.
run_step() {
  local repo="$1" token="$2" actor="$3"
  local shell_argv
  read -r -a shell_argv <"${WORK}/shell.cmd"
  (
    cd "$repo" || exit 1
    PATH="${repo}.bin:${PATH}" \
      HEAD_REF=feature \
      GITHUB_REPOSITORY=stSoftwareAU/NEAT-AI-core \
      PUSH_TOKEN="$token" \
      GITHUB_ACTOR="$actor" \
      "${shell_argv[@]}" "${WORK}/step.sh"
  )
}

@test "dependabot run with no push token leaves the bump unpushed and warns" {
  require_python3
  extract_step "$CI_WF" "Commit version and dependency changes" "$WORK"
  repo="${BATS_TEST_TMPDIR}/dependabot-no-token"
  make_repo "$repo"
  stage_change "$repo"
  before_head="$(git -C "$repo" rev-parse HEAD)"

  run run_step "$repo" "" "dependabot[bot]"

  [ "$status" -eq 0 ] || { echo "$output" >&2; return 1; }
  [[ "$output" == *"::warning title=Version bump not pushed::"* ]] || {
    echo "missing warning annotation:" >&2
    echo "$output" >&2
    return 1
  }
  [ ! -e "${repo}/push.log" ]
  after_head="$(git -C "$repo" rev-parse HEAD)"
  [ "$before_head" = "$after_head" ]
}

@test "non-dependabot actor with no push token fails loud" {
  require_python3
  extract_step "$CI_WF" "Commit version and dependency changes" "$WORK"
  repo="${BATS_TEST_TMPDIR}/other-actor-no-token"
  make_repo "$repo"
  stage_change "$repo"

  run run_step "$repo" "" "nleck"

  [ "$status" -ne 0 ] || { echo "expected non-zero exit, got 0: $output" >&2; return 1; }
  [[ "$output" == *"::error title=ACTIONS_PUSH missing::"* ]] || {
    echo "missing error annotation:" >&2
    echo "$output" >&2
    return 1
  }
  [ ! -e "${repo}/push.log" ]
}

@test "dependabot actor with a push token still pushes" {
  require_python3
  extract_step "$CI_WF" "Commit version and dependency changes" "$WORK"
  repo="${BATS_TEST_TMPDIR}/dependabot-with-token"
  make_repo "$repo"
  stage_change "$repo"
  before_head="$(git -C "$repo" rev-parse HEAD)"

  run run_step "$repo" "tok" "dependabot[bot]"

  [ "$status" -eq 0 ] || { echo "$output" >&2; return 1; }
  [ -e "${repo}/push.log" ] || { echo "expected a push, found none" >&2; return 1; }
  grep -q "https://x-access-token:tok@github.com/stSoftwareAU/NEAT-AI-core.git" "${repo}/push.log"
  grep -q "HEAD:refs/heads/feature" "${repo}/push.log"
  after_head="$(git -C "$repo" rev-parse HEAD)"
  [ "$before_head" != "$after_head" ]
}

@test "no staged changes is a no-op regardless of actor or token" {
  require_python3
  extract_step "$CI_WF" "Commit version and dependency changes" "$WORK"
  repo="${BATS_TEST_TMPDIR}/no-changes"
  make_repo "$repo"

  run run_step "$repo" "" "nleck"

  [ "$status" -eq 0 ] || { echo "$output" >&2; return 1; }
  [[ "$output" == *"No changes after version bump"* ]]
  [ ! -e "${repo}/push.log" ]
}
