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

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/wasm-bundle.yml"
}

@test "wasm-bundle workflow file exists" {
  [ -f "$WORKFLOW" ]
}

@test "wasm-bundle workflow is valid YAML" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 -c "import yaml; yaml.safe_load(open('$WORKFLOW'))"
  [ "$status" -eq 0 ]
}

@test "publish job has a step that generates a CycloneDX SBOM" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
steps = data["jobs"]["publish"]["steps"]
runs = [s.get("run", "") for s in steps]
# A step must actually invoke cargo-cyclonedx to build the SBOM.
assert any("cargo cyclonedx" in r for r in runs), runs
PY
  [ "$status" -eq 0 ]
}

@test "cargo-cyclonedx install is version-pinned for supply-chain hygiene" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import re, yaml
data = yaml.safe_load(open("$WORKFLOW"))
steps = data["jobs"]["publish"]["steps"]
# Locate the step(s) that install cargo-cyclonedx and assert the install is
# pinned to an explicit --version (mirrors the wasm-pack pinning, Issue #78).
install_lines = []
for s in steps:
    run = s.get("run", "")
    for line in run.splitlines():
        if "cargo install cargo-cyclonedx" in line:
            install_lines.append(line)
assert install_lines, "no cargo install cargo-cyclonedx step found"
pin_re = re.compile(r"--version[= ]\\S+")
for line in install_lines:
    assert pin_re.search(line), f"install not version-pinned: {line!r}"
    assert "--locked" in line, f"install not --locked: {line!r}"
PY
  [ "$status" -eq 0 ]
}

@test "SBOM is published as a Release asset (.cdx.json)" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
steps = data["jobs"]["publish"]["steps"]
runs = [s.get("run", "") for s in steps]
# The CycloneDX JSON asset must be handed to `gh release create` or
# `gh release upload` so it lands on the same per-commit Release.
attaches = any(
    ".cdx.json" in r and ("gh release create" in r or "gh release upload" in r)
    for r in runs
)
assert attaches, runs
PY
  [ "$status" -eq 0 ]
}

@test "SBOM is generated before the Release is published" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
steps = data["jobs"]["publish"]["steps"]
gen_idx = next(i for i, s in enumerate(steps) if "cargo cyclonedx" in s.get("run", ""))
pub_idx = next(i for i, s in enumerate(steps) if "gh release create" in s.get("run", ""))
assert gen_idx < pub_idx, (gen_idx, pub_idx)
PY
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
