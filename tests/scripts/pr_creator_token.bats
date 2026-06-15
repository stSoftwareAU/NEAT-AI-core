#!/usr/bin/env bats
# Regression assertion for Issue #168 — the peter-evans/create-pull-request
# step in upgrade-dependencies.yml must authenticate with the org-level PAT
# (ACTIONS_PUSH) and only fall back to GITHUB_TOKEN when the secret is unset
# (Issue #1636). Using GITHUB_TOKEN directly makes GitHub suppress downstream
# workflow triggers on the created PR, so CI / labels / reviewer automation
# never fire until somebody pushes a new commit.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  UPGRADE_WF="${REPO_ROOT}/.github/workflows/upgrade-dependencies.yml"
}

@test "upgrade-dependencies.yml uses create-pull-request action" {
  [ -f "$UPGRADE_WF" ]
  run grep -q 'peter-evans/create-pull-request@' "$UPGRADE_WF"
  [ "$status" -eq 0 ]
}

@test "create-pull-request authenticates with ACTIONS_PUSH and GITHUB_TOKEN fallback" {
  [ -f "$UPGRADE_WF" ]
  # The create-pull-request step's token must prefer ACTIONS_PUSH and fall back
  # to GITHUB_TOKEN only when the PAT secret is unset.
  run grep -Eq 'token:[[:space:]]*\$\{\{[[:space:]]*secrets\.ACTIONS_PUSH[[:space:]]*\|\|[[:space:]]*secrets\.GITHUB_TOKEN[[:space:]]*\}\}' "$UPGRADE_WF"
  [ "$status" -eq 0 ]
}

@test "create-pull-request step does not authenticate with GITHUB_TOKEN alone" {
  [ -f "$UPGRADE_WF" ]
  # Find the create-pull-request step and inspect its `token:` line. A bare
  # GITHUB_TOKEN (no ACTIONS_PUSH fallback) re-opens the suppressed-trigger bug.
  step_block="$(awk '
    /peter-evans\/create-pull-request@/ { capture = 1 }
    capture && /token:/ { print; capture = 0 }
  ' "$UPGRADE_WF")"
  [ -n "$step_block" ]
  if printf '%s\n' "$step_block" | grep -Eq 'token:[[:space:]]*\$\{\{[[:space:]]*secrets\.GITHUB_TOKEN[[:space:]]*\}\}'; then
    printf 'create-pull-request authenticates with bare GITHUB_TOKEN:\n%s\n' "$step_block" >&2
    return 1
  fi
}
