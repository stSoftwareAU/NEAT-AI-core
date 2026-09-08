#!/usr/bin/env bats
# Regression assertions for Issue #607 — the `wasm-bench` research harness is
# excluded from the root virtual workspace, so it resolves its own 22-package
# `Cargo.lock`. Nothing read that lockfile: `security.yml` audited only the root
# lock, `quality.sh` denied only the root manifest, and `.github/dependabot.yml`
# carried a single `directory: "/"` entry. An advisory or a banned source
# reaching a crate only the harness depends on was therefore invisible.
#
# These are "what" tests: each reads the committed pipeline artefact and asserts
# on the configuration it would actually execute, and the SECURITY.md scope
# statement is checked against that same wiring rather than against prose.
#
# Oracle rule 4 (AGENTS.md): each rule has exactly **one** pattern definition,
# exported from setup() and compiled both by the sweep over the live files and
# by the good/bad literal check, so gutting a live pattern cannot leave a
# private copy passing.

load helpers

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  SECURITY_WORKFLOW="${REPO_ROOT}/.github/workflows/security.yml"
  QUALITY="${REPO_ROOT}/quality.sh"
  CI_WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
  SECURITY_MD="${REPO_ROOT}/SECURITY.md"

  # `cargo audit --file <lockfile>` — the capture is the lockfile it reads.
  export AUDIT_LOCKFILE_RE='\bcargo[ \t]+audit\b[^\n;&|]*--file[ \t=]+([^ \t\n;&|]+)'
  # `cargo deny --manifest-path <manifest> … check` — the capture is the
  # manifest whose graph is checked.
  export DENY_MANIFEST_RE='\bcargo[ \t]+deny\b[^\n;&|]*--manifest-path[ \t=]+([^ \t\n;&|]+)[^\n;&|]*\bcheck\b'
  # A bare `cargo audit` with no `--file`: the root lockfile, by default.
  export ROOT_AUDIT_RE='\bcargo[ \t]+audit\b(?![^\n;&|]*--file)'

  WASM_BENCH_LOCK="wasm-bench/Cargo.lock"
  WASM_BENCH_MANIFEST="wasm-bench/Cargo.toml"
}

# Every lockfile `security.yml` audits: the `--file` captures, plus the root
# `Cargo.lock` when a bare `cargo audit` runs.
audited_lockfiles() {
  local stripped="${BATS_TEST_TMPDIR:-/tmp}/security.stripped"
  strip_comments "$SECURITY_WORKFLOW" >"$stripped"
  python3 - "$stripped" <<'PY'
import os
import re
import sys

text = open(sys.argv[1], encoding="utf-8").read()
found = set(re.findall(os.environ["AUDIT_LOCKFILE_RE"], text))
if re.search(os.environ["ROOT_AUDIT_RE"], text):
    found.add("Cargo.lock")
print("\n".join(sorted(found)))
PY
}

# Every manifest `quality.sh` runs `cargo deny check` against: the
# `--manifest-path` captures, plus the root manifest for a bare `cargo deny
# check`.
denied_manifests() {
  local stripped="${BATS_TEST_TMPDIR:-/tmp}/deny-source.stripped"
  strip_comments "$1" >"$stripped"
  python3 - "$stripped" <<'PY'
import os
import re
import sys

text = open(sys.argv[1], encoding="utf-8").read()
found = set(re.findall(os.environ["DENY_MANIFEST_RE"], text))
if re.search(r'\bcargo[ \t]+deny[ \t]+check\b', text):
    found.add("Cargo.toml")
print("\n".join(sorted(found)))
PY
}

# The lockfiles named in SECURITY.md's "Supply-chain audit scope" section.
scoped_lockfiles() {
  python3 - "$SECURITY_MD" <<'PY'
import re
import sys

lines = open(sys.argv[1], encoding="utf-8").read().splitlines()
start = next(
    (
        i
        for i, l in enumerate(lines)
        if re.fullmatch(r"#{2,3}\s+Supply-chain audit scope", l.strip(), re.I)
    ),
    None,
)
if start is None:
    sys.exit("SECURITY.md has no 'Supply-chain audit scope' section")
end = next(
    (i for i in range(start + 1, len(lines)) if re.match(r"#{1,2} ", lines[i])),
    len(lines),
)
section = "\n".join(lines[start:end])
# Path-shaped backticked tokens only: a backticked *command* that names a
# lockfile (`cargo audit --file wasm-bench/Cargo.lock`) is not itself a
# lockfile entry.
found = sorted(set(re.findall(r"`([A-Za-z0-9_./-]*Cargo\.lock)`", section)))
if not found:
    sys.exit("the 'Supply-chain audit scope' section names no lockfile")
print("\n".join(found))
PY
}

