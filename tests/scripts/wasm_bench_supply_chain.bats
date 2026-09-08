#!/usr/bin/env bats
# Regression assertions for Issue #607 — the `wasm-bench` research harness is
# excluded from the root virtual workspace, so it resolves its own 22-package
# `Cargo.lock`. Nothing read that lockfile: `security.yml` audited only the root
# lock, `quality.sh` denied only the root manifest, and `.github/dependabot.yml`
# carried a single `directory: "/"` entry. An advisory or a banned source
# reaching a crate only the harness depends on was therefore invisible.
#
# These are "what" tests: each one reads the commands the committed pipeline
# would actually **execute** — a workflow's `run:` bodies, not its step names —
# and the SECURITY.md scope statement is checked against that same wiring
# rather than against prose.
#
# Oracle rule 4 (AGENTS.md): each rule has exactly **one** pattern definition,
# exported from setup() and compiled both by the sweep over the live files and
# by the good/bad literal check, so gutting a live pattern cannot leave a
# private copy passing.

load helpers

setup() {
  require_python3
  if ! python3 -c 'import yaml' 2>/dev/null; then
    skip "PyYAML required for workflow parsing"
  fi

  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  SECURITY_WORKFLOW="${REPO_ROOT}/.github/workflows/security.yml"
  QUALITY="${REPO_ROOT}/quality.sh"
  CI_WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
  SECURITY_MD="${REPO_ROOT}/SECURITY.md"

  # `cargo audit --file <lockfile>` — the capture is the lockfile it reads.
  export AUDIT_LOCKFILE_RE='\bcargo[ \t]+audit\b[^\n;&|]*--file[ \t=]+([^ \t\n;&|]+)'
  # A bare `cargo audit` with no `--file`: the root lockfile, by default.
  export ROOT_AUDIT_RE='\bcargo[ \t]+audit\b(?![^\n;&|]*--file)'
  # `cargo deny --manifest-path <manifest> … check` — the capture is the
  # manifest whose graph is checked.
  export DENY_MANIFEST_RE='\bcargo[ \t]+deny\b[^\n;&|]*--manifest-path[ \t=]+([^ \t\n;&|]+)[^\n;&|]*\bcheck\b'
  # A bare `cargo deny check`: the root manifest, by default.
  export ROOT_DENY_RE='\bcargo[ \t]+deny[ \t]+check\b'

  WASM_BENCH_LOCK="wasm-bench/Cargo.lock"
  WASM_BENCH_MANIFEST="wasm-bench/Cargo.toml"
}

# The shell commands file $1 would actually execute, written to stdout: the
# `run:` bodies of a workflow, or the comment-stripped body of a shell script.
# A step *name* quoting a command is prose — `- name: Run cargo audit
# (fallback)` must never satisfy an assertion about what runs.
command_text() {
  case "$1" in
    *.yml | *.yaml)
      python3 - "$1" <<'PY'
import sys

import yaml

data = yaml.safe_load(open(sys.argv[1], encoding="utf-8"))
for job in (data.get("jobs") or {}).values():
    for step in job.get("steps") or []:
        if "run" in step:
            # `run: true` parses as a YAML boolean; str() keeps the sweep total.
            print(str(step["run"]))
PY
      ;;
    *) strip_comments "$1" ;;
  esac
}

# Every lockfile the commands in file $1 audit: the `--file` captures, plus the
# root `Cargo.lock` when a bare `cargo audit` runs.
audited_lockfiles() {
  local commands="${BATS_TEST_TMPDIR:-/tmp}/audit-commands.txt"
  command_text "$1" >"$commands"
  python3 - "$commands" <<'PY'
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

# Every manifest the commands in file $1 run `cargo deny check` against: the
# `--manifest-path` captures, plus the root manifest for a bare `cargo deny
# check`.
denied_manifests() {
  local commands="${BATS_TEST_TMPDIR:-/tmp}/deny-commands.txt"
  command_text "$1" >"$commands"
  python3 - "$commands" <<'PY'
import os
import re
import sys

text = open(sys.argv[1], encoding="utf-8").read()
found = set(re.findall(os.environ["DENY_MANIFEST_RE"], text))
if re.search(os.environ["ROOT_DENY_RE"], text):
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
# lockfile (`cargo audit --file wasm-bench/Cargo.lock`) is not itself an entry.
found = sorted(set(re.findall(r"`([A-Za-z0-9_./-]*Cargo\.lock)`", section)))
if not found:
    sys.exit("the 'Supply-chain audit scope' section names no lockfile")
print("\n".join(found))
PY
}

@test "security.yml audits the wasm-bench lockfile" {
  run audited_lockfiles "$SECURITY_WORKFLOW"
  [ "$status" -eq 0 ]
  printf '%s\n' "$output" | grep -Fxq "$WASM_BENCH_LOCK"
}

@test "security.yml still audits the root lockfile" {
  run audited_lockfiles "$SECURITY_WORKFLOW"
  [ "$status" -eq 0 ]
  printf '%s\n' "$output" | grep -Fxq "Cargo.lock"
}

@test "the audit patterns accept the live commands and reject their near misses" {
  # The same AUDIT_LOCKFILE_RE / ROOT_AUDIT_RE the sweeps above compile.
  run python3 - <<'PY'
import os
import re

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
# pins that), so the CI `quality` job has to make the same calls itself or the
# wasm-bench graph is unchecked on every PR.
@test "the CI quality job runs cargo deny over both manifests" {
  run denied_manifests "$CI_WORKFLOW"
  [ "$status" -eq 0 ]
  printf '%s\n' "$output" | grep -Fxq "$WASM_BENCH_MANIFEST"
  printf '%s\n' "$output" | grep -Fxq "Cargo.toml"
}

@test "the deny patterns accept the live commands and reject their near misses" {
  run python3 - <<'PY'
import os
import re

deny = re.compile(os.environ["DENY_MANIFEST_RE"])
root = re.compile(os.environ["ROOT_DENY_RE"])

good = "cargo deny --manifest-path wasm-bench/Cargo.toml check"
m = deny.search(good)
assert m and m.group(1) == "wasm-bench/Cargo.toml", good

for bad in (
    "cargo deny check",
    "cargo deny --manifest-path wasm-bench/Cargo.toml",  # no check subcommand
    "cargo build --manifest-path wasm-bench/Cargo.toml check",
):
    assert not deny.search(bad), bad

assert root.search("cargo deny check"), "bare cargo deny check is the root graph"
assert not root.search(good), "a --manifest-path check is not the root graph"
PY
  [ "$status" -eq 0 ]
}

# A workflow's step names are prose: quoting a command there must not satisfy
# any assertion above. This is what stops `run: cargo audit` being gutted while
# `- name: Run cargo audit (fallback)` keeps the suite green.
@test "command_text reads run: bodies and ignores step names" {
  local workflow="${BATS_TEST_TMPDIR}/prose.yml"
  cat >"$workflow" <<'YAML'
jobs:
  security:
    steps:
      - name: Run cargo audit (fallback) and cargo deny check
        run: echo nothing
YAML
  run command_text "$workflow"
  [ "$status" -eq 0 ]
  [ "$output" = "echo nothing" ]

  run audited_lockfiles "$workflow"
  [ "$status" -eq 0 ]
  [ -z "$output" ]

  run denied_manifests "$workflow"
  [ "$status" -eq 0 ]
  [ -z "$output" ]
}

@test "every lockfile SECURITY.md scopes exists and is audited by security.yml" {
  run scoped_lockfiles
  [ "$status" -eq 0 ]
  local scoped="$output"
  run audited_lockfiles "$SECURITY_WORKFLOW"
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
