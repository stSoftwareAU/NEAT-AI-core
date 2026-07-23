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

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/release.yml"
}

@test "release workflow file exists" {
  [ -f "$WORKFLOW" ]
}

@test "release workflow is valid YAML" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 -c "import yaml; yaml.safe_load(open('$WORKFLOW'))"
  [ "$status" -eq 0 ]
}

@test "release job has a step that generates a CycloneDX SBOM" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
steps = data["jobs"]["release"]["steps"]
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
steps = data["jobs"]["release"]["steps"]
# Locate the step(s) that install cargo-cyclonedx and assert the install is
# pinned to an explicit --version and --locked (mirrors wasm-pack, Issue #78).
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
steps = data["jobs"]["release"]["steps"]
runs = [s.get("run", "") for s in steps]
# The CycloneDX JSON asset must be handed to `gh release create` or
# `gh release upload` so it lands on the same semver Release.
attaches = any(
    ".cdx.json" in r and ("gh release create" in r or "gh release upload" in r)
    for r in runs
)
assert attaches, runs
PY
  [ "$status" -eq 0 ]
}

# Issue #323 — actions/checkout writes the workflow GITHUB_TOKEN into
# .git/config by default, leaving a usable credential on disk for every later
# step in the job. The release job cuts the tag and publishes the release via
# `gh` with GH_TOKEN from the environment, and its git operations
# (`git ls-remote origin`) target a public repository that needs no credential,
# so the persisted token is pure blast radius.
@test "release workflow checkout does not persist credentials on disk" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
checkouts = [
    s for s in data["jobs"]["release"]["steps"]
    if str(s.get("uses", "")).startswith("actions/checkout@")
]
assert checkouts, "no actions/checkout step found"
for step in checkouts:
    with_ = step.get("with") or {}
    assert with_.get("persist-credentials") is False, (
        f"checkout persists credentials: {step}"
    )
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
steps = data["jobs"]["release"]["steps"]
gen_idx = next(i for i, s in enumerate(steps) if "cargo cyclonedx" in s.get("run", ""))
pub_idx = next(i for i, s in enumerate(steps) if "gh release create" in s.get("run", ""))
assert gen_idx < pub_idx, (gen_idx, pub_idx)
PY
  [ "$status" -eq 0 ]
}
