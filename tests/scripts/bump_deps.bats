#!/usr/bin/env bats
# Tests for bump-deps.sh — Cargo dependency refresh helper (Issue #38).
# NEAT-AI-core is the root of the dependency chain (no internal stSoftware
# crates), so this script focuses on the external (crates.io) bump, the
# audit gate, and the dual native/WASM build verification.

setup() {
  SCRIPT_UNDER_TEST="${BATS_TEST_DIRNAME}/../../bump-deps.sh"
  [ -x "$SCRIPT_UNDER_TEST" ] || chmod +x "$SCRIPT_UNDER_TEST"

  TMP_REPO="$(mktemp -d)"
  export TMP_REPO
}

teardown() {
  rm -rf "$TMP_REPO"
}

@test "shows usage with --help" {
  run "$SCRIPT_UNDER_TEST" --help
  [ "$status" -eq 0 ]
  [[ "$output" == *"Usage:"* ]]
  [[ "$output" == *"--quarantine-hours"* ]]
}

@test "rejects unknown options" {
  run "$SCRIPT_UNDER_TEST" --nonsense
  [ "$status" -ne 0 ]
  [[ "$output" == *"unknown option"* ]]
}

@test "rejects non-integer quarantine hours" {
  run "$SCRIPT_UNDER_TEST" --quarantine-hours abc \
    --skip-external --skip-audit --skip-build \
    --repo "$TMP_REPO"
  [ "$status" -ne 0 ]
  [[ "$output" == *"quarantine-hours"* ]]
}

@test "all skip flags: produces a clean no-op" {
  run "$SCRIPT_UNDER_TEST" \
    --skip-external --skip-audit --skip-build \
    --repo "$TMP_REPO"
  [ "$status" -eq 0 ]
  [[ "$output" == *"no bumps"* ]]
}

@test "summary line is single-line and lists every stage" {
  run "$SCRIPT_UNDER_TEST" \
    --skip-external --skip-audit --skip-build \
    --repo "$TMP_REPO"
  [ "$status" -eq 0 ]
  # Final line of output is the one-line summary.
  last="$(printf '%s\n' "$output" | tail -n 1)"
  [[ "$last" == *"external="* ]]
  [[ "$last" == *"audit="* ]]
  [[ "$last" == *"build="* ]]
}

@test "summary includes WASM build stage label" {
  # --skip-build short-circuits the build stage but the summary should still
  # carry the build= field — the script's contract is that build covers both
  # native and wasm32 targets.
  run "$SCRIPT_UNDER_TEST" \
    --skip-external --skip-audit --skip-build \
    --repo "$TMP_REPO"
  [ "$status" -eq 0 ]
  [[ "$output" == *"build="* ]]
}

@test "check-published: ancient timestamp is older than quarantine (exit 0)" {
  run "$SCRIPT_UNDER_TEST" --check-published "2020-01-01T00:00:00Z" 24
  [ "$status" -eq 0 ]
}

@test "check-published: very recent timestamp is within quarantine (exit 1)" {
  recent="$(python3 -c 'import datetime; print(datetime.datetime.now(datetime.timezone.utc).isoformat())')"
  run "$SCRIPT_UNDER_TEST" --check-published "$recent" 24
  [ "$status" -eq 1 ]
}

@test "check-published: quarantine of zero hours always allows the bump" {
  recent="$(python3 -c 'import datetime; print(datetime.datetime.now(datetime.timezone.utc).isoformat())')"
  run "$SCRIPT_UNDER_TEST" --check-published "$recent" 0
  [ "$status" -eq 0 ]
}

@test "check-published: invalid timestamp surfaces an error" {
  run "$SCRIPT_UNDER_TEST" --check-published "not-a-date" 24
  [ "$status" -ne 0 ]
}

@test "VIBE_BUMP_QUARANTINE_HOURS env var is honoured for quarantine default" {
  recent="$(python3 -c 'import datetime; print(datetime.datetime.now(datetime.timezone.utc).isoformat())')"
  # With a 0-hour default in the env, a fresh timestamp is allowed.
  VIBE_BUMP_QUARANTINE_HOURS=0 run "$SCRIPT_UNDER_TEST" --check-published "$recent" 0
  [ "$status" -eq 0 ]
}

@test "rejects --skip-internal (NEAT-AI-core has no internal deps)" {
  # NEAT-AI-core is the root — there is no internal step, so the option must
  # not exist (otherwise callers might assume one). It should be rejected as
  # an unknown option just like any other typo.
  run "$SCRIPT_UNDER_TEST" --skip-internal
  [ "$status" -ne 0 ]
  [[ "$output" == *"unknown option"* ]]
}

