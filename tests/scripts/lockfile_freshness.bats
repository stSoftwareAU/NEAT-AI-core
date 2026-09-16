#!/usr/bin/env bats
# Regression assertions for Issue #695 — the CI `version-increment` job rewrites
# `[workspace.package].version` and re-locks the **root** lockfile only, so every
# other committed lockfile keeps naming the superseded `neat-core` version and a
# `--locked` read of its manifest fails. The staleness grew by one bump per
# merge.
#
# These are "what" tests: each one builds a real two-crate tree, moves the path
# dependency's version exactly as the bump does, and runs the real script over
# it — the assertions are on `cargo`'s own verdict, never on the script's text.

load helpers

setup() {
  REPO_ROOT="$(cd "${BATS_TEST_DIRNAME}/../.." && pwd)"
  SCRIPT="${REPO_ROOT}/scripts/lockfile-freshness.sh"
  QUALITY="${REPO_ROOT}/quality.sh"
  CI_WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
  # cargo is not optional in a Rust repository: a skip here would report the
  # gate's own suite green with zero coverage (Issue #631 outlawed exactly that
  # for this suite).
  if ! command -v cargo &>/dev/null; then
    echo "cargo is required — the lockfile-freshness gate cannot be tested without it" >&2
    return 1
  fi
}

# Write a crate at $1 named $2 at version $3, depending on the path crates $4…
write_crate() {
  local dir="$1" name="$2" version="$3"
  shift 3
  mkdir -p "${dir}/src"
  {
    printf '[package]\nname = "%s"\nversion = "%s"\nedition = "2021"\n\n' \
      "$name" "$version"
    # Its own workspace root, mirroring wasm-bench's empty [workspace] table.
    printf '[workspace]\n\n[dependencies]\n'
    local dep
    for dep in "$@"; do
      printf 'fixture-%s = { path = "../%s" }\n' "$dep" "$dep"
    done
  } >"${dir}/Cargo.toml"
  printf 'pub fn value() -> u8 { 1 }\n' >"${dir}/src/lib.rs"
  cargo generate-lockfile --offline --manifest-path "${dir}/Cargo.toml" >/dev/null
}

# A tree shaped like this repository: a versioned library plus a consumer that
# reaches it through a path dependency and resolves its own lockfile.
make_fixture() {
  local root="$1"
  write_crate "${root}/dep" "fixture-dep" "0.1.0"
  write_crate "${root}/app" "fixture-app" "0.1.0" "dep"
}

# Move the library's version, exactly as the version-increment bump does.
bump_dep() {
  sed -i.bak 's/^version = "0.1.0"$/version = "0.2.0"/' "$1/dep/Cargo.toml"
  rm -f "$1/dep/Cargo.toml.bak"
  cargo update --workspace --offline --manifest-path "$1/dep/Cargo.toml" >/dev/null
}

@test "the committed tree passes the freshness check" {
  run "$SCRIPT" --check --root "$REPO_ROOT"
  [ "$status" -eq 0 ] || printf '%s\n' "$output" >&2
  [ "$status" -eq 0 ]
}

@test "a bump that stales a sibling lockfile is reported, naming the lockfile" {
  local root="${BATS_TEST_TMPDIR}/tree"
  make_fixture "$root"
  run "$SCRIPT" --check --root "$root"
  [ "$status" -eq 0 ]

  bump_dep "$root"
  run "$SCRIPT" --check --root "$root"
  [ "$status" -ne 0 ]
  printf '%s\n' "$output" | grep -q 'app/Cargo.lock'
}

@test "--update re-locks the staled sibling and the check then passes" {
  local root="${BATS_TEST_TMPDIR}/tree"
  make_fixture "$root"
  bump_dep "$root"

  run "$SCRIPT" --update --root "$root"
  [ "$status" -eq 0 ] || printf '%s\n' "$output" >&2
  [ "$status" -eq 0 ]
  grep -q 'version = "0.2.0"' "${root}/app/Cargo.lock"

  run "$SCRIPT" --check --root "$root"
  [ "$status" -eq 0 ]
}

@test "a lockfile added later is swept without being wired in by hand" {
  local root="${BATS_TEST_TMPDIR}/tree"
  make_fixture "$root"
  write_crate "${root}/extra" "fixture-extra" "0.1.0" "dep"
  bump_dep "$root"

  run "$SCRIPT" --check --root "$root"
  [ "$status" -ne 0 ]
  printf '%s\n' "$output" | grep -q 'extra/Cargo.lock'
  # Every lockfile is reported, not just the first one found.
  printf '%s\n' "$output" | grep -q 'app/Cargo.lock'

  run "$SCRIPT" --update --root "$root"
  [ "$status" -eq 0 ]
  run "$SCRIPT" --check --root "$root"
  [ "$status" -eq 0 ]
}

@test "a lockfile with no manifest beside it fails loud" {
  local root="${BATS_TEST_TMPDIR}/tree"
  make_fixture "$root"
  mkdir -p "${root}/orphan"
  printf 'version = 4\n' >"${root}/orphan/Cargo.lock"

  run "$SCRIPT" --check --root "$root"
  [ "$status" -ne 0 ]
  printf '%s\n' "$output" | grep -q 'orphan/Cargo.toml'
}

