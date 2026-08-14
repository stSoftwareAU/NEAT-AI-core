#!/usr/bin/env bats
# Tests for the wasm64 (Memory64) dual-ship lane of wasm-bundle.yml — Issue #541.
#
# The published Release must carry a genuine Memory64 artefact NEAT-AI can pin,
# alongside the wasm32 asset that remains the rollback window. These are "what"
# tests: they assert on the YAML the runner will execute, and they execute the
# real gate scripts where a shell step is what is under test — never on prose.
#
# The pinned-install assertion is shared with wasm_pack_pinned_install.bats via
# helpers.bash (Issue #477).

load helpers

setup() {
  require_python3
  REPO_ROOT="$(cd "${BATS_TEST_DIRNAME}/../.." && pwd)"
  WF="${REPO_ROOT}/.github/workflows/wasm-bundle.yml"
  [ -f "$WF" ]
  export REPO_ROOT WF
}

# Every `run:` body in the publish job, newline-joined.
publish_runs() {
  python3 - "$WF" <<'PY'
import sys

import yaml

data = yaml.safe_load(open(sys.argv[1]))
steps = data["jobs"]["publish"]["steps"]
print("\n".join(s.get("run", "") for s in steps))
PY
}

@test "the publish job builds a wasm64 bundle" {
  run publish_runs
  [ "$status" -eq 0 ]
  [[ "$output" == *"--arch wasm64"* ]]
}

@test "the publish job still builds the wasm32 rollback bundle" {
  run publish_runs
  [ "$status" -eq 0 ]
  [[ "$output" == *"--arch wasm32"* ]]
}

@test "both bundles are published as distinctly named Release assets" {
  run publish_runs
  [ "$status" -eq 0 ]
  [[ "$output" == *"wasm_activation-pkg.tar.gz"* ]]
  [[ "$output" == *"wasm_activation-wasm64-pkg.tar.gz"* ]]
}

@test "the wasm64 asset carries its own SHA-256 sidecar" {
  run publish_runs
  [ "$status" -eq 0 ]
  [[ "$output" == *"wasm_activation-wasm64-pkg.tar.gz.sha256"* ]]
}

@test "numeric parity between the two arches gates the publish" {
  run publish_runs
  [ "$status" -eq 0 ]
  [[ "$output" == *"check_wasm_arch_parity.ts"* ]]
}

@test "the parity check runs before the Release is created" {
  run python3 - "$WF" <<'PY'
import sys

import yaml

steps = yaml.safe_load(open(sys.argv[1]))["jobs"]["publish"]["steps"]
runs = [s.get("run", "") for s in steps]
parity = next(i for i, r in enumerate(runs) if "check_wasm_arch_parity.ts" in r)
release = next(i for i, r in enumerate(runs) if "gh release create" in r)
assert parity < release, f"parity runs after publish: {parity} > {release}"
PY
  echo "$output"
  [ "$status" -eq 0 ]
}

@test "the wasm64 build installs a nightly toolchain with rust-src" {
  run python3 - "$WF" <<'PY'
import sys

import yaml

steps = yaml.safe_load(open(sys.argv[1]))["jobs"]["publish"]["steps"]
blob = yaml.safe_dump(steps)
# wasm64-unknown-unknown is Tier 3: no prebuilt std, so -Z build-std needs the
# rust-src component on a nightly toolchain.
assert "rust-src" in blob, "no rust-src component requested"
assert "nightly" in blob, "no nightly toolchain requested"
PY
  echo "$output"
  [ "$status" -eq 0 ]
}

@test "wasm-bindgen CLI is installed from a version- and checksum-pinned release" {
  run assert_pinned_cli_install "$WF" WASM_BINDGEN \
    'github\.com/(rustwasm|wasm-bindgen)/wasm-bindgen/releases/download/(\$\{?WASM_BINDGEN_VERSION\}?|[0-9]+\.[0-9]+\.[0-9]+)/'
  echo "$output"
  [ "$status" -eq 0 ]
}