@test "sources HOME/.cargo/env so cargo is found in non-login shells" {
  # quality.sh already sources ~/.cargo/env; bump-deps must too — the Vibe
  # Coder worker invokes bump-deps before quality.sh, and a non-login shell
  # often has rustup's cargo only via that file (PR #539 comment).
  fake_home="$(mktemp -d)"
  mkdir -p "$fake_home/.cargo" "$fake_home/bin"
  cat >"$fake_home/bin/cargo" <<'EOF'
#!/usr/bin/env bash
# Stub: empty dry-run → bump_external reports "no updates" and exits 0.
exit 0
EOF
  chmod +x "$fake_home/bin/cargo"
  cat >"$fake_home/.cargo/env" <<EOF
export PATH="$fake_home/bin:\$PATH"
EOF

  # PATH has no cargo; HOME points at the stub env that would add it.
  run env HOME="$fake_home" PATH="/usr/bin:/bin:/usr/sbin:/sbin" \
    "$SCRIPT_UNDER_TEST" --skip-audit --skip-build --repo "$TMP_REPO"
  rm -rf "$fake_home"
  [ "$status" -eq 0 ]
  [[ "$output" != *"cargo not available"* ]]
  [[ "$output" == *"external=no updates"* ]]
}

# --- Issue #598: audit tool selection, versioned specs, grouped retry --------
#
# The worker host carries cargo-deny but not cargo-audit, so the old
# `cargo audit`-only audit stage exited 1 on every unattended run and every
# bump was reverted. These tests drive the real script against stub tools on a
# stub PATH, so they assert on the script's observable outcome (exit code,
# reported lines, the argv it handed cargo) rather than on its source.

# Build a stub PATH holding a fake `cargo` plus a fake Cargo.lock, so the
# external bump and audit stages run without network or a real toolchain.
setup_stub_cargo() {
  STUB_BIN="$TMP_REPO/bin"
  mkdir -p "$STUB_BIN" "$TMP_REPO/home" "$TMP_REPO/publish"
  STUB_LOG="$TMP_REPO/cargo-argv.log"
  STUB_DRY_RUN="$TMP_REPO/dry-run.txt"
  STUB_DRY_RUN_STATUS="$TMP_REPO/dry-run-status.txt"
  STUB_TARGETS="$TMP_REPO/targets.txt"
  STUB_REJECT="$TMP_REPO/reject.txt"
  STUB_DRAG="$TMP_REPO/drag.txt"
  STUB_AUDIT_OUT="$TMP_REPO/audit-output.txt"
  STUB_AUDIT_STATUS="$TMP_REPO/audit-status.txt"
  STUB_LOCK_READONLY="$TMP_REPO/lock-readonly.txt"
  CURL_LOG="$TMP_REPO/curl-argv.log"
  STUB_CURL_OUT="$TMP_REPO/curl-output.txt"
  STUB_CURL_STATUS="$TMP_REPO/curl-status.txt"
  : >"$STUB_LOG"
  : >"$STUB_DRY_RUN"
  echo 0 >"$STUB_DRY_RUN_STATUS"
  : >"$STUB_TARGETS"
  : >"$STUB_REJECT"
  : >"$STUB_DRAG"
  : >"$STUB_AUDIT_OUT"
  echo 0 >"$STUB_AUDIT_STATUS"
  : >"$STUB_LOCK_READONLY"
  : >"$CURL_LOG"
  : >"$STUB_CURL_OUT"
  echo 0 >"$STUB_CURL_STATUS"
  export STUB_LOG STUB_DRY_RUN STUB_DRY_RUN_STATUS STUB_TARGETS STUB_REJECT \
    STUB_DRAG STUB_AUDIT_OUT STUB_AUDIT_STATUS STUB_LOCK_READONLY \
    CURL_LOG STUB_CURL_OUT STUB_CURL_STATUS

  cat >"$STUB_BIN/cargo" <<'STUB'
#!/usr/bin/env bash
# Stub cargo: records its argv, serves a canned `update --dry-run`, applies
# updates to the fake Cargo.lock, and replays canned audit output.
set -uo pipefail
printf '%s\n' "$*" >>"$STUB_LOG"
cmd="${1:-}"
shift || true
case "$cmd" in
  deny|audit)
    cat "$STUB_AUDIT_OUT" >&2
    exit "$(cat "$STUB_AUDIT_STATUS")"
    ;;
  update) ;;
  *) exit 0 ;;
esac

for a in "$@"; do
  if [ "$a" = "--dry-run" ]; then cat "$STUB_DRY_RUN"; exit "$(cat "$STUB_DRY_RUN_STATUS")"; fi
done

specs=()
precise=""
while [ $# -gt 0 ]; do
  case "$1" in
    -p) specs+=("$2"); shift 2 ;;
    --precise) precise="$2"; shift 2 ;;
    *) shift ;;
  esac
done
[ "${#specs[@]}" -gt 0 ] || exit 0

# A crate listed in $STUB_REJECT cannot be pinned with --precise on its own —
# it only resolves when its whole family moves together (the wasm-bindgen
# case, where -p js-sys --precise 0.3.105 is rejected by web-sys 0.3.104).
if [ -n "$precise" ]; then
  for spec in "${specs[@]}"; do
    if grep -qxF "${spec%@*}" "$STUB_REJECT"; then
      echo "error: failed to select a version for \`${spec%@*}\`" >&2
      exit 101
    fi
  done
