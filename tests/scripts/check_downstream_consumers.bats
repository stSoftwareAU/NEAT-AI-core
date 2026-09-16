#!/usr/bin/env bats
# Tests for scripts/check-downstream-consumers.sh (Issue #644).
#
# "What" tests: a stub `cargo` records the consumer it ran in, its arguments
# and RUSTFLAGS, and fails on demand; the script is driven in --workspace mode
# against throwaway sibling directories, so no network, toolchain or real
# consumer is needed. The last two tests read the committed registry and the
# CI wiring — the gate only protects consumers it knows about and only if it
# runs.

load helpers

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  SCRIPT="${REPO_ROOT}/scripts/check-downstream-consumers.sh"
  WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
  RULESET="${REPO_ROOT}/.github/rulesets/develop.json"
  WORK="$(mktemp -d)"
  CORE="$WORK/NEAT-AI-core"
  mkdir -p "$CORE/neat-core"
  printf '[package]\nname = "neat-core"\n' >"$CORE/neat-core/Cargo.toml"
  BIN="$WORK/bin"
  mkdir -p "$BIN"
  CARGO_LOG="$WORK/cargo.log"
  cat >"$BIN/cargo" <<'STUB'
#!/bin/bash
# consumer|arguments|RUSTFLAGS — and a compile error for consumers named in
# FAKE_CARGO_FAIL (space-separated).
printf '%s|%s|%s\n' "$(basename "$PWD")" "$*" "${RUSTFLAGS-<unset>}" >>"$CARGO_LOG"
case " ${FAKE_CARGO_FAIL:-} " in
  *" $(basename "$PWD") "*)
    echo 'error[E0616]: field `neurons` of struct `CompiledNetwork` is private' >&2
    exit 101
    ;;
esac
exit 0
STUB
  chmod +x "$BIN/cargo"
  REGISTRY="$WORK/registry.txt"
  printf '# test registry\nstSoftwareAU/Alpha\nstSoftwareAU/Beta\n' >"$REGISTRY"
  for consumer in Alpha Beta; do
    mkdir -p "$WORK/$consumer"
    printf '[workspace]\nmembers = []\n' >"$WORK/$consumer/Cargo.toml"
  done
}

teardown() {
  rm -rf "$WORK"
}

# run_gate [VAR=value ...] — run the script in --workspace mode with the stub
# cargo first on PATH and any extra environment the test needs.
run_gate() {
  run env PATH="$BIN:$PATH" CARGO_LOG="$CARGO_LOG" "$@" \
    "$SCRIPT" --registry "$REGISTRY" --core "$CORE" --workspace "$WORK"
}

@test "every registered consumer is compiled in its own checkout and the gate passes" {
  run_gate
  [ "$status" -eq 0 ]
  [[ "$output" == *"✅ stSoftwareAU/Alpha compiles"* ]]
  [[ "$output" == *"✅ stSoftwareAU/Beta compiles"* ]]
  [[ "$output" == *"all 2 registered consumers compile"* ]]
  [ "$(wc -l <"$CARGO_LOG" | tr -d ' ')" -eq 2 ]
  grep -q '^Alpha|check --workspace --all-targets|' "$CARGO_LOG"
  grep -q '^Beta|check --workspace --all-targets|' "$CARGO_LOG"
}

@test "a consumer that stops compiling fails the gate by name and cites the three-phase flow" {
  run_gate FAKE_CARGO_FAIL=Beta
  [ "$status" -eq 1 ]
  [[ "$output" == *"❌ stSoftwareAU/Beta no longer compiles"* ]]
  [[ "$output" == *"E0616"* ]]
  [[ "$output" == *"1 of 2 registered consumers broke"* ]]
  [[ "$output" == *"three-phase flow"* ]]
  [[ "$output" == *"✅ stSoftwareAU/Alpha compiles"* ]]
}

@test "every consumer is checked even after the first one fails" {
  run_gate FAKE_CARGO_FAIL="Alpha Beta"
  [ "$status" -eq 1 ]
  [[ "$output" == *"❌ stSoftwareAU/Alpha no longer compiles"* ]]
  [[ "$output" == *"❌ stSoftwareAU/Beta no longer compiles"* ]]
  [[ "$output" == *"2 of 2 registered consumers broke"* ]]
  [ "$(wc -l <"$CARGO_LOG" | tr -d ' ')" -eq 2 ]
}

