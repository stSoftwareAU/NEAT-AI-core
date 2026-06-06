#!/usr/bin/env bats
# Tests for the build-provenance attestation attached to the wasm_activation
# bundle Release (Issue #122). Downstream consumers pin the bundle by commit
# SHA and re-verify its *contents/size* post-publish, but neither proves
# *origin*: anyone with `contents:write` could swap the Release asset for a
# malicious tarball of the right shape. A Sigstore-backed keyless provenance
# attestation lets a consumer assert "this artefact was built by this
# workflow at commit X" before trusting it, closing the published-artefact
# substitution attack class.
#
# These are "what" tests (AGENTS.md): they parse wasm-bundle.yml and assert on
# observable outcomes — the workflow grants the attestation token scopes, runs
# the attest action over the published bundle, and pins the action by SHA —
# not on incidental source text.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/wasm-bundle.yml"
}

@test "wasm-bundle workflow file exists" {
  [ -f "$WORKFLOW" ]
}

@test "wasm-bundle workflow grants the token scopes attestation requires" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
# attest-build-provenance needs an OIDC token (id-token: write) to fetch a
# Sigstore signing identity and attestations: write to record the bundle.
# Permissions may be set at the top level or on the publish job.
top = data.get("permissions", {}) or {}
job = data["jobs"]["publish"].get("permissions", {}) or {}
perms = {**top, **job}
assert perms.get("id-token") == "write", perms
assert perms.get("attestations") == "write", perms
# The pre-existing contents:write for `gh release create` must be retained.
assert perms.get("contents") == "write", perms
PY
  [ "$status" -eq 0 ]
}

@test "publish job has a step that attests build provenance" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
steps = data["jobs"]["publish"]["steps"]
uses = [s.get("uses", "") for s in steps]
assert any("actions/attest-build-provenance@" in u for u in uses), uses
PY
  [ "$status" -eq 0 ]
}

@test "provenance attestation covers the published bundle tarball" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
steps = data["jobs"]["publish"]["steps"]
attest = next(s for s in steps if "actions/attest-build-provenance@" in s.get("uses", ""))
subject = attest.get("with", {}).get("subject-path", "")
assert "wasm_activation-pkg.tar.gz" in subject, subject
PY
  [ "$status" -eq 0 ]
}

@test "attest-build-provenance action is SHA-pinned for supply-chain hygiene" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import re, yaml
data = yaml.safe_load(open("$WORKFLOW"))
steps = data["jobs"]["publish"]["steps"]
attest = next(s for s in steps if "actions/attest-build-provenance@" in s.get("uses", ""))
ref = attest["uses"]
# Mirrors the SHA-pinning rule (Issue #77): a 40-char hex commit SHA, never
# a floating tag like @v4.
assert re.match(r"^actions/attest-build-provenance@[0-9a-f]{40}$", ref), ref
PY
  [ "$status" -eq 0 ]
}

@test "provenance is attested only after the bundle is built" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
steps = data["jobs"]["publish"]["steps"]
build_idx = next(i for i, s in enumerate(steps) if "build-wasm-bundle.sh" in s.get("run", ""))
attest_idx = next(i for i, s in enumerate(steps) if "actions/attest-build-provenance@" in s.get("uses", ""))
# Attesting before the artefact exists would sign nothing meaningful.
assert build_idx < attest_idx, (build_idx, attest_idx)
PY
  [ "$status" -eq 0 ]
}