fi

apply_version() {
  python3 - Cargo.lock "$1" "$2" "$3" <<'PY'
import sys

lock, name, frm, to = sys.argv[1:5]
lines = open(lock).read().splitlines()
current = None
for i, line in enumerate(lines):
    if line.startswith("name = "):
        current = line.split("=", 1)[1].strip().strip('"')
    elif line.startswith("version = ") and current == name:
        if line.split("=", 1)[1].strip().strip('"') == frm:
            lines[i] = 'version = "%s"' % to
            break
open(lock, "w").write("\n".join(lines) + "\n")
PY
}

for spec in "${specs[@]}"; do
  name="${spec%@*}"
  from="${spec##*@}"
  to="$precise"
  if [ -z "$to" ]; then
    to="$(awk -v n="$name" '$1 == n { print $2 }' "$STUB_TARGETS")"
  fi
  [ -n "$to" ] || continue
  apply_version "$name" "$from" "$to"
done

# Crates cargo drags along to satisfy the versions it was asked for, named in
# $STUB_DRAG as "<name> <from> <to>" — the spec never mentions them.
while read -r dname dfrom dto; do
  [ -n "${dname:-}" ] || continue
  apply_version "$dname" "$dfrom" "$dto"
done <"$STUB_DRAG"
# Models a lockfile the script can no longer write back: the file is left
# read-only, so a later restore of the pre-run snapshot cannot land.
[ -s "$STUB_LOCK_READONLY" ] && chmod 0444 Cargo.lock
exit 0
STUB
  chmod +x "$STUB_BIN/cargo"

  cat >"$STUB_BIN/curl" <<'STUB'
#!/usr/bin/env bash
# Stub curl: records its argv and replays a canned status and payload, so the
# crates.io lookup is exercised without reaching the network.
printf '%s\n' "$*" >>"$CURL_LOG"
cat "$STUB_CURL_OUT"
exit "$(cat "$STUB_CURL_STATUS")"
STUB
  chmod +x "$STUB_BIN/curl"
}

# Write a minimal Cargo.lock holding the given "<name> <version>" pairs.
write_fake_lock() {
  {
    echo 'version = 4'
    for pair in "$@"; do
      set -- $pair
      printf '\n[[package]]\nname = "%s"\nversion = "%s"\n' "$1" "$2"
    done
  } >"$TMP_REPO/Cargo.lock"
}

# Put <name> on the stub PATH so `command -v <name>` finds it.
install_stub_tool() {
  printf '#!/usr/bin/env bash\nexit 0\n' >"$STUB_BIN/$1"
  chmod +x "$STUB_BIN/$1"
}

# Run bump-deps.sh against the stub PATH only — no real cargo, no network.
run_stubbed() {
  run env -i \
    HOME="$TMP_REPO/home" \
    PATH="$STUB_BIN:/usr/bin:/bin:/usr/sbin:/sbin" \
    BUMP_DEPS_PUBLISH_FIXTURE="${STUB_FIXTURE_DIR-$TMP_REPO/publish}" \
    STUB_LOG="$STUB_LOG" STUB_DRY_RUN="$STUB_DRY_RUN" \
    STUB_DRY_RUN_STATUS="$STUB_DRY_RUN_STATUS" \
    STUB_TARGETS="$STUB_TARGETS" STUB_REJECT="$STUB_REJECT" \
    STUB_DRAG="$STUB_DRAG" \
    STUB_AUDIT_OUT="$STUB_AUDIT_OUT" STUB_AUDIT_STATUS="$STUB_AUDIT_STATUS" \
    STUB_LOCK_READONLY="$STUB_LOCK_READONLY" \
    CURL_LOG="$CURL_LOG" STUB_CURL_OUT="$STUB_CURL_OUT" \
    STUB_CURL_STATUS="$STUB_CURL_STATUS" \
    bash "$SCRIPT_UNDER_TEST" --repo "$TMP_REPO" "$@"
}

@test "audit: cargo-deny present and cargo-audit absent passes" {
  setup_stub_cargo
  install_stub_tool cargo-deny
  run_stubbed --skip-external --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"audit: ok"* ]]
  # The advisory scan must have gone through cargo-deny, not cargo audit.
  grep -q "deny .*check advisories" "$STUB_LOG"
  ! grep -qx "audit" "$STUB_LOG"
}

@test "audit: cargo-audit present and cargo-deny absent passes" {
  setup_stub_cargo
  install_stub_tool cargo-audit
  run_stubbed --skip-external --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"audit: ok"* ]]
  grep -q "^audit" "$STUB_LOG"
}