@test "a tree with no lockfile at all fails loud rather than passing empty" {
  local root="${BATS_TEST_TMPDIR}/empty"
  mkdir -p "$root"
  run "$SCRIPT" --check --root "$root"
  [ "$status" -ne 0 ]
  printf '%s\n' "$output" | grep -qi 'no Cargo.lock'
}

@test "an unknown argument is refused rather than silently ignored" {
  run "$SCRIPT" --relock
  [ "$status" -ne 0 ]
  printf '%s\n' "$output" | grep -q -- '--relock'
}

# The re-lock must move local path packages only: pulling newer *remote*
# versions here would bypass the release-age quarantine bump-deps.sh enforces
# (Issue #76). Over an already-fresh tree it is therefore a no-op.
@test "--update leaves an already-fresh tree byte-identical" {
  local copy="${BATS_TEST_TMPDIR}/copy"
  mkdir -p "$copy"
  # Tracked files only: a `cp -a` of the directories would drag in whatever
  # build output a local `wasm-bench` run left behind.
  (cd "$REPO_ROOT" && git ls-files -z -- Cargo.toml Cargo.lock neat-core wasm-bench |
    tar -cf - --null -T -) | (cd "$copy" && tar -xf -)

  run "$SCRIPT" --update --root "$copy"
  [ "$status" -eq 0 ] || printf '%s\n' "$output" >&2
  [ "$status" -eq 0 ]
  run diff -u "${REPO_ROOT}/Cargo.lock" "${copy}/Cargo.lock"
  [ "$status" -eq 0 ]
  run diff -u "${REPO_ROOT}/wasm-bench/Cargo.lock" "${copy}/wasm-bench/Cargo.lock"
  [ "$status" -eq 0 ]
}

# --- wiring: the gate and the re-lock must be reachable from the pipeline ---

@test "quality.sh runs the freshness check, in check mode" {
  run strip_comments "$QUALITY"
  [ "$status" -eq 0 ]
  local invocations
  invocations="$(printf '%s\n' "$output" | grep 'lockfile-freshness\.sh' || true)"
  [ -n "$invocations" ]
  # Every invocation the local gate makes is a read-only check: a `--update`
  # here would rewrite a contributor's lockfiles behind the gate's back, and a
  # mode-less one would silently become whatever the default later is.
  local line
  while IFS= read -r line; do
    [[ "$line" == *"--check"* ]] || {
      printf 'quality.sh invocation is not in check mode: %s\n' "$line" >&2
      return 1
    }
  done <<<"$invocations"
}

# The re-lock reaches CI through this script, which puts it out of reach of the
# `cargo update` ban ci_workflow_quarantine.bats enforces on ci.yml (Issue #76).
# Every update it runs must therefore stay scoped to the local path packages —
# a bare `cargo update` here would move remote versions the release-age
# quarantine chose.
@test "the re-lock never runs an unscoped cargo update" {
  run strip_comments "$SCRIPT"
  [ "$status" -eq 0 ]
  local updates
  updates="$(printf '%s\n' "$output" | grep -E 'cargo[[:space:]]+update' || true)"
  [ -n "$updates" ]
  local line
  while IFS= read -r line; do
    [[ "$line" == *"--workspace"* ]] || {
      printf 'unscoped cargo update bypasses the quarantine: %s\n' "$line" >&2
      return 1
    }
  done <<<"$updates"
}

@test "the CI quality job runs the freshness check" {
  require_python3
  run python3 - "$CI_WORKFLOW" <<'PY'
import sys

import yaml

data = yaml.safe_load(open(sys.argv[1], encoding="utf-8"))
runs = [
    str(s["run"])
    for s in (data["jobs"]["quality"].get("steps") or [])
    if "run" in s
]
assert any("lockfile-freshness.sh" in r for r in runs), runs
assert not any("--update" in r and "lockfile-freshness.sh" in r for r in runs), runs
PY
  [ "$status" -eq 0 ] || printf '%s\n' "$output" >&2
  [ "$status" -eq 0 ]
}

@test "version-increment re-locks the siblings after the bump, before the commit" {
  require_python3
  run python3 - "$CI_WORKFLOW" <<'PY'
import sys

import yaml

data = yaml.safe_load(open(sys.argv[1], encoding="utf-8"))
steps = data["jobs"]["version-increment"].get("steps") or []
runs = [str(s.get("run") or "") for s in steps]

relock = next(
    (i for i, r in enumerate(runs) if "lockfile-freshness.sh --update" in r), None
)
assert relock is not None, "no step re-locks the sibling lockfiles"
bump = next((i for i, r in enumerate(runs) if "bump-deps.sh --" in r), None)
assert bump is not None, "no step runs bump-deps.sh"
commit = next((i for i, r in enumerate(runs) if "git commit" in r), None)
assert commit is not None, "no step commits the bump"
assert bump <= relock < commit, (
    f"re-lock at {relock} must sit after the bump at {bump} and before the "
    f"commit at {commit}"
)
PY
  [ "$status" -eq 0 ] || printf '%s\n' "$output" >&2
  [ "$status" -eq 0 ]
}