@test "RUSTFLAGS from the environment never reach a consumer build" {
  # -D warnings would turn a phase-1 #[deprecated], or a warning the consumer
  # already carries, into a failed gate; only "does not compile" may fail it.
  run_gate RUSTFLAGS="-D warnings"
  [ "$status" -eq 0 ]
  grep -q '|<unset>$' "$CARGO_LOG"
  ! grep -q -- '-D warnings' "$CARGO_LOG"
}

@test "a registered consumer missing from the workspace fails the gate by name" {
  rm -rf "$WORK/Beta"
  run_gate
  [ "$status" -eq 1 ]
  [[ "$output" == *"❌ stSoftwareAU/Beta: no checkout"* ]]
  [[ "$output" == *"missing from workspace"* ]]
  # The consumer that is present is still compiled.
  grep -q '^Alpha|' "$CARGO_LOG"
}

@test "a malformed registry line is a usage error" {
  printf 'stSoftwareAU/Alpha\nnot-a-repo\n' >"$REGISTRY"
  run_gate
  [ "$status" -eq 2 ]
  [[ "$output" == *"expected owner/name, got 'not-a-repo'"* ]]
  [ ! -f "$CARGO_LOG" ]
}

@test "an empty registry is a usage error, not a green gate" {
  printf '# nothing registered\n\n' >"$REGISTRY"
  run_gate
  [ "$status" -eq 2 ]
  [[ "$output" == *"lists no consumers"* ]]
}

@test "a duplicate registry entry is a usage error" {
  printf 'stSoftwareAU/Alpha\nstSoftwareAU/Alpha\n' >"$REGISTRY"
  run_gate
  [ "$status" -eq 2 ]
  [[ "$output" == *"duplicate entry"* ]]
}

@test "the workspace's NEAT-AI-core must be the core under test" {
  local other="$WORK/elsewhere/NEAT-AI-core"
  mkdir -p "$other/neat-core"
  printf '[package]\nname = "neat-core"\n' >"$other/neat-core/Cargo.toml"
  run env PATH="$BIN:$PATH" CARGO_LOG="$CARGO_LOG" \
    "$SCRIPT" --registry "$REGISTRY" --core "$other" --workspace "$WORK"
  [ "$status" -eq 2 ]
  [[ "$output" == *"must be the core under test"* ]]
  [ ! -f "$CARGO_LOG" ]
}

@test "--list validates the committed registry, which names every path-dependency consumer" {
  run "$SCRIPT" --list
  [ "$status" -eq 0 ]
  local line
  while IFS= read -r line; do
    [[ "$line" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]]
  done <<<"$output"
  # The consumer whose break takes the production fleet down.
  [[ "$output" == *"stSoftwareAU/NEAT-AI-scorer"* ]]
}

@test "ci.yml runs the downstream gate unconditionally on every pull request, read-only" {
  require_python3
  run python3 - "$WORKFLOW" <<'PY'
import sys

import yaml

data = yaml.safe_load(open(sys.argv[1]))
job = (data.get("jobs") or {}).get("downstream-consumers")
assert job is not None, "no downstream-consumers job"
assert "if" not in job, f"the gate must be unconditional, got if: {job['if']!r}"
runs = [s.get("run", "") for s in job.get("steps") or []]
assert any("check-downstream-consumers.sh" in r for r in runs), runs
print(job["name"])
PY
  [ "$status" -eq 0 ]
  run assert_job_least_privilege "$WORKFLOW" downstream-consumers
  [ "$status" -eq 0 ]
}

@test "the Develop ruleset requires the downstream gate" {
  require_python3
  run python3 - "$WORKFLOW" "$RULESET" <<'PY'
import json
import sys

import yaml

name = yaml.safe_load(open(sys.argv[1]))["jobs"]["downstream-consumers"]["name"]
ruleset = json.load(open(sys.argv[2]))
contexts = {
    c["context"]
    for rule in ruleset["rules"]
    if rule["type"] == "required_status_checks"
    for c in rule["parameters"]["required_status_checks"]
}
assert name in contexts, f"{name!r} is not a required check; required: {sorted(contexts)}"
PY
  [ "$status" -eq 0 ]
}

