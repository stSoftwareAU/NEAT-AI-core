#!/usr/bin/env bats
# Tests for Issue #99 — gitleaks must run from a pinned upstream CLI tarball
# with SHA-256 verification rather than the Node-based gitleaks-action.
#
# Why this matters: gitleaks/gitleaks-action@v2.3.9 declares
# `using: node20` in its action.yml. Node 20 is scheduled for automatic
# upgrade to Node 24 on GitHub-hosted runners on 2026-06-02 and full
# removal on 2026-09-16. Upstream has not shipped a Node 22/24 release of
# the action, so the safe migration is to install the gitleaks CLI binary
# directly (mirrors the wasm-pack pinned-install pattern from Issue #78).
# This also drops the GITLEAKS_LICENSE requirement, which the action
# imposed on organisations but the CLI does not.
#
# These are "what" tests — they assert on the YAML the runner will
# execute, not on commentary or surrounding prose.
#
# The pinned-install assertions, the comment stripper and the branch-filter glob
# model are shared with wasm_pack_pinned_install.bats via helpers.bash
# (Issue #477).

load helpers

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WF="${REPO_ROOT}/.github/workflows/gitleaks.yml"
}

@test "gitleaks.yml does not use the Node-based gitleaks-action" {
  [ -f "$WF" ]
  stripped="$(strip_comments "$WF")"
  if printf '%s\n' "$stripped" | grep -E 'gitleaks/gitleaks-action@' >/dev/null; then
    printf 'Node-based gitleaks-action still referenced in gitleaks.yml:\n%s\n' \
      "$(printf '%s\n' "$stripped" | grep -nE 'gitleaks/gitleaks-action@')" >&2
    return 1
  fi
}

# Download from the gitleaks release archive at a specific tag (either a literal
# vX.Y.Z or a v${GITLEAKS_VERSION} expansion), verified with sha256sum -c
# against pinned version + SHA-256 env vars.
@test "gitleaks.yml installs gitleaks from a version- and checksum-pinned release" {
  [ -f "$WF" ]
  run assert_pinned_cli_install "$WF" GITLEAKS \
    'github\.com/gitleaks/gitleaks/releases/download/v(\$\{?GITLEAKS_VERSION\}?|[0-9]+\.[0-9]+\.[0-9]+)/'
  echo "$output"
  [ "$status" -eq 0 ]
}

# Issue #328 — milestone sub-issue PRs target a shared milestone/<slug> branch.
# GitHub branch-filter globs treat `*` as "any chars except /", so a filter of
# ["*"] never matches milestone/<slug> and the secret scan silently skips those
# PRs. The filter must match milestone branches so the gate runs on them too.
@test "gitleaks.yml pull_request filter matches milestone branches" {
  require_python3
  # The existing default branches must still match too.
  run assert_pr_branch_filter_matches "$WF" \
    "milestone/clean-up-23-jul" Develop main
  echo "$output"
  [ "$status" -eq 0 ]
}

@test "gitleaks.yml invokes the gitleaks CLI" {
  [ -f "$WF" ]
  # Either `./gitleaks detect …` (binary extracted to CWD) or
  # `gitleaks detect …` (binary on PATH). Both are acceptable.
  run grep -E '(\./)?gitleaks[[:space:]]+(detect|dir|git)' "$WF"
  [ "$status" -eq 0 ]
}
