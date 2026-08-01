#!/usr/bin/env bats
# Tests for the CycloneDX SBOM attached to the semver GitHub release cut by
# release.yml (Issue #197). The semver release is a published artefact pinned
# by downstream consumers via `v<major.minor.patch>`, so — like the per-commit
# wasm_activation bundle (Issue #125) — it must ship a machine-readable crate
# inventory alongside it for incident lookup.
#
# These are "what" tests (AGENTS.md): they parse release.yml and assert on
# observable outcomes — that an SBOM is generated from the locked graph and
# published as a Release asset — not on incidental source text.
#
# The SBOM contract itself is identical to the wasm_activation bundle's, so it
# lives once in helpers.bash and both suites assert through it (Issue #477).
# The checkout credential check (Issue #323) now lives in the repo-wide sweep
# in workflow_checkout_credentials.bats.

load helpers

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/release.yml"
}

@test "release workflow file exists" {
  [ -f "$WORKFLOW" ]
}

@test "release workflow is valid YAML" {
  require_python3
  run python3 -c "import yaml; yaml.safe_load(open('$WORKFLOW'))"
  [ "$status" -eq 0 ]
}

# One assertion per contract clause: the release job generates a CycloneDX SBOM
# with a version-pinned --locked cargo-cyclonedx, and publishes the .cdx.json
# as an asset of the Release it cuts, before that Release is created.
@test "release job publishes a CycloneDX SBOM built from a pinned cargo-cyclonedx" {
  require_python3
  run assert_cyclonedx_sbom_release "$WORKFLOW" release
  echo "$output"
  [ "$status" -eq 0 ]
}