# --- [patch] injection for git-tag-pinned consumers (Issue #681) -------------
#
# Once a consumer pins `neat-core = { git = …, tag = "v…" }` instead of taking
# the path dependency, the gate must still compile it against the *candidate*
# core. These tests use real git remotes on disk — reached through a
# `url.<base>.insteadOf` rewrite in a throwaway global git config — so the
# clone, the fetch and (in the first test) the compile are the real thing and
# nothing leaves the machine.

# A bare repository at $1 whose default branch is $2, built from the working
# tree at $3 with the tags named after it.
make_bare_repo() {
  local bare="$1" branch="$2" src="$3" tag
  shift 3
  git -C "$src" init -q
  git -C "$src" symbolic-ref HEAD "refs/heads/$branch"
  git -C "$src" add -A
  git -C "$src" -c user.email=fixture@example.com -c user.name=fixture \
    commit -q -m fixture
  for tag in "$@"; do
    git -C "$src" tag "$tag"
  done
  git clone -q --bare "$src" "$bare"
}

# Rewrite every family URL onto $WORK/remotes, for git and for cargo alike.
use_local_family_remotes() {
  mkdir -p "$WORK/remotes"
  export GIT_CONFIG_NOSYSTEM=1
  export GIT_CONFIG_GLOBAL="$WORK/gitconfig"
  cat >"$GIT_CONFIG_GLOBAL" <<EOF
[url "file://$WORK/remotes/"]
	insteadOf = https://github.com/stSoftwareAU/
EOF
  # libgit2 does not honour insteadOf; cargo's git CLI transport does.
  export CARGO_NET_GIT_FETCH_WITH_CLI=true
  export CARGO_HOME="$WORK/cargo-home"
}

# A candidate core at $WORK/candidate carrying `candidate_only()`, a symbol no
# release has — so "the consumer compiled" is proof the candidate was used.
# $1 is its version (default 0.15.9).
write_candidate_core() {
  mkdir -p "$WORK/candidate/neat-core/src"
  cat >"$WORK/candidate/neat-core/Cargo.toml" <<TOML
[package]
name = "neat-core"
version = "${1:-0.15.9}"
edition = "2021"
TOML
  printf 'pub fn candidate_only() -> u32 { 1 }\n' \
    >"$WORK/candidate/neat-core/src/lib.rs"
  printf '[workspace]\nmembers = ["neat-core"]\nresolver = "2"\n' \
    >"$WORK/candidate/Cargo.toml"
}

# The released core the consumer pins: same crate, without `candidate_only`,
# published at tag v$1 (default v0.15.9).
publish_released_core() {
  local version="${1:-0.15.9}"
  mkdir -p "$WORK/released/neat-core/src"
  cat >"$WORK/released/neat-core/Cargo.toml" <<TOML
[package]
name = "neat-core"
version = "$version"
edition = "2021"
TOML
  printf 'pub fn released_only() -> u32 { 0 }\n' \
    >"$WORK/released/neat-core/src/lib.rs"
  printf '[workspace]\nmembers = ["neat-core"]\nresolver = "2"\n' \
    >"$WORK/released/Cargo.toml"
  make_bare_repo "$WORK/remotes/NEAT-AI-core" main "$WORK/released" "v$version"
}

# A consumer repository pinning neat-core by git tag, published as a bare repo
# the gate can clone. $1 is extra text appended to its root manifest;
# $PIN_DEP_LINE and $APP_BODY override the pin and the code that exercises it.
publish_pinned_consumer() {
  local dep_line="${PIN_DEP_LINE-}" app_body="${APP_BODY-}"
  [ -n "$dep_line" ] ||
    dep_line='neat-core = { git = "https://github.com/stSoftwareAU/NEAT-AI-core", tag = "v0.15.9" }'
  [ -n "$app_body" ] ||
    app_body='pub fn go() -> u32 { neat_core::candidate_only() }'
  mkdir -p "$WORK/pinned/app/src"
  cat >"$WORK/pinned/Cargo.toml" <<'TOML'
[workspace]
members = ["app"]
resolver = "2"
TOML
  [ "$#" -eq 0 ] || printf '%s\n' "$1" >>"$WORK/pinned/Cargo.toml"
  cat >"$WORK/pinned/app/Cargo.toml" <<TOML
[package]
name = "app"
version = "0.1.0"
edition = "2021"

[dependencies]
$dep_line
TOML
  printf '%s\n' "$app_body" >"$WORK/pinned/app/src/lib.rs"
  make_bare_repo "$WORK/remotes/Pinned.git" Develop "$WORK/pinned"
  printf 'stSoftwareAU/Pinned\n' >"$REGISTRY"
}

