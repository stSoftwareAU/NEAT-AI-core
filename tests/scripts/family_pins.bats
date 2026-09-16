#!/usr/bin/env bats
# Tests for scripts/family-pins.sh (Issue #681) — the canonical helper every
# NEAT-AI family repository copies byte-for-byte to move its family git-tag
# pins to the latest release.
#
# These are "what" tests: each runs the real script against a fixture checkout
# whose family remote is a *real* local bare git repository carrying real `v*`
# tags, reached through a `url.<base>.insteadOf` rewrite in a throwaway global
# git config. Nothing is stubbed on the git side, so "resolves the newest
# release tag" is an assertion over real `git ls-remote` output rather than
# over a mock. `cargo` is a shim that records its invocations, which is what
# makes "ran cargo update for the moved pin" — and "ran no cargo command at
# all" — assertions rather than inferences.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  SCRIPT="${REPO_ROOT}/scripts/family-pins.sh"

  WORK="${BATS_TEST_TMPDIR}"
  REMOTES="${WORK}/remotes"
  CONSUMER="${WORK}/consumer"
  SHIM_DIR="${WORK}/shims"
  export FAMILY_PINS_SHIM_LOG="${WORK}/cargo-invocations.log"
  mkdir -p "$REMOTES" "$SHIM_DIR" "$CONSUMER"

  # A throwaway global config rewrites every family URL onto the local bare
  # remotes, so the script's real `git ls-remote` never leaves the machine.
  export GIT_CONFIG_NOSYSTEM=1
  export GIT_CONFIG_GLOBAL="${WORK}/gitconfig"
  cat >"$GIT_CONFIG_GLOBAL" <<EOF
[url "file://${REMOTES}/"]
	insteadOf = https://github.com/stSoftwareAU/
EOF

  cat >"${SHIM_DIR}/cargo" <<'SHIM'
#!/usr/bin/env bash
echo "$*" >>"$FAMILY_PINS_SHIM_LOG"
if [ "${FAMILY_PINS_SHIM_UPDATE_FAILS:-0}" = "1" ]; then
  echo "error: package ID specification did not match any packages" >&2
  exit 101
fi
exit 0
SHIM
  chmod +x "${SHIM_DIR}/cargo"
  export PATH="${SHIM_DIR}:${PATH}"

  make_remote NEAT-AI-core v0.15.9 v0.15.10 v0.16.0 v0.17.0-rc1
  make_remote NEAT-AI-Rebase v1.0.0 v1.2.0
}

# --- fixture plumbing -------------------------------------------------------

# make_remote <name> <tag>… — a bare repository at $REMOTES/<name> carrying
# exactly those tags, which is what the family URL resolves to under the
# insteadOf rewrite above.
make_remote() {
  local name="$1" src="${WORK}/sources/$1"
  shift
  mkdir -p "$src"
  printf 'fixture remote\n' >"$src/README.md"
  git init -q "$src"
  git -C "$src" add -A
  git -C "$src" -c user.email=fixture@example.com -c user.name=fixture \
    commit -q -m "fixture"
  local tag
  for tag in "$@"; do
    git -C "$src" tag "$tag"
  done
  git clone -q --bare "$src" "${REMOTES}/${name}" 2>/dev/null
}

# write_consumer <manifest-body> — a consumer checkout whose root manifest is
# the given body.
write_consumer() {
  mkdir -p "$CONSUMER"
  cat >"${CONSUMER}/Cargo.toml"
}

run_pins() {
  run env -u RUSTFLAGS "$@" bash -c "cd '$CONSUMER' && '$SCRIPT' $EXTRA_ARGS"
}

EXTRA_ARGS=""

# --- behaviour --------------------------------------------------------------

@test "an outdated inline pin moves to the newest release and Cargo.lock follows" {
  write_consumer <<'TOML'
[package]
name = "fixture-consumer"
version = "0.1.0"

[dependencies]
neat-core = { git = "https://github.com/stSoftwareAU/NEAT-AI-core", tag = "v0.15.9" }
TOML
  run_pins
  [ "$status" -eq 0 ]
  [[ "$output" == *"neat-core v0.15.9 → v0.16.0"* ]]
  grep -q 'tag = "v0.16.0"' "${CONSUMER}/Cargo.toml"
  ! grep -q 'v0.15.9' "${CONSUMER}/Cargo.toml"
  # The git URL beside it is untouched.
  grep -q 'git = "https://github.com/stSoftwareAU/NEAT-AI-core"' "${CONSUMER}/Cargo.toml"
  grep -q '^update --package neat-core$' "$FAMILY_PINS_SHIM_LOG"
}

