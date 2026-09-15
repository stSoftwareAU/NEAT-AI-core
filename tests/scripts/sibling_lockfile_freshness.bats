#!/usr/bin/env bats
# Regression assertions for PR #693 — the auto-bump job rewrites
# `[workspace.package].version`, which is the version `neat-core` resolves to,
# and then re-locks the **root** lockfile only (`bump-deps.sh`). Every lockfile
# outside the root workspace that records `neat-core` through a path dependency
# — today `wasm-bench/Cargo.lock` — kept the pre-bump version, and every later
# `--locked` read of it died with "cannot update the lock file … because
# --locked was passed". That is what turned the `Scripts & spelling` job red on
# PR #693.
#
# These are "what" tests: the first reads the graph cargo actually resolves
# from the committed lockfiles, and the second reads the commands the committed
# workflow would actually execute — `run:` bodies, never step names.
#
# Oracle rule 4 (AGENTS.md): the re-lock pattern has exactly **one**
# definition, exported from setup() and compiled both by the sweep over the live
# workflow and by the good/bad literal check, so gutting the live command cannot
# leave a private copy passing.

load helpers

setup() {
  require_python3
  REPO_ROOT="$(cd "${BATS_TEST_DIRNAME}/../.." && pwd)"
  CI_WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"

  # `cargo update … --workspace` — re-locks the local path packages and
  # preserves remote versions, so the release-age quarantine stands. A bare
  # `cargo update` (no `--workspace`) would re-resolve the whole graph and is
  # deliberately not matched.
  export RELOCK_RE='\bcargo[ \t]+update\b[^\n;&|]*--workspace\b'
}

# Every lockfile in the tree bar the root one: the siblings the bump job must
# re-lock. Discovered from the tree rather than hard-coded, so a workspace added
# later is swept without being wired in by hand.
sibling_lockfiles() {
  (cd "$REPO_ROOT" && find . -name Cargo.lock -not -path './target/*' -not -path './.git/*' -not -path './Cargo.lock' | sed 's|^\./||')
}

# The shell commands the version-increment job would actually execute. A step
# *name* quoting a command is prose and must never satisfy an assertion.
version_increment_commands() {
  python3 - "$CI_WORKFLOW" <<'PY'
import sys

import yaml

data = yaml.safe_load(open(sys.argv[1], encoding="utf-8"))
job = (data.get("jobs") or {}).get("version-increment")
if job is None:
    sys.exit("ci.yml has no version-increment job")
for step in job.get("steps") or []:
    if step.get("run"):
        print(step["run"])
PY
}

# Every committed lockfile agrees with the manifest beside it. This is the
# symptom itself: before the fix, `wasm-bench/Cargo.lock` still named
# neat-core 0.20.1 while the manifest resolved 0.21.0, and this failed.
@test "every committed lockfile resolves against its manifest with --locked" {
  if ! command -v cargo >/dev/null 2>&1; then
    skip "cargo not installed"
  fi
  local failed=0 lock manifest
  while IFS= read -r lock; do
    manifest="$(dirname "$lock")/Cargo.toml"
    if ! cargo metadata --format-version 1 --locked \
      --manifest-path "${REPO_ROOT}/${manifest}" >/dev/null 2>&1; then
      printf '%s is stale against %s — re-lock it with `cargo update --manifest-path %s --workspace`\n' \
        "$lock" "$manifest" "$manifest" >&2
      failed=1
    fi
  done < <(cd "$REPO_ROOT" && find . -name Cargo.lock -not -path './target/*' -not -path './.git/*' | sed 's|^\./||')
  [ "$failed" -eq 0 ]
}

# Keeping the committed lockfiles fresh by hand lasts exactly until the next
# auto-bump. The job that causes the skew must repair it in the same run.
@test "the version-increment job re-locks the sibling lockfiles it staled" {
  local commands
  commands="$(version_increment_commands)"
  [ -n "$commands" ]
  run python3 -c '
import os
import re
import sys

commands = sys.stdin.read()
assert re.search(os.environ["RELOCK_RE"], commands), "version-increment never runs cargo update --workspace"
' <<<"$commands"
  echo "$output"
  [ "$status" -eq 0 ]
}

# The sweep above is worth nothing if there is no sibling lockfile to re-lock:
# it would pass on a tree where the skew cannot happen. This pins that the
# repository really does carry one.
@test "the tree carries at least one sibling lockfile for that sweep to reach" {
  run sibling_lockfiles
  [ "$status" -eq 0 ]
  [ -n "$output" ]
  printf '%s\n' "$output" | grep -Fxq "wasm-bench/Cargo.lock"
}

# Oracle rule 4: the one live pattern, compiled against literals it must accept
# and near misses it must reject.
@test "the re-lock pattern accepts the live command and rejects its near misses" {
  run python3 -c '
import os
import re
import sys

pattern = re.compile(os.environ["RELOCK_RE"])
good = [
    "cargo update --manifest-path \"$(dirname \"$lock\")/Cargo.toml\" --workspace",
    "cargo update --workspace --manifest-path wasm-bench/Cargo.toml",
]
bad = [
    "cargo update",                      # re-resolves everything, quarantine bypassed
    "cargo update --manifest-path wasm-bench/Cargo.toml",
    "cargo metadata --workspace",        # not a re-lock at all
]
for text in good:
    assert pattern.search(text), f"pattern rejected a good command: {text}"
for text in bad:
    assert not pattern.search(text), f"pattern accepted a bad command: {text}"
'
  echo "$output"
  [ "$status" -eq 0 ]
}