@test "audit: neither scanner installed warns and skips instead of failing the run" {
  # Issue #621: a scanner missing from an unattended host is a host gap, not a
  # bumped tree that must be rejected. Nothing was bumped here, so the run is a
  # clean no-op — but the warning must still name both installs.
  setup_stub_cargo
  run_stubbed --skip-external --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"audit: SKIPPED"* ]]
  [[ "$output" == *"cargo install cargo-deny --locked"* ]]
  [[ "$output" == *"cargo install cargo-audit --locked"* ]]
}

@test "audit: neither scanner installed reverts the bumps rather than landing them unscanned" {
  # Skipping the scan must never leave an unscanned bump behind: the run drops
  # what it bumped and reports a no-op, so the exit stays 0 without weakening
  # the supply-chain gate.
  setup_stub_cargo
  write_fake_lock "cc 1.4.2"
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating cc v1.4.2 -> v1.4.5
TXT
  run_stubbed --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"bump: cc -> 1.4.5"* ]]
  [[ "$output" == *"audit: SKIPPED"* ]]
  [[ "$output" == *"external: reverted — no advisory scanner to verify them"* ]]
  # The summary must not still claim the bump the run dropped.
  [[ "$output" == *"external=0 bumped, 0 deferred (lockfile restored"* ]]
  grep -qx 'version = "1.4.2"' "$TMP_REPO/Cargo.lock"
  ! grep -q '1.4.5' "$TMP_REPO/Cargo.lock"
}

@test "audit: a cargo-deny advisory names the crate and RUSTSEC id" {
  setup_stub_cargo
  install_stub_tool cargo-deny
  # Real `cargo deny --format json check advisories` diagnostic (cargo-deny
  # 0.20.2), trimmed to the fields the parser reads.
  cat >"$STUB_AUDIT_OUT" <<'JSON'
{"fields":{"advisory":{"id":"RUSTSEC-2021-0003","package":"smallvec","title":"Buffer overflow in SmallVec::insert_many"},"code":"vulnerability","graphs":[{"Krate":{"name":"smallvec","version":"1.6.0"}}],"message":"Buffer overflow in SmallVec::insert_many","severity":"error"},"type":"diagnostic"}
{"fields":{"advisories":{"errors":1,"helps":0,"notes":0,"warnings":0}},"type":"summary"}
JSON
  echo 1 >"$STUB_AUDIT_STATUS"
  run_stubbed --skip-external --skip-build
  [ "$status" -eq 1 ]
  [[ "$output" == *"audit: FAILED — smallvec (RUSTSEC-2021-0003)"* ]]
}

@test "audit: a cargo-audit advisory names the crate and RUSTSEC id" {
  setup_stub_cargo
  install_stub_tool cargo-audit
  cat >"$STUB_AUDIT_OUT" <<'TXT'
Crate:     smallvec
Version:   1.6.0
Title:     Buffer overflow in SmallVec::insert_many
Date:      2021-01-08
ID:        RUSTSEC-2021-0003
TXT
  echo 1 >"$STUB_AUDIT_STATUS"
  run_stubbed --skip-external --skip-build
  [ "$status" -eq 1 ]
  [[ "$output" == *"audit: FAILED — smallvec (RUSTSEC-2021-0003)"* ]]
}

@test "external: per-crate update pins the package spec to the locked version" {
  setup_stub_cargo
  write_fake_lock "syn 2.0.119" "syn 3.0.3"
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating syn v3.0.3 -> v3.0.5
TXT
  run_stubbed --skip-audit --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"bump: syn -> 3.0.5"* ]]
  # A crate locked at two majors is ambiguous for a bare `-p syn`; the spec
  # must carry the locked version being replaced.
  grep -qF -- "-p syn@3.0.3 --precise 3.0.5" "$STUB_LOG"
}

@test "external: crates the per-crate pass rejects are retried as one group" {
  setup_stub_cargo
  write_fake_lock "js-sys 0.3.104" "wasm-bindgen 0.2.127"
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating js-sys v0.3.104 -> v0.3.105
    Updating wasm-bindgen v0.2.127 -> v0.2.128
TXT
  # Neither crate resolves alone; together they do.
  printf 'js-sys\nwasm-bindgen\n' >"$STUB_REJECT"
  printf 'js-sys 0.3.105\nwasm-bindgen 0.2.128\n' >"$STUB_TARGETS"
  run_stubbed --skip-audit --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"bump: js-sys -> 0.3.105"* ]]
  [[ "$output" == *"bump: wasm-bindgen -> 0.2.128"* ]]
  [[ "$output" == *"external: 2 bumped, 0 deferred"* ]]
  # One grouped invocation carrying both specs.
  grep -qF -- "-p js-sys@0.3.104" "$STUB_LOG"
  grep -F -- "-p js-sys@0.3.104" "$STUB_LOG" | grep -qF -- "-p wasm-bindgen@0.2.127"
}

