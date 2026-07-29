#!/usr/bin/env bats
# Tests for the SHA-256 sidecar published alongside the wasm_activation bundle
# (Issue #438). NEAT-AI's build.sh verifies the downloaded tarball against a
# release-side `wasm_activation-pkg.tar.gz.sha256`, but until now this workflow
# published only the tarball and the CycloneDX SBOM. Without a per-revision
# anchor the only SHA-256 a consumer holds for a *new* revision is the pin
# recorded for the *old* one, so every internal `neatCore.rev` bump fails
# (stSoftwareAU/NEAT-AI#3504).
#
# These are "what" tests (AGENTS.md): the wiring tests parse wasm-bundle.yml and
# assert on observable outcomes (a sidecar is generated, attested, published,
# and re-verified), and the behavioural tests execute the workflow's real step
# scripts under the same shell GitHub would use and assert on exit status and
# on-disk results — never on incidental source text.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOW="${REPO_ROOT}/.github/workflows/wasm-bundle.yml"
  SIDECAR="wasm_activation-pkg.tar.gz.sha256"
  WORK="${BATS_TEST_TMPDIR}/step"
  mkdir -p "$WORK"
}

# Write the run: body of the first step whose name contains $2 in workflow $1 to
# $WORK/step.sh, and the argv GitHub would launch it with to $WORK/shell.cmd.
extract_step() {
  python3 - "$1" "$2" "$WORK" <<'PY'
import os, sys, yaml

workflow, needle, out = sys.argv[1:4]
with open(workflow) as fh:
    data = yaml.safe_load(fh)

def declared_shell(step, job):
    for scope in (step, job.get("defaults", {}).get("run", {}),
                  (data.get("defaults") or {}).get("run", {})):
        if scope.get("shell"):
            return scope["shell"]
    return None

for job in (data.get("jobs") or {}).values():
    for step in job.get("steps") or []:
        if needle not in (step.get("name") or "") or "run" not in step:
            continue
        body = step["run"]
        assert "${{" not in body, "step body interpolates a GitHub expression"
        shell = declared_shell(step, job)
        # GitHub: no `shell:` → `bash -e {0}`; `shell: bash` → `bash
        # --noprofile --norc -eo pipefail {0}`.
        argv = {
            None: "bash -e",
            "bash": "bash --noprofile --norc -eo pipefail",
        }.get(shell)
        assert argv, f"unsupported shell for this harness: {shell!r}"
        with open(os.path.join(out, "step.sh"), "w") as fh:
            fh.write(body)
        with open(os.path.join(out, "shell.cmd"), "w") as fh:
            fh.write(argv)
        sys.exit(0)

sys.exit(f"no step named like {needle!r} with a run: block in {workflow}")
PY
}

# --- Wiring: the sidecar reaches the Release ---------------------------------

@test "publish job generates the tarball SHA-256 sidecar" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
steps = data["jobs"]["publish"]["steps"]
runs = [s.get("run", "") for s in steps]
assert any("$SIDECAR" in r and "sha256sum" in r for r in runs), runs
PY
  [ "$status" -eq 0 ]
}

@test "sidecar is published as a Release asset" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
runs = [s.get("run", "") for s in data["jobs"]["publish"]["steps"]]
attaches = any(
    "$SIDECAR" in r and ("gh release create" in r or "gh release upload" in r)
    for r in runs
)
assert attaches, runs
PY
  [ "$status" -eq 0 ]
}

@test "sidecar is generated before the Release is published" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
steps = data["jobs"]["publish"]["steps"]
gen = next(i for i, s in enumerate(steps)
           if "$SIDECAR" in s.get("run", "") and "sha256sum" in s.get("run", ""))
pub = next(i for i, s in enumerate(steps) if "gh release create" in s.get("run", ""))
assert gen < pub, (gen, pub)
PY
  [ "$status" -eq 0 ]
}

@test "provenance attestation covers the sidecar" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import yaml
data = yaml.safe_load(open("$WORKFLOW"))
steps = data["jobs"]["publish"]["steps"]
attest_idx, attest = next(
    (i, s) for i, s in enumerate(steps)
    if "actions/attest-build-provenance@" in s.get("uses", "")
)
subject = attest.get("with", {}).get("subject-path", "")
assert "$SIDECAR" in subject, subject
# Signing must happen after the sidecar exists, else it signs nothing.
gen = next(i for i, s in enumerate(steps)
           if "$SIDECAR" in s.get("run", "") and "sha256sum" in s.get("run", ""))
assert gen < attest_idx, (gen, attest_idx)
PY
  [ "$status" -eq 0 ]
}

@test "verify step re-downloads the sidecar and checks it" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import fnmatch, re, yaml
data = yaml.safe_load(open("$WORKFLOW"))
verify = next(s for s in data["jobs"]["publish"]["steps"]
              if "Verify published bundle" in (s.get("name") or ""))