@test "a pre-release tag is never what a pin is moved onto" {
  # The fixture remote's newest tag is v0.17.0-rc1; the newest *release* is
  # v0.16.0, and that is the pin a consumer must end up on.
  write_consumer <<'TOML'
[package]
name = "fixture-consumer"
version = "0.1.0"

[dependencies]
neat-core = { git = "https://github.com/stSoftwareAU/NEAT-AI-core", tag = "v0.15.9" }
TOML
  run_pins
  [ "$status" -eq 0 ]
  ! grep -q 'rc1' "${CONSUMER}/Cargo.toml"
  grep -q 'tag = "v0.16.0"' "${CONSUMER}/Cargo.toml"
}

@test "an already-current pin produces no diff, no cargo command and exit 0" {
  write_consumer <<'TOML'
[package]
name = "fixture-consumer"
version = "0.1.0"

[dependencies]
neat-core = { git = "https://github.com/stSoftwareAU/NEAT-AI-core", tag = "v0.16.0" }
TOML
  cp "${CONSUMER}/Cargo.toml" "${WORK}/before.toml"
  run_pins
  [ "$status" -eq 0 ]
  diff "${WORK}/before.toml" "${CONSUMER}/Cargo.toml"
  [ ! -f "$FAMILY_PINS_SHIM_LOG" ]
}

@test "running it twice is idempotent — the second run changes nothing" {
  write_consumer <<'TOML'
[package]
name = "fixture-consumer"
version = "0.1.0"

[dependencies]
neat-core = { git = "https://github.com/stSoftwareAU/NEAT-AI-core", tag = "v0.15.9" }
TOML
  run_pins
  [ "$status" -eq 0 ]
  cp "${CONSUMER}/Cargo.toml" "${WORK}/after-first.toml"
  rm -f "$FAMILY_PINS_SHIM_LOG"
  run_pins
  [ "$status" -eq 0 ]
  diff "${WORK}/after-first.toml" "${CONSUMER}/Cargo.toml"
  [ ! -f "$FAMILY_PINS_SHIM_LOG" ]
}

@test "a remote that cannot be listed fails the run instead of leaving the pin stale" {
  write_consumer <<'TOML'
[package]
name = "fixture-consumer"
version = "0.1.0"

[dependencies]
gone = { git = "https://github.com/stSoftwareAU/NEAT-AI-Missing", tag = "v0.1.0" }
TOML
  run_pins
  [ "$status" -eq 1 ]
  [[ "$output" == *"cannot list the tags of https://github.com/stSoftwareAU/NEAT-AI-Missing"* ]]
  grep -q 'tag = "v0.1.0"' "${CONSUMER}/Cargo.toml"
}

@test "every workspace member's pins move, not just the root manifest's" {
  write_consumer <<'TOML'
[workspace]
members = ["app"]
resolver = "2"
TOML
  mkdir -p "${CONSUMER}/app"
  cat >"${CONSUMER}/app/Cargo.toml" <<'TOML'
[package]
name = "app"
version = "0.1.0"

[dependencies]
neat-core = { git = "https://github.com/stSoftwareAU/NEAT-AI-core", tag = "v0.15.9" }
rebase = { git = "https://github.com/stSoftwareAU/NEAT-AI-Rebase", tag = "v1.0.0" }
TOML
  run_pins
  [ "$status" -eq 0 ]
  grep -q 'tag = "v0.16.0"' "${CONSUMER}/app/Cargo.toml"
  grep -q 'tag = "v1.2.0"' "${CONSUMER}/app/Cargo.toml"
  [[ "$output" == *"app/Cargo.toml"* ]]
  grep -q '^update --package neat-core$' "$FAMILY_PINS_SHIM_LOG"
  grep -q '^update --package rebase$' "$FAMILY_PINS_SHIM_LOG"
}