@test "external: grouped retry excludes crates still inside quarantine" {
  setup_stub_cargo
  write_fake_lock "js-sys 0.3.104" "quarantined 1.0.0"
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating js-sys v0.3.104 -> v0.3.105
    Updating quarantined v1.0.0 -> v1.0.1
TXT
  python3 -c 'import datetime; print(datetime.datetime.now(datetime.timezone.utc).isoformat())' \
    >"$TMP_REPO/publish/quarantined-1.0.1.iso"
  printf 'js-sys\n' >"$STUB_REJECT"
  printf 'js-sys 0.3.105\nquarantined 1.0.1\n' >"$STUB_TARGETS"
  run_stubbed --skip-audit --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"defer: quarantined 1.0.1"* ]]
  [[ "$output" == *"1 bumped, 1 deferred"* ]]
  # The quarantined crate must never be handed to cargo update.
  ! grep -qF -- "-p quarantined" "$STUB_LOG"
  grep -qx 'version = "1.0.0"' "$TMP_REPO/Cargo.lock"
}

@test "external: grouped retry landing an unapproved version is reverted" {
  setup_stub_cargo
  write_fake_lock "js-sys 0.3.104" "wasm-bindgen 0.2.127"
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating js-sys v0.3.104 -> v0.3.105
    Updating wasm-bindgen v0.2.127 -> v0.2.128
TXT
  printf 'js-sys\nwasm-bindgen\n' >"$STUB_REJECT"
  # The group resolves js-sys past the version the quarantine approved.
  printf 'js-sys 0.3.199\nwasm-bindgen 0.2.128\n' >"$STUB_TARGETS"
  run_stubbed --skip-audit --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"defer: js-sys -> 0.3.105"* ]]
  [[ "$output" == *"defer: wasm-bindgen -> 0.2.128"* ]]
  # Each member reports the revert, not the misleading "cargo update rejected".
  [[ "$output" != *"(cargo update rejected)"* ]]
  [[ "$output" == *"(reverted — grouped retry moved js-sys off its approved target 0.3.105)"* ]]
  # The group must actually have been attempted, in one invocation.
  grep -F -- "-p js-sys@0.3.104" "$STUB_LOG" | grep -qF -- "-p wasm-bindgen@0.2.127"
  # The unapproved version must not survive in the lock.
  ! grep -q '0.3.199' "$TMP_REPO/Cargo.lock"
  grep -qx 'version = "0.3.104"' "$TMP_REPO/Cargo.lock"
  grep -qx 'version = "0.2.127"' "$TMP_REPO/Cargo.lock"
}

@test "external: a crate landed by another crate's update counts as bumped" {
  setup_stub_cargo
  write_fake_lock "cc 1.4.2" "find-msvc-tools 0.1.10"
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating cc v1.4.2 -> v1.4.5
    Updating find-msvc-tools v0.1.10 -> v0.1.12
TXT
  # Updating cc drags find-msvc-tools along, so by the time its own per-crate
  # attempt comes round there is no 0.1.10 left in the lock to move.
  printf 'find-msvc-tools 0.1.10 0.1.12\n' >"$STUB_DRAG"
  printf 'find-msvc-tools\n' >"$STUB_REJECT"
  printf 'find-msvc-tools 0.1.12\n' >"$STUB_TARGETS"
  run_stubbed --skip-audit --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"bump: find-msvc-tools -> 0.1.12"* ]]
  [[ "$output" == *"2 bumped, 0 deferred"* ]]
}

@test "audit: cargo-deny is preferred when both scanners are installed" {
  setup_stub_cargo
  install_stub_tool cargo-deny
  install_stub_tool cargo-audit
  run_stubbed --skip-external --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"audit: ok"* ]]
  # cargo-deny is the repo's own advisory tool, so it wins the tie.
  grep -q "deny .*check advisories" "$STUB_LOG"
  ! grep -qx "audit" "$STUB_LOG"
}

@test "audit: a cargo-deny advisory with no package field names the graph crate" {
  setup_stub_cargo
  install_stub_tool cargo-deny
  # cargo-deny omits advisory.package for some diagnostic kinds; the crate the
  # inclusion graph is rooted at is the offending one either way.
  cat >"$STUB_AUDIT_OUT" <<'JSON'
{"fields":{"advisory":{"id":"RUSTSEC-2021-0003","title":"Buffer overflow in SmallVec::insert_many"},"code":"vulnerability","graphs":[{"Krate":{"name":"smallvec","version":"1.6.0"}}],"message":"Buffer overflow in SmallVec::insert_many","severity":"error"},"type":"diagnostic"}
JSON
  echo 1 >"$STUB_AUDIT_STATUS"
  run_stubbed --skip-external --skip-build
  [ "$status" -eq 1 ]
  [[ "$output" == *"audit: FAILED — smallvec (RUSTSEC-2021-0003)"* ]]
}

