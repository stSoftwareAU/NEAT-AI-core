#!/usr/bin/env bats
# Tests for SEC-613f26c3c4e3 (Issue #78) — wasm-pack must be installed
# from a pinned tarball with a checksum verification rather than the
# classic `curl … | sh` bootstrap.
#
# Why this matters: `curl … | sh` ships whatever bytes the upstream
# server returns straight into bash with `contents: write` permissions
# on the runner. A compromise of `rustwasm.github.io`, the wasm-pack
# release pipeline, or the network path silently propagates into every
# per-commit `wasm_activation` bundle our downstream consumers pin
# against. Pinning the version + SHA-256 kills that re-fetch window.
#
# These are "what" tests — they assert on the YAML the runner will
# execute, not on commentary or surrounding prose.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WF="${REPO_ROOT}/.github/workflows/wasm-bundle.yml"
}

# Strip YAML comments so the assertions can't be defeated by leaving
# the original `curl … | sh` line behind as a comment.
strip_comments() {
  sed -E 's/[[:space:]]*#.*$//' "$1"
}

@test "wasm-bundle.yml does not pipe a remote installer into sh" {
  [ -f "$WF" ]
  stripped="$(strip_comments "$WF")"
  if printf '%s\n' "$stripped" | grep -E 'curl[^|]*\|[[:space:]]*sh([[:space:]]|$)' >/dev/null; then
    printf 'Unsafe curl | sh found in wasm-bundle.yml:\n%s\n' \
      "$(printf '%s\n' "$stripped" | grep -nE 'curl.*\|[[:space:]]*sh')" >&2
    return 1
  fi
}

@test "wasm-bundle.yml installs wasm-pack from a version-pinned release URL" {
  [ -f "$WF" ]
  # Require a download from the rustwasm/wasm-pack release archive at a
  # specific tag (either a literal vX.Y.Z or a v${WASM_PACK_VERSION} env
  # expansion — the version itself is asserted by a sibling test).
  run grep -E 'github\.com/(rustwasm|wasm-bindgen)/wasm-pack/releases/download/v(\$\{?WASM_PACK_VERSION\}?|[0-9]+\.[0-9]+\.[0-9]+)/' "$WF"
  [ "$status" -eq 0 ]
}

@test "wasm-bundle.yml verifies the wasm-pack tarball with sha256sum -c" {
  [ -f "$WF" ]
  run grep -E 'sha256sum[[:space:]]+-c' "$WF"
  [ "$status" -eq 0 ]
}

@test "wasm-bundle.yml declares a 64-hex WASM_PACK_SHA256 env var" {
  [ -f "$WF" ]
  run grep -E 'WASM_PACK_SHA256:[[:space:]]*"?[0-9a-f]{64}"?' "$WF"
  [ "$status" -eq 0 ]
}

@test "wasm-bundle.yml declares a semver WASM_PACK_VERSION env var" {
  [ -f "$WF" ]
  run grep -E 'WASM_PACK_VERSION:[[:space:]]*"?[0-9]+\.[0-9]+\.[0-9]+"?' "$WF"
  [ "$status" -eq 0 ]
}