@test "the pinned wasm-bindgen CLI version is checked against Cargo.lock" {
  # A CLI/crate skew is what produced the July 2026 NO-GO. The workflow must
  # compare the two and fail, not proceed on a hopeful match.
  run publish_runs
  [ "$status" -eq 0 ]
  [[ "$output" == *"Cargo.lock"* ]]
  [[ "$output" == *"WASM_BINDGEN_VERSION"* ]]
}

@test "the CLI/Cargo.lock version check fails loud on a skew" {
  tmp="$(mktemp -d)"
  extract_step "$WF" "Install wasm-bindgen" "$tmp"
  # Replay only the version-agreement guard from the real step, against a
  # deliberately skewed Cargo.lock.
  cat >"$tmp/Cargo.lock" <<'LOCK'
[[package]]
name = "wasm-bindgen"
version = "0.2.108"
LOCK
  guard="$(grep -n 'locked=' -A 8 "$tmp/step.sh" | sed 's/^[0-9]*[-:]//')"
  [ -n "$guard" ]
  cd "$tmp"
  WASM_BINDGEN_VERSION="0.2.127" run bash -euo pipefail -c "$guard"
  [ "$status" -ne 0 ]
  [[ "$output" == *"0.2.108"* ]]
  rm -rf "$tmp"
}

@test "the pinned wasm-bindgen CLI version already agrees with Cargo.lock" {
  # The workflow guard fails the publish on a skew; this catches the same skew
  # one step earlier, on the PR that bumps the crate, so Develop never breaks.
  run python3 - "$WF" "${REPO_ROOT}/Cargo.lock" <<'PY'
import re
import sys

import yaml

workflow, lockfile = sys.argv[1], sys.argv[2]
steps = yaml.safe_load(open(workflow))["jobs"]["publish"]["steps"]
pinned = next(
    s["env"]["WASM_BINDGEN_VERSION"]
    for s in steps
    if "WASM_BINDGEN_VERSION" in (s.get("env") or {})
)
lock = open(lockfile, encoding="utf-8").read()
match = re.search(r'name = "wasm-bindgen"\nversion = "([^"]+)"', lock)
assert match, "Cargo.lock has no wasm-bindgen entry"
assert pinned == match.group(1), (
    f"workflow pins wasm-bindgen CLI {pinned} but Cargo.lock resolves the "
    f"crate to {match.group(1)} — bump WASM_BINDGEN_VERSION and "
    f"WASM_BINDGEN_SHA256 together with the crate"
)
PY
  echo "$output"
  [ "$status" -eq 0 ]
}

@test "the CycloneDX SBOM is resolved against the arch it describes" {
  run publish_runs
  [ "$status" -eq 0 ]
  [[ "$output" == *"--target wasm32-unknown-unknown"* ]]
  [[ "$output" == *"--target wasm64-unknown-unknown"* ]]
}

@test "provenance is attested for the wasm64 assets too" {
  run python3 - "$WF" <<'PY'
import sys

import yaml

steps = yaml.safe_load(open(sys.argv[1]))["jobs"]["publish"]["steps"]
attest = [
    s for s in steps
    if str(s.get("uses", "")).startswith("actions/attest-build-provenance@")
]
assert attest, "no attest-build-provenance step"
subjects = "\n".join(s.get("with", {}).get("subject-path", "") for s in attest)
for asset in (
    "wasm_activation-pkg.tar.gz",
    "wasm_activation-wasm64-pkg.tar.gz",
    "wasm_activation-wasm64-pkg.tar.gz.sha256",
):
    assert asset in subjects, f"{asset} is not attested: {subjects!r}"
PY
  echo "$output"
  [ "$status" -eq 0 ]
}

@test "the published wasm64 asset is re-verified after upload" {
  run python3 - "$WF" <<'PY'
import sys

import yaml

steps = yaml.safe_load(open(sys.argv[1]))["jobs"]["publish"]["steps"]
runs = [s.get("run", "") for s in steps]
release = next(i for i, r in enumerate(runs) if "gh release create" in r)
after = "\n".join(runs[release + 1:])
assert "wasm_activation-wasm64-pkg.tar.gz" in after, (
    "the wasm64 asset is never re-downloaded and verified after publish"
)
assert "verify-wasm-bundle.sh" in after, "verify-wasm-bundle.sh is not re-run"
PY
  echo "$output"
  [ "$status" -eq 0 ]
}