@test "external: a crate with no unambiguous package spec is deferred, not guessed" {
  setup_stub_cargo
  # syn is locked at two majors and the dry run names a third version, so no
  # `-p syn@<locked>` spec exists. That must be reported, not passed to cargo
  # as a bare ambiguous `-p syn`.
  write_fake_lock "syn 2.0.119" "syn 3.0.3"
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating syn v3.0.9 -> v3.1.0
TXT
  run_stubbed --skip-audit --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"skip: syn 3.1.0 (locked at several versions"* ]]
  [[ "$output" == *"defer: syn -> 3.1.0"* ]]
  ! grep -qF -- "-p syn" "$STUB_LOG"
}

@test "external: grouped retry dragging a quarantined crate is reverted" {
  setup_stub_cargo
  write_fake_lock "js-sys 0.3.104" "quarantined 1.0.0"
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating js-sys v0.3.104 -> v0.3.105
    Updating quarantined v1.0.0 -> v1.0.1
TXT
  python3 -c 'import datetime; print(datetime.datetime.now(datetime.timezone.utc).isoformat())' \
    >"$TMP_REPO/publish/quarantined-1.0.1.iso"
  printf 'js-sys\n' >"$STUB_REJECT"
  printf 'js-sys 0.3.105\n' >"$STUB_TARGETS"
  # The grouped update lands js-sys but also drags the quarantined crate past
  # the release-age window — the whole group must be reverted.
  printf 'quarantined 1.0.0 1.0.1\n' >"$STUB_DRAG"
  run_stubbed --skip-audit --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"revert: grouped retry moved quarantined quarantined off 1.0.0"* ]]
  [[ "$output" == *"defer: js-sys -> 0.3.105"* ]]
  grep -qx 'version = "1.0.0"' "$TMP_REPO/Cargo.lock"
  grep -qx 'version = "0.3.104"' "$TMP_REPO/Cargo.lock"
}

@test "external: grouped retry keeps a group that also moves a transitive crate" {
  setup_stub_cargo
  write_fake_lock "js-sys 0.3.104" "wasm-bindgen 0.2.127" "bumpalo 3.0.0"
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating js-sys v0.3.104 -> v0.3.105
    Updating wasm-bindgen v0.2.127 -> v0.2.128
TXT
  printf 'js-sys\nwasm-bindgen\n' >"$STUB_REJECT"
  printf 'js-sys 0.3.105\nwasm-bindgen 0.2.128\n' >"$STUB_TARGETS"
  # cargo must be free to move an out-of-group dependency to satisfy the
  # versions the group asked for; that is not a quarantine breach and must not
  # revert a group whose own crates all landed on target.
  printf 'bumpalo 3.0.0 3.1.0\n' >"$STUB_DRAG"
  run_stubbed --skip-audit --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"bump: js-sys -> 0.3.105"* ]]
  [[ "$output" == *"bump: wasm-bindgen -> 0.2.128"* ]]
  [[ "$output" == *"2 bumped, 0 deferred"* ]]
  [[ "$output" != *"revert:"* ]]
  grep -qx 'version = "3.1.0"' "$TMP_REPO/Cargo.lock"
}

# --- Issue #614: the per-crate pass carries the same quarantine contract -----

@test "external: per-crate update dragging a quarantined crate is reverted" {
  setup_stub_cargo
  write_fake_lock "cc 1.4.2" "quarantined 1.0.0"
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating cc v1.4.2 -> v1.4.5
    Updating quarantined v1.0.0 -> v1.0.1
TXT
  python3 -c 'import datetime; print(datetime.datetime.now(datetime.timezone.utc).isoformat())' \
    >"$TMP_REPO/publish/quarantined-1.0.1.iso"
  # Updating cc on its own drags the deferred crate past the release-age
  # window — the per-crate update must be reverted, not silently kept.
  printf 'quarantined 1.0.0 1.0.1\n' >"$STUB_DRAG"
  run_stubbed --skip-audit --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"revert: per-crate update of cc moved quarantined quarantined off 1.0.0"* ]]
  [[ "$output" == *"defer: cc -> 1.4.5"* ]]
  [[ "$output" != *"(cargo update rejected)"* ]]
  grep -qx 'version = "1.0.0"' "$TMP_REPO/Cargo.lock"
  grep -qx 'version = "1.4.2"' "$TMP_REPO/Cargo.lock"
}

@test "external: per-crate update dragging a planned crate off its target is reverted" {
  setup_stub_cargo
  write_fake_lock "cc 1.4.2" "find-msvc-tools 0.1.10"
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating cc v1.4.2 -> v1.4.5
    Updating find-msvc-tools v0.1.10 -> v0.1.12
TXT
  # cc's update drags find-msvc-tools past the version the quarantine
  # approved, so cc reverts; the restored lock then lets find-msvc-tools land
  # its own approved target.
  printf 'find-msvc-tools 0.1.10 0.1.99\n' >"$STUB_DRAG"
  run_stubbed --skip-audit --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"revert: per-crate update of cc moved find-msvc-tools off its approved target 0.1.12"* ]]
  [[ "$output" == *"defer: cc -> 1.4.5"* ]]
  # The restored lock must still let the next crate land: cc sitting at its
  # pre-bump version is not itself a breach.
  [[ "$output" == *"bump: find-msvc-tools -> 0.1.12"* ]]
  ! grep -q '0.1.99' "$TMP_REPO/Cargo.lock"
  grep -qx 'version = "1.4.2"' "$TMP_REPO/Cargo.lock"
}

