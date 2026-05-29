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

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WF="${REPO_ROOT}/.github/workflows/gitleaks.yml"
}

# Strip YAML comments so the assertions can't be defeated by leaving the
# original `uses: gitleaks/gitleaks-action@…` line behind as a comment.
strip_comments() {
  sed -E 's/[[:space:]]*#.*$//' "$1"
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

@test "gitleaks.yml installs gitleaks from a version-pinned release URL" {
  [ -f "$WF" ]
  # Require a download from the gitleaks release archive at a specific tag
  # (either a literal vX.Y.Z or a v${GITLEAKS_VERSION} env expansion — the
  # version itself is asserted by a sibling test).
  run grep -E 'github\.com/gitleaks/gitleaks/releases/download/v(\$\{?GITLEAKS_VERSION\}?|[0-9]+\.[0-9]+\.[0-9]+)/' "$WF"
  [ "$status" -eq 0 ]
}

@test "gitleaks.yml verifies the gitleaks tarball with sha256sum -c" {
  [ -f "$WF" ]
  run grep -E 'sha256sum[[:space:]]+-c' "$WF"
  [ "$status" -eq 0 ]
}

@test "gitleaks.yml declares a 64-hex GITLEAKS_SHA256 env var" {
  [ -f "$WF" ]
  run grep -E 'GITLEAKS_SHA256:[[:space:]]*"?[0-9a-f]{64}"?' "$WF"
  [ "$status" -eq 0 ]
}

@test "gitleaks.yml declares a semver GITLEAKS_VERSION env var" {
  [ -f "$WF" ]
  run grep -E 'GITLEAKS_VERSION:[[:space:]]*"?[0-9]+\.[0-9]+\.[0-9]+"?' "$WF"
  [ "$status" -eq 0 ]
}

@test "gitleaks.yml invokes the gitleaks CLI" {
  [ -f "$WF" ]
  # Either `./gitleaks detect …` (binary extracted to CWD) or
  # `gitleaks detect …` (binary on PATH). Both are acceptable.
  run grep -E '(\./)?gitleaks[[:space:]]+(detect|dir|git)' "$WF"
  [ "$status" -eq 0 ]
}