@test "a consumer pinning neat-core by git tag is compiled against the candidate core" {
  command -v cargo >/dev/null || skip "cargo is required to compile the fixture consumer"
  use_local_family_remotes
  write_candidate_core
  publish_released_core
  publish_pinned_consumer
  # No stub cargo: this one really compiles. The consumer calls a function only
  # the candidate core defines, so a green check is proof the [patch] override
  # replaced the pinned release.
  run "$SCRIPT" --registry "$REGISTRY" --core "$WORK/candidate"
  [ "$status" -eq 0 ]
  [[ "$output" == *"🩹 stSoftwareAU/Pinned: [patch] 1 git pin(s) of neat-core"* ]]
  [[ "$output" == *"✅ stSoftwareAU/Pinned compiles against this core"* ]]
}

@test "a consumer that already patches neat-core fails the gate instead of compiling some other core" {
  use_local_family_remotes
  write_candidate_core
  publish_released_core
  publish_pinned_consumer '[patch."https://github.com/stSoftwareAU/NEAT-AI-core"]
neat-core = { path = "/somewhere/else/neat-core" }'
  run env PATH="$BIN:$PATH" CARGO_LOG="$CARGO_LOG" \
    "$SCRIPT" --registry "$REGISTRY" --core "$WORK/candidate"
  [ "$status" -eq 1 ]
  [[ "$output" == *"already declares a [patch] for neat-core"* ]]
  [[ "$output" == *"[patch] injection failed"* ]]
  # It is never compiled: a green check against an unknown core is the failure
  # this gate exists to prevent.
  [ ! -f "$CARGO_LOG" ]
}

@test "--workspace mode writes nothing into the sibling checkouts you already have" {
  # Alpha has moved to the git-tag pin; its checkout is the developer's own
  # working tree, so the gate must leave it byte-for-byte alone.
  cat >>"$WORK/Alpha/Cargo.toml" <<'TOML'

[workspace.dependencies]
neat-core = { git = "https://github.com/stSoftwareAU/NEAT-AI-core", tag = "v0.15.9" }
TOML
  cp "$WORK/Alpha/Cargo.toml" "$WORK/Alpha.before"
  run_gate
  [ "$status" -eq 0 ]
  diff "$WORK/Alpha.before" "$WORK/Alpha/Cargo.toml"
  [[ "$output" != *"🩹"* ]]
}

@test "a [patch] cargo ignored fails the gate rather than passing on the release the consumer pins" {
  command -v cargo >/dev/null || skip "cargo is required to compile the fixture consumer"
  use_local_family_remotes
  # The candidate is 0.16.0 and the consumer asks for 0.15: cargo keeps the
  # override out of the crate graph, compiles the release it pins, and exits 0.
  # A gate that only read the exit status would call that "compiles against
  # this core" for a core it never opened.
  write_candidate_core 0.16.0
  publish_released_core 0.15.9
  PIN_DEP_LINE='neat-core = { git = "https://github.com/stSoftwareAU/NEAT-AI-core", tag = "v0.15.9", version = "0.15" }'
  APP_BODY='pub fn go() -> u32 { neat_core::released_only() }'
  export PIN_DEP_LINE APP_BODY
  publish_pinned_consumer
  run "$SCRIPT" --registry "$REGISTRY" --core "$WORK/candidate"
  [ "$status" -eq 1 ]
  [[ "$output" == *"cargo ignored the [patch] override"* ]]
  [[ "$output" == *"was not used in the crate graph"* ]]
  [[ "$output" == *"([patch] not used)"* ]]
  [[ "$output" != *"✅ stSoftwareAU/Pinned compiles"* ]]
}