@test "external: per-crate update moving an out-of-plan transitive crate is kept" {
  setup_stub_cargo
  write_fake_lock "cc 1.4.2" "bumpalo 3.0.0"
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating cc v1.4.2 -> v1.4.5
TXT
  # cargo must stay free to move an out-of-plan dependency to satisfy the
  # version it was asked for — the same allowance the grouped retry makes.
  printf 'bumpalo 3.0.0 3.1.0\n' >"$STUB_DRAG"
  run_stubbed --skip-audit --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"bump: cc -> 1.4.5"* ]]
  [[ "$output" != *"revert:"* ]]
  grep -qx 'version = "3.1.0"' "$TMP_REPO/Cargo.lock"
}

@test "external: per-crate update dragging another major of the same crate is reverted" {
  setup_stub_cargo
  # syn is locked at two majors. The 3.x pin lands, but the update drags the
  # 2.x copy to a version the quarantine never saw — the crate sitting on its
  # target must not hide movement on its other major.
  write_fake_lock "syn 2.0.119" "syn 3.0.3"
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating syn v3.0.3 -> v3.0.5
TXT
  printf 'syn 2.0.119 2.0.999\n' >"$STUB_DRAG"
  run_stubbed --skip-audit --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"revert: per-crate update of syn moved syn off its approved target 3.0.5"* ]]
  [[ "$output" == *"defer: syn -> 3.0.5"* ]]
  ! grep -q '2.0.999' "$TMP_REPO/Cargo.lock"
  grep -qx 'version = "2.0.119"' "$TMP_REPO/Cargo.lock"
  grep -qx 'version = "3.0.3"' "$TMP_REPO/Cargo.lock"
}

@test "external: grouped retry dragging a planned crate it skipped is reverted" {
  setup_stub_cargo
  # syn is planned but has no unambiguous spec, so the group never names it.
  # The group still must not leave it on a version the quarantine never
  # approved — the check covers the whole plan, not just the group members.
  write_fake_lock "js-sys 0.3.104" "syn 2.0.119" "syn 3.0.3"
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating js-sys v0.3.104 -> v0.3.105
    Updating syn v3.0.9 -> v3.1.0
TXT
  printf 'js-sys\n' >"$STUB_REJECT"
  printf 'js-sys 0.3.105\n' >"$STUB_TARGETS"
  printf 'syn 3.0.3 3.0.7\n' >"$STUB_DRAG"
  run_stubbed --skip-audit --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"revert: grouped retry moved syn off its approved target 3.1.0"* ]]
  [[ "$output" == *"defer: js-sys -> 0.3.105"* ]]
  ! grep -q '3.0.7' "$TMP_REPO/Cargo.lock"
  grep -qx 'version = "0.3.104"' "$TMP_REPO/Cargo.lock"
}

@test "external: a bump with no lockfile to verify against is refused, not run" {
  setup_stub_cargo
  # No Cargo.lock — nothing to snapshot, so nothing to verify. The bump must
  # be refused and named rather than run unverified.
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating cc v1.4.2 -> v1.4.5
TXT
  run_stubbed --skip-audit --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"skip: cc 1.4.5 (no ${TMP_REPO}/Cargo.lock to verify against)"* ]]
  [[ "$output" == *"defer: cc -> 1.4.5 (no ${TMP_REPO}/Cargo.lock to verify against)"* ]]
  ! grep -qF -- "-p cc" "$STUB_LOG"
}

# --- Issue #621: a crate that cannot be bumped safely is a deferral ----------

@test "external: a crate cargo update rejects is deferred, not failed" {
  setup_stub_cargo
  write_fake_lock "js-sys 0.3.104"
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating js-sys v0.3.104 -> v0.3.105
TXT
  # Rejected on its own, and the grouped retry cannot land it either — the
  # crate simply stays where it is, which is a deferral and not a run failure.
  printf 'js-sys\n' >"$STUB_REJECT"
  run_stubbed --skip-audit --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"defer: js-sys -> 0.3.105 (cargo update rejected)"* ]]
  [[ "$output" == *"external: 0 bumped, 1 deferred"* ]]
  [[ "$output" != *"failed"* ]]
  grep -qx 'version = "0.3.104"' "$TMP_REPO/Cargo.lock"
}

