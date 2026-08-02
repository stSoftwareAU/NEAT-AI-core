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
#
# The pinned-install assertions and the comment stripper are shared with
# gitleaks_pinned_install.bats via helpers.bash (Issue #477).

load helpers

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WF="${REPO_ROOT}/.github/workflows/wasm-bundle.yml"
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

# Download from the wasm-pack release archive at a specific tag (either a
# literal vX.Y.Z or a v${WASM_PACK_VERSION} expansion), verified with
# sha256sum -c against pinned version + SHA-256 env vars.
@test "wasm-bundle.yml installs wasm-pack from a version- and checksum-pinned release" {
  [ -f "$WF" ]
  run assert_pinned_cli_install "$WF" WASM_PACK \
    'github\.com/(rustwasm|wasm-bindgen)/wasm-pack/releases/download/v(\$\{?WASM_PACK_VERSION\}?|[0-9]+\.[0-9]+\.[0-9]+)/'
  echo "$output"
  [ "$status" -eq 0 ]
}
