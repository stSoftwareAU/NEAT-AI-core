#!/usr/bin/env bats
# Build-profile contract for the workspace root Cargo.toml (Issue #546).
#
# Fleet decision (stSoftwareAU/VibeCoding#4159): dev builds compile as fast as
# possible, release builds produce the most optimised artefact possible and
# compile time is irrelevant. Stable Rust only.
#
# Cargo profiles come from the crate being built, so `neat-core`'s own tables
# govern only this workspace's tests and benches — the settings are still
# declared at the workspace root, workspace-wide, because that is what the
# workspace's own dev/release builds resolve.
#
# These are "what" tests: the TOML assertions read the live manifest through a
# real parser, and the last two splice the live `[profile.*]` tables into a
# throwaway crate and assert on the flags **cargo itself** hands rustc — so a
# key that cargo would ignore (a misspelling, a table at the wrong depth) fails
# here rather than silently doing nothing.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  MANIFEST="${REPO_ROOT}/Cargo.toml"
  export MANIFEST
}

# Emit a throwaway crate in $1 carrying the live root manifest's `[profile.*]`
# tables verbatim, so cargo resolves the real settings.
probe_crate() {
  local dir="$1"
  mkdir -p "${dir}/src"
  echo 'fn main() {}' >"${dir}/src/main.rs"
  python3 - "${dir}/Cargo.toml" <<'PY'
import json
import os
import re
import sys
import tomllib

BARE = re.compile(r"^[A-Za-z0-9_-]+$")


def key(name):
    return name if BARE.match(name) else json.dumps(name)


def value(val):
    if isinstance(val, bool):
        return "true" if val else "false"
    if isinstance(val, (int, float)):
        return repr(val)
    if isinstance(val, str):
        return json.dumps(val)
    if isinstance(val, list):
        return "[" + ", ".join(value(v) for v in val) + "]"
    raise AssertionError(f"unsupported profile value: {val!r}")


def emit(prefix, table):
    lines = [f"[{prefix}]"]
    lines += [f"{key(k)} = {value(v)}" for k, v in table.items()
              if not isinstance(v, dict)]
    lines.append("")
    for name, sub in table.items():
        if isinstance(sub, dict):
            lines += emit(f"{prefix}.{key(name)}", sub)
    return lines


with open(os.environ["MANIFEST"], "rb") as fh:
    profiles = tomllib.load(fh).get("profile") or {}
assert profiles, "root manifest declares no [profile.*] tables"

out = ['[package]', 'name = "profile-probe"', 'version = "0.0.0"',
       'edition = "2024"', '']
for name, table in profiles.items():
    out += emit(f"profile.{key(name)}", table)
with open(sys.argv[1], "w") as fh:
    fh.write("\n".join(out) + "\n")
PY
}

# Print the rustc invocation cargo builds the probe crate with, for profile $2.
probe_rustc_flags() {
  local dir="$1" profile="$2"
  (
    cd "$dir" || exit 1
    CARGO_TARGET_DIR="${dir}/target" cargo build -v --offline \
      --profile "$profile" 2>&1
  ) | grep -F -- '--crate-name profile_probe'
}

@test "dev profile keeps panic file:line but drops full DWARF" {
  run python3 - <<'PY'
import os
import tomllib

with open(os.environ["MANIFEST"], "rb") as fh:
    dev = (tomllib.load(fh).get("profile") or {}).get("dev")
assert dev is not None, "no [profile.dev] table in the root manifest"
assert dev.get("debug") == "line-tables-only", (
    f'expected debug = "line-tables-only", got {dev.get("debug")!r}'
)
PY
  [ "$status" -eq 0 ] || {
    echo "$output"
    false
  }
}

