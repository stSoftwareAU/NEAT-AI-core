#!/usr/bin/env bats
# Tests for the CycloneDX SBOM attached to the wasm_activation bundle Release
# (Issue #125). The bundle is a *built binary artefact* downstream consumers
# pin by SHA, so it must ship a machine-readable crate inventory alongside it.
#
# These are "what" tests (AGENTS.md): they parse wasm-bundle.yml and assert on
# observable outcomes — that an SBOM is generated from the locked graph and
# published as a Release asset — not on incidental source text. The final test
# is behavioural: if cargo-cyclonedx is installed locally it generates a real
# SBOM from this repo's manifest and asserts it is valid CycloneDX JSON.
#
# The SBOM contract itself is identical to the semver release's, so it lives
# once in helpers.bash and both suites assert through it (Issue #477).

load helpers

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/wasm-bundle.yml"
}

@test "wasm-bundle workflow file exists" {
  [ -f "$WORKFLOW" ]
}

@test "wasm-bundle workflow is valid YAML" {
  require_python3
  run python3 -c "import yaml; yaml.safe_load(open('$WORKFLOW'))"
  [ "$status" -eq 0 ]
}

# One assertion per contract clause: the publish job generates a CycloneDX SBOM
# with a version-pinned --locked cargo-cyclonedx, and publishes the .cdx.json as
# an asset of the Release it cuts, before that Release is created.
@test "publish job publishes a CycloneDX SBOM built from a pinned cargo-cyclonedx" {
  require_python3
  run assert_cyclonedx_sbom_release "$WORKFLOW" publish
  echo "$output"
  [ "$status" -eq 0 ]
}

@test "cargo-cyclonedx produces valid CycloneDX JSON from this repo's manifest" {
  if ! command -v cargo-cyclonedx &>/dev/null; then
    skip "cargo-cyclonedx not installed — install: cargo install cargo-cyclonedx --locked"
  fi
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for JSON parsing"
  fi
  ( cd "$REPO_ROOT" && cargo cyclonedx --format json --manifest-path Cargo.toml >/dev/null 2>&1 )
  # cargo-cyclonedx writes <package>.cdx.json into each crate directory.
  sbom=""
  for f in "$REPO_ROOT"/neat-core/*.cdx.json; do
    [ -f "$f" ] && sbom="$f"
  done
  [ -n "$sbom" ]
  run python3 - "$sbom" <<'PY'
import json, sys
doc = json.load(open(sys.argv[1]))
assert doc.get("bomFormat") == "CycloneDX", doc.get("bomFormat")
assert doc.get("specVersion"), doc
# The locked graph must include our direct dependencies (e.g. serde).
names = {c.get("name") for c in doc.get("components", [])}
assert "serde" in names, sorted(names)
PY
  # SBOMs are Release assets, never tree artefacts — clean up what we generated.
  rm -f "$REPO_ROOT"/neat-core/*.cdx.json
  [ "$status" -eq 0 ]
}