@test "the dependency-table form of a pin moves too" {
  write_consumer <<'TOML'
[package]
name = "fixture-consumer"
version = "0.1.0"

[dependencies.neat-core]
git = "https://github.com/stSoftwareAU/NEAT-AI-core"
tag = "v0.15.9"
default-features = false
TOML
  run_pins
  [ "$status" -eq 0 ]
  grep -q '^tag = "v0.16.0"$' "${CONSUMER}/Cargo.toml"
  grep -q '^default-features = false$' "${CONSUMER}/Cargo.toml"
  grep -q '^update --package neat-core$' "$FAMILY_PINS_SHIM_LOG"
}

@test "a commented-out pin is left exactly as it is" {
  write_consumer <<'TOML'
[package]
name = "fixture-consumer"
version = "0.1.0"

[dependencies]
# neat-core = { git = "https://github.com/stSoftwareAU/NEAT-AI-core", tag = "v0.15.9" }
TOML
  cp "${CONSUMER}/Cargo.toml" "${WORK}/before.toml"
  run_pins
  [ "$status" -eq 0 ]
  diff "${WORK}/before.toml" "${CONSUMER}/Cargo.toml"
}

@test "a dependency outside the family is not touched" {
  write_consumer <<'TOML'
[package]
name = "fixture-consumer"
version = "0.1.0"

[dependencies]
other = { git = "https://github.com/someone-else/Other", tag = "v0.0.1" }
serde = "1"
TOML
  cp "${CONSUMER}/Cargo.toml" "${WORK}/before.toml"
  run_pins
  [ "$status" -eq 0 ]
  diff "${WORK}/before.toml" "${CONSUMER}/Cargo.toml"
}

@test "a family pin spread over several lines fails loud rather than being skipped" {
  write_consumer <<'TOML'
[package]
name = "fixture-consumer"
version = "0.1.0"

[dependencies]
neat-core = { git = "https://github.com/stSoftwareAU/NEAT-AI-core",
              tag = "v0.15.9" }
TOML
  run_pins
  [ "$status" -ne 0 ]
  [[ "$output" == *"spread over several lines"* ]]
}

@test "a failing cargo update fails the run rather than reporting a moved pin" {
  write_consumer <<'TOML'
[package]
name = "fixture-consumer"
version = "0.1.0"

[dependencies]
neat-core = { git = "https://github.com/stSoftwareAU/NEAT-AI-core", tag = "v0.15.9" }
TOML
  run_pins FAMILY_PINS_SHIM_UPDATE_FAILS=1
  [ "$status" -eq 1 ]
  [[ "$output" == *"cargo update --package neat-core failed"* ]]
}

@test "--manifest scans exactly the manifest named" {
  write_consumer <<'TOML'
[workspace]
members = ["app"]
TOML
  mkdir -p "${CONSUMER}/app" "${CONSUMER}/other"
  cat >"${CONSUMER}/app/Cargo.toml" <<'TOML'
[package]
name = "app"
version = "0.1.0"

[dependencies]
neat-core = { git = "https://github.com/stSoftwareAU/NEAT-AI-core", tag = "v0.15.9" }
TOML
  cat >"${CONSUMER}/other/Cargo.toml" <<'TOML'
[package]
name = "other"
version = "0.1.0"

[dependencies]
neat-core = { git = "https://github.com/stSoftwareAU/NEAT-AI-core", tag = "v0.15.9" }
TOML
  EXTRA_ARGS="--manifest other/Cargo.toml"
  run_pins
  [ "$status" -eq 0 ]
  grep -q 'tag = "v0.16.0"' "${CONSUMER}/other/Cargo.toml"
  grep -q 'tag = "v0.15.9"' "${CONSUMER}/app/Cargo.toml"
}

@test "an unknown argument is a usage error" {
  write_consumer <<'TOML'
[package]
name = "fixture-consumer"
version = "0.1.0"
TOML
  EXTRA_ARGS="--nope"
  run_pins
  [ "$status" -eq 2 ]
  [[ "$output" == *"unknown argument: --nope"* ]]
}