@test "dev profile stays unoptimised and incremental" {
  run python3 - <<'PY'
import os
import tomllib

with open(os.environ["MANIFEST"], "rb") as fh:
    dev = (tomllib.load(fh).get("profile") or {}).get("dev") or {}
# Absent keys keep cargo's dev defaults (opt-level 0, incremental on); an
# explicit value must not walk away from them, or dev builds get slower.
assert dev.get("opt-level", 0) == 0, f'opt-level = {dev["opt-level"]!r}'
assert dev.get("incremental", True) is True, (
    f'incremental = {dev["incremental"]!r}'
)
PY
  [ "$status" -eq 0 ] || {
    echo "$output"
    false
  }
}

@test "release profile is fully optimised workspace-wide, not per package" {
  run python3 - <<'PY'
import os
import tomllib

with open(os.environ["MANIFEST"], "rb") as fh:
    release = (tomllib.load(fh).get("profile") or {}).get("release")
assert release is not None, "no [profile.release] table in the root manifest"
assert release.get("opt-level") == 3, f'opt-level = {release.get("opt-level")!r}'
# `lto = true` is the same mode (cargo emits a bare `-C lto`); the manifest
# spells "fat" so the mode is unambiguous to a reader and to the gate below.
assert release.get("lto") == "fat", f'lto = {release.get("lto")!r}'
assert release.get("codegen-units") == 1, (
    f'codegen-units = {release.get("codegen-units")!r}'
)

# Package-scoped tables may sharpen the workspace-wide settings, never weaken
# them — a per-package opt-level of 1 would quietly de-optimise that crate.
for name, override in (release.get("package") or {}).items():
    assert override.get("opt-level", 3) == 3, (
        f'profile.release.package.{name} lowers opt-level to '
        f'{override["opt-level"]!r}'
    )
    assert override.get("lto", "fat") in ("fat", True), (
        f'profile.release.package.{name} disables fat LTO: {override["lto"]!r}'
    )
PY
  [ "$status" -eq 0 ] || {
    echo "$output"
    false
  }
}

@test "no build input pins target-cpu=native — consumers own that flag" {
  # The wasm32/wasm64 bundles and every downstream consumer must stay portable,
  # so the flag belongs in a consumer's own config or build invocation. Prose
  # explaining the rule is free; build inputs are what is swept.
  local -a inputs=()
  while IFS= read -r path; do
    inputs+=("$path")
  done < <(
    cd "$REPO_ROOT" || exit 1
    find .cargo scripts .github/workflows -type f \
      \( -name '*.toml' -o -name '*.sh' -o -name '*.yml' \) 2>/dev/null
  )
  [ "${#inputs[@]}" -gt 0 ]
  local failed=0 path
  for path in "${inputs[@]}"; do
    if grep -Fq 'target-cpu=native' "${REPO_ROOT}/${path}"; then
      echo "target-cpu=native pinned by ${path}" >&2
      failed=1
    fi
  done
  [ "$failed" -eq 0 ]
}

@test "cargo resolves the dev profile to line-tables-only debuginfo" {
  probe_crate "$BATS_TEST_TMPDIR"
  run probe_rustc_flags "$BATS_TEST_TMPDIR" dev
  [ "$status" -eq 0 ]
  echo "$output" | grep -Fq -- '-C debuginfo=line-tables-only'
  # Fast dev builds still need incremental compilation and no optimisation.
  echo "$output" | grep -Fq -- '-C incremental='
  ! echo "$output" | grep -Eq -- '-C opt-level=[1-9]'
}

@test "cargo resolves the release profile to fat LTO in one codegen unit" {
  probe_crate "$BATS_TEST_TMPDIR"
  run probe_rustc_flags "$BATS_TEST_TMPDIR" release
  [ "$status" -eq 0 ]
  echo "$output" | grep -Fq -- '-C opt-level=3'
  echo "$output" | grep -Fq -- '-C lto=fat'
  # Anchored: a bare -F match on "=1" also accepts "=16".
  echo "$output" | grep -Eq -- '-C codegen-units=1([[:space:]]|$)'
}