@test "external: a crate whose release age cannot be established is deferred" {
  setup_stub_cargo
  write_fake_lock "cc 1.4.2"
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating cc v1.4.2 -> v1.4.5
TXT
  # No fixture, so the real crates.io code path runs — against a stub curl that
  # cannot connect (exit 7). An unknown release age is not an ancient one: the
  # crate is held, counted apart from a quarantine wait, and never bumped.
  echo 7 >"$STUB_CURL_STATUS"
  STUB_FIXTURE_DIR="" run_stubbed --skip-audit --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"defer: cc 1.4.5 (publish time lookup failed)"* ]]
  [[ "$output" == *"external: 0 bumped, 0 deferred, 1 release age unknown"* ]]
  ! grep -qF -- "-p cc" "$STUB_LOG"
  grep -qx 'version = "1.4.2"' "$TMP_REPO/Cargo.lock"
  # The lookup is bounded: it cannot hang the run, and one blip is retried.
  grep -qF -- "--connect-timeout 10" "$CURL_LOG"
  grep -qF -- "--max-time 30" "$CURL_LOG"
  grep -qF -- "--retry 2" "$CURL_LOG"
}

@test "external: a failed cargo update --dry-run is reported, not passed off as no updates" {
  setup_stub_cargo
  write_fake_lock "cc 1.4.2"
  echo "error: failed to query replaced source registry crates-io" >"$STUB_DRY_RUN"
  echo 101 >"$STUB_DRY_RUN_STATUS"
  run_stubbed --skip-audit --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"external: plan unavailable (cargo update --dry-run failed)"* ]]
  # "no updates" would claim the registry said there was nothing to do.
  [[ "$output" != *"no updates"* ]]
  [[ "$output" == *"failed to query replaced source registry"* ]]
}

@test "external: a publish time the registry does not name defers the crate" {
  setup_stub_cargo
  write_fake_lock "cc 1.4.2"
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating cc v1.4.2 -> v1.4.5
TXT
  # The lookup answers, but names no publish time for this version. An unknown
  # release age is not an ancient one — the crate must be held, not bumped.
  : >"$TMP_REPO/publish/cc-1.4.5.iso"
  run_stubbed --skip-audit --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"defer: cc 1.4.5 (publish time lookup failed)"* ]]
  ! grep -qF -- "-p cc" "$STUB_LOG"
  grep -qx 'version = "1.4.2"' "$TMP_REPO/Cargo.lock"
}

# --- Issue #621: the two states that must still fail the run -----------------

@test "a missing cargo fails the run loud" {
  setup_stub_cargo
  write_fake_lock "cc 1.4.2"
  # No cargo anywhere: nothing can be done safely, so the run must not pretend
  # otherwise. This is one of the two exits the contract still reserves.
  rm -f "$STUB_BIN/cargo"
  run_stubbed --skip-audit --skip-build
  [ "$status" -eq 1 ]
  [[ "$output" == *"cargo not available"* ]]
}

@test "a Cargo.lock that cannot be restored fails the run loud" {
  setup_stub_cargo
  write_fake_lock "cc 1.4.2"
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating cc v1.4.2 -> v1.4.5
TXT
  # The bump lands, no scanner can vouch for it, and the lockfile has been left
  # unwritable — the revert cannot complete, which is the "lockfile left in a
  # broken state" case the contract still exits non-zero for.
  echo 1 >"$STUB_LOCK_READONLY"
  run_stubbed --skip-build
  chmod 0644 "$TMP_REPO/Cargo.lock"
  [ "$status" -eq 1 ]
  [[ "$output" == *"could not restore"* ]]
}

@test "audit: no scanner reverts a lockfile the run changed even with no crate landed" {
  setup_stub_cargo
  write_fake_lock "cc 1.4.2" "transitive 9.0.0"
  cat >"$STUB_DRY_RUN" <<'TXT'
    Updating cc v1.4.2 -> v1.4.5
TXT
  # cc never lands, so the bump counter stays 0 — but the grouped retry moves
  # an out-of-plan transitive crate, which the quarantine check allows. With no
  # scanner to vouch for it, that change must not survive either.
  printf 'cc\n' >"$STUB_REJECT"
  printf 'transitive 9.0.0 9.9.9\n' >"$STUB_DRAG"
  run_stubbed --skip-build
  [ "$status" -eq 0 ]
  [[ "$output" == *"audit: SKIPPED"* ]]
  [[ "$output" == *"external: reverted — no advisory scanner to verify them"* ]]
  grep -qx 'version = "9.0.0"' "$TMP_REPO/Cargo.lock"
  ! grep -q '9.9.9' "$TMP_REPO/Cargo.lock"
}

@test "rejects a --repo that is not a directory" {
  # A misconfigured repo path is a usage error; it must not be reported as a
  # cargo failure and pass as a green no-op.
  run "$SCRIPT_UNDER_TEST" --repo "$TMP_REPO/does-not-exist" \
    --skip-external --skip-audit --skip-build
  [ "$status" -eq 2 ]
  [[ "$output" == *"is not a directory"* ]]
}