body = verify["run"]
quotes = chr(39) + chr(34)
patterns = [p.strip(quotes) for p in re.findall(r"--pattern\s+(\S+)", body)]
assert any(fnmatch.fnmatch("$SIDECAR", p) for p in patterns), patterns
assert "sha256sum -c" in body, body
PY
  [ "$status" -eq 0 ]
}

# --- Behaviour: the generated sidecar is a usable anchor ---------------------

@test "generate step emits a sidecar build.sh can parse and verify" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  extract_step "$WORKFLOW" "sidecar"
  work="${BATS_TEST_TMPDIR}/gen"
  mkdir -p "$work"
  printf 'not a real bundle, but real bytes\n' >"$work/wasm_activation-pkg.tar.gz"
  ( cd "$work" && bash -e "$WORK/step.sh" )

  [ -s "$work/$SIDECAR" ]
  # Standard `shasum -a 256` format: <64-hex><two spaces><filename>.
  run grep -Eq '^[0-9a-f]{64}  wasm_activation-pkg\.tar\.gz$' "$work/$SIDECAR"
  [ "$status" -eq 0 ]
  # build.sh consumes the hash with `awk '{print $1}' | head -n1`.
  parsed="$(awk '{print $1}' <"$work/$SIDECAR" | head -n1)"
  actual="$(sha256sum "$work/wasm_activation-pkg.tar.gz" | awk '{print $1}')"
  [ "$parsed" = "$actual" ]
}

@test "generate step fails loud when the tarball is missing" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  extract_step "$WORKFLOW" "sidecar"
  work="${BATS_TEST_TMPDIR}/gen-missing"
  mkdir -p "$work"
  run bash -c "cd '$work' && bash -e '$WORK/step.sh'"
  [ "$status" -ne 0 ]
  [ ! -s "$work/$SIDECAR" ]
}

# --- Behaviour: the verify step catches a bad publish ------------------------

# Build a stub workspace: a `gh` on PATH whose `release download` copies
# $FIXTURE_DIR into --dir, and a no-op verify-wasm-bundle.sh so the test
# isolates the sidecar check from the (separately tested) bundle check.
stub_workspace() {
  work="$1"
  mkdir -p "$work/bin" "$work/scripts" "$work/fixture"
  cat >"$work/bin/gh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
dir=""
prev=""
for arg in "$@"; do
  [[ "$prev" == "--dir" ]] && dir="$arg"
  prev="$arg"
done
[[ -n "$dir" ]] || { echo "stub gh: no --dir" >&2; exit 2; }
mkdir -p "$dir"
shopt -s nullglob
found=0
for f in "$FIXTURE_DIR"/*; do
  cp "$f" "$dir/"
  found=1
done
[[ "$found" -eq 1 ]] || { echo "stub gh: release has no assets" >&2; exit 1; }
SH
  chmod +x "$work/bin/gh"
  printf '#!/usr/bin/env bash\nexit 0\n' >"$work/scripts/verify-wasm-bundle.sh"
  chmod +x "$work/scripts/verify-wasm-bundle.sh"
}

@test "verify step passes when the published sidecar matches the tarball" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  extract_step "$WORKFLOW" "Verify published bundle"
  work="${BATS_TEST_TMPDIR}/verify-ok"
  stub_workspace "$work"
  printf 'bundle bytes\n' >"$work/fixture/wasm_activation-pkg.tar.gz"
  ( cd "$work/fixture" && sha256sum wasm_activation-pkg.tar.gz >"$SIDECAR" )

  run bash -c "cd '$work' && PATH='$work/bin:$PATH' FIXTURE_DIR='$work/fixture' \
    GITHUB_SHA=deadbeef bash -e '$WORK/step.sh'"
  [ "$status" -eq 0 ]
}

@test "verify step fails when the published sidecar hash is wrong" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  extract_step "$WORKFLOW" "Verify published bundle"
  work="${BATS_TEST_TMPDIR}/verify-mismatch"
  stub_workspace "$work"
  printf 'bundle bytes\n' >"$work/fixture/wasm_activation-pkg.tar.gz"
  ( cd "$work/fixture" && sha256sum wasm_activation-pkg.tar.gz >"$SIDECAR" )
  # Simulate a swapped upload: the tarball no longer matches the sidecar.
  printf 'tampered bytes\n' >"$work/fixture/wasm_activation-pkg.tar.gz"

  run bash -c "cd '$work' && PATH='$work/bin:$PATH' FIXTURE_DIR='$work/fixture' \
    GITHUB_SHA=deadbeef bash -e '$WORK/step.sh'"
  [ "$status" -ne 0 ]
}

@test "verify step fails when the sidecar is missing from the Release" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  extract_step "$WORKFLOW" "Verify published bundle"
  work="${BATS_TEST_TMPDIR}/verify-missing"
  stub_workspace "$work"
  printf 'bundle bytes\n' >"$work/fixture/wasm_activation-pkg.tar.gz"

  run bash -c "cd '$work' && PATH='$work/bin:$PATH' FIXTURE_DIR='$work/fixture' \
    GITHUB_SHA=deadbeef bash -e '$WORK/step.sh'"
  [ "$status" -ne 0 ]
}