@test "security.yml audits the wasm-bench lockfile" {
  run audited_lockfiles
  [ "$status" -eq 0 ]
  printf '%s\n' "$output" | grep -Fxq "$WASM_BENCH_LOCK"
}

@test "security.yml still audits the root lockfile" {
  run audited_lockfiles
  [ "$status" -eq 0 ]
  printf '%s\n' "$output" | grep -Fxq "Cargo.lock"
}

@test "the audit pattern accepts a --file audit and rejects a root-only one" {
  # The same AUDIT_LOCKFILE_RE the sweep above compiles, against literals.
  run python3 - <<'PY'
import os
import re
import sys

audit = re.compile(os.environ["AUDIT_LOCKFILE_RE"])
root = re.compile(os.environ["ROOT_AUDIT_RE"])

good = "cargo audit --file wasm-bench/Cargo.lock"
m = audit.search(good)
assert m and m.group(1) == "wasm-bench/Cargo.lock", good

for bad in ("cargo audit", "cargo audit --deny warnings", "cargo tree --file x"):
    assert not audit.search(bad), bad

assert root.search("cargo audit"), "bare cargo audit must read as the root lock"
assert not root.search(good), "a --file audit is not the root lock audit"
PY
  [ "$status" -eq 0 ]
}

@test "quality.sh runs cargo deny over the wasm-bench manifest" {
  run denied_manifests "$QUALITY"
  [ "$status" -eq 0 ]
  printf '%s\n' "$output" | grep -Fxq "$WASM_BENCH_MANIFEST"
}

@test "quality.sh still runs cargo deny over the root manifest" {
  run denied_manifests "$QUALITY"
  [ "$status" -eq 0 ]
  printf '%s\n' "$output" | grep -Fxq "Cargo.toml"
}

# quality.sh is the local gate; no workflow runs it (docs_pipeline_accuracy.bats
# pins that), so the CI `quality` job has to make the same call itself or the
# wasm-bench graph is unchecked on every PR.
@test "the CI quality job runs cargo deny over both manifests" {
  run denied_manifests "$CI_WORKFLOW"
  [ "$status" -eq 0 ]
  printf '%s\n' "$output" | grep -Fxq "$WASM_BENCH_MANIFEST"
  printf '%s\n' "$output" | grep -Fxq "Cargo.toml"
}

@test "the deny pattern accepts a --manifest-path check and rejects a bare deny" {
  run python3 - <<'PY'
import os
import re

deny = re.compile(os.environ["DENY_MANIFEST_RE"])

good = "cargo deny --manifest-path wasm-bench/Cargo.toml check"
m = deny.search(good)
assert m and m.group(1) == "wasm-bench/Cargo.toml", good

for bad in (
    "cargo deny check",
    "cargo deny --manifest-path wasm-bench/Cargo.toml",  # no check subcommand
    "cargo build --manifest-path wasm-bench/Cargo.toml check",
):
    assert not deny.search(bad), bad
PY
  [ "$status" -eq 0 ]
}

@test "every lockfile SECURITY.md scopes exists and is audited by security.yml" {
  run scoped_lockfiles
  [ "$status" -eq 0 ]
  local scoped="$output"
  run audited_lockfiles
  [ "$status" -eq 0 ]
  local audited="$output"

  local lock
  while IFS= read -r lock; do
    [ -n "$lock" ] || continue
    if [ ! -f "${REPO_ROOT}/${lock}" ]; then
      printf 'SECURITY.md scopes %s, which does not exist\n' "$lock" >&2
      return 1
    fi
    if ! printf '%s\n' "$audited" | grep -Fxq "$lock"; then
      printf 'SECURITY.md scopes %s, which security.yml does not audit\n' "$lock" >&2
      return 1
    fi
  done <<<"$scoped"
}

@test "every lockfile in the tree is named in SECURITY.md's audit scope" {
  run scoped_lockfiles
  [ "$status" -eq 0 ]
  local scoped="$output"

  local lock
  while IFS= read -r lock; do
    lock="${lock#./}"
    if ! printf '%s\n' "$scoped" | grep -Fxq "$lock"; then
      printf 'lockfile %s is outside the SECURITY.md audit scope\n' "$lock" >&2
      return 1
    fi
  done < <(cd "$REPO_ROOT" && find . -name Cargo.lock -not -path './target/*' -not -path './.git/*')
}
