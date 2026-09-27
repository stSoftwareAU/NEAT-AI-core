#!/usr/bin/env bats
# Tests for quiet `cargo test` output on the green path (Issue #735).
#
# A green gate should be quiet and a red one fully informative. These are
# "what" tests: each takes the live `cargo test` commands — the `cargo test`
# lines of quality.sh and the `run:` of ci.yml's "Run tests" / "Run doctests"
# steps — runs them against a throwaway crate, and asserts on the output:
#   - a passing suite prints no per-test `test <name> ... ok` line;
#   - a failing test still prints its name, assertion message and panic site.

load helpers

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORK="$(mktemp -d)"
  export CARGO_TARGET_DIR="${WORK}/target"
  if ! command -v cargo &>/dev/null; then
    echo "cargo is required to run the live cargo test commands" >&2
    return 1
  fi

  # quality.sh's cargo test commands, in order.
  grep -E '^cargo test ' "${REPO_ROOT}/quality.sh" >"${WORK}/quality.cmds"
  if [ "$(wc -l <"${WORK}/quality.cmds")" -ne 2 ]; then
    echo "expected quality.sh to hold exactly two cargo test commands" >&2
    cat "${WORK}/quality.cmds" >&2
    return 1
  fi
}

teardown() {
  cd / || return 1
  rm -rf "$WORK"
}

# ci_step_cmd <step-name> — the run: body of the ci.yml step named <step-name>.
ci_step_cmd() {
  mkdir -p "${WORK}/step"
  extract_step "${REPO_ROOT}/.github/workflows/ci.yml" "$1" "${WORK}/step" >&2
  cat "${WORK}/step/step.sh"
}

# write_crate <fail|pass> — a minimal library crate with a unit test and a
# doctest, both passing, or both failing with a recognisable message.
write_crate() {
  local crate="${WORK}/crate"
  mkdir -p "${crate}/src"
  cat >"${crate}/Cargo.toml" <<'TOML'
[package]
name = "quiet_probe"
version = "0.1.0"
edition = "2021"

[workspace]
TOML
  local expected=2
  [ "$1" = "fail" ] && expected=3
  cat >"${crate}/src/lib.rs" <<RS
/// Adds one.
///
/// \`\`\`
/// assert_eq!(quiet_probe::add_one(1), ${expected}, "doctest probe message");
/// \`\`\`
pub fn add_one(x: i32) -> i32 {
    x + 1
}

#[cfg(test)]
mod tests {
    #[test]
    fn probe_case() {
        assert_eq!(super::add_one(1), ${expected}, "unit probe message");
    }
}
RS
  cd "$crate" || return 1
}

run_cmd() {
  run bash -c "$1" </dev/null
  echo "$output"
}

@test "quality.sh and ci.yml run the same cargo test commands" {
  require_python3
  [ "$(sed -n 1p "${WORK}/quality.cmds")" = "$(ci_step_cmd 'Run tests')" ]
  [ "$(sed -n 2p "${WORK}/quality.cmds")" = "$(ci_step_cmd 'Run doctests')" ]
}

@test "a green test suite prints no per-test ok line" {
  write_crate pass
  run_cmd "$(sed -n 1p "${WORK}/quality.cmds")"
  [ "$status" -eq 0 ]
  [[ "$output" != *"probe_case ... ok"* ]]
  [[ "$output" == *"test result: ok"* ]]
}

@test "a green doctest suite prints no per-test ok line" {
  write_crate pass
  run_cmd "$(sed -n 2p "${WORK}/quality.cmds")"
  [ "$status" -eq 0 ]
  [[ "$output" != *"add_one"*"... ok"* ]]
  [[ "$output" == *"test result: ok"* ]]
}

@test "a failing test still prints its name, message and panic site" {
  write_crate fail
  run_cmd "$(sed -n 1p "${WORK}/quality.cmds")"
  [ "$status" -ne 0 ]
  [[ "$output" == *"probe_case"* ]]
  [[ "$output" == *"unit probe message"* ]]
  [[ "$output" == *"panicked at"* ]]
}

@test "a failing doctest still prints its name, message and panic site" {
  write_crate fail
  run_cmd "$(sed -n 2p "${WORK}/quality.cmds")"
  [ "$status" -ne 0 ]
  [[ "$output" == *"add_one"* ]]
  [[ "$output" == *"doctest probe message"* ]]
  [[ "$output" == *"panicked at"* ]]
}
