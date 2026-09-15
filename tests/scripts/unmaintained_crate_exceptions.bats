#!/usr/bin/env bats
# Regression assertions for Issue #677 — `tinytemplate` is unmaintained (1.2.1,
# published 2021-03-04, nothing since; "Project dead?", "Maintenance?" and an
# unanswered CVE report all still open upstream) and it is in this
# repository's resolved graph.
#
# It cannot simply be dropped. `criterion 0.8.2` — the current release, from the
# maintained `criterion-rs` org — declares `tinytemplate` as a **non-optional**
# dependency, so no feature selection removes it; the mitigation the finding
# suggested (turning off `html_reports`) would cost the developer HTML benchmark
# reports and leave the crate resolved exactly as before.
#
# What can be held is the boundary that keeps the finding at `severity:low`:
# `tinytemplate` reaches a developer's `cargo bench` only, never the library,
# the wasm bundle or anything this repository ships. These tests pin that
# boundary, and fail the moment the carrier stops forcing the crate so the
# exception is deleted rather than inherited forever. SECURITY.md carries the
# prose; this file is the gate.
#
# These are "what" tests: they read the dependency graph cargo actually
# resolves — edge kinds and all — not the manifest line that happens to
# declare it.

setup() {
  # Wires the vendored YAML parser in when PyYAML is missing (Issue #642).
  load helpers
  require_python3
  REPO_ROOT="$(cd "${BATS_TEST_DIRNAME}/../.." && pwd)"
  SECURITY_MD="${REPO_ROOT}/SECURITY.md"

  # Unmaintained crates this repository knowingly tolerates, as
  # "<crate> <carrier>" pairs: <carrier> is the maintained dependency that
  # forces <crate> into the graph. One definition, read by every test below and
  # by the SECURITY.md cross-check. Deleting a pair is how an exception ends.
  UNMAINTAINED_EXCEPTIONS=("tinytemplate criterion")
}

# Every Cargo manifest in the tree that resolves its own graph — one per
# committed lockfile, so a workspace added later (as `wasm-bench` was, Issue
# #607) is swept without being wired in by hand.
resolving_manifests() {
  local lock
  while IFS= read -r lock; do
    printf '%s/Cargo.toml\n' "$(dirname "${lock#./}")"
  done < <(cd "$REPO_ROOT" && find . -name Cargo.lock -not -path './target/*' -not -path './.git/*')
}

# cargo's resolved graph for manifest $1, as JSON on stdout. `--locked` keeps
# the read read-only: a test must never rewrite a committed lockfile, and a
# lockfile that no longer matches its manifest fails here loudly rather than
# being silently repaired mid-suite.
metadata() {
  cargo metadata --format-version 1 --locked --manifest-path "${REPO_ROOT}/${1#./}"
}

# The crates manifest $1 ships: every package reachable from a workspace member
# through normal and build edges. Dev-dependency edges are excluded — they are
# compiled by `cargo test`/`cargo bench` on a developer's machine and never
# linked into the library, the wasm bundle or a released artefact.
shipped_crates() {
  metadata "$1" | python3 -c '
import json
import sys

md = json.load(sys.stdin)
nodes = {n["id"]: n for n in md["resolve"]["nodes"]}
names = {p["id"]: p["name"] for p in md["packages"]}

# `kind` is null for a normal dependency, "dev" or "build" otherwise.
SHIPPED = {None, "build"}
seen = set()
stack = list(md["workspace_members"])
while stack:
    pid = stack.pop()
    if pid in seen:
        continue
    seen.add(pid)
    for dep in nodes[pid]["deps"]:
        if any(k.get("kind") in SHIPPED for k in dep["dep_kinds"]):
            stack.append(dep["pkg"])

print("\n".join(sorted({names[pid] for pid in seen})))
'
}

# Every crate in manifest $1s resolved graph, shipped or not.
resolved_crates() {
  metadata "$1" | python3 -c '
import json
import sys

md = json.load(sys.stdin)
print("\n".join(sorted({p["name"] for p in md["packages"]})))
'
}

# Exit 0 when <carrier> ($2) declares <crate> ($3) as a dependency no feature
# selection can switch off, in the graph of manifest $1.
carrier_forces_crate() {
  metadata "$1" | python3 -c '
import json
import sys

carrier, crate = sys.argv[1], sys.argv[2]
md = json.load(sys.stdin)
for pkg in md["packages"]:
    if pkg["name"] != carrier:
        continue
    for dep in pkg["dependencies"]:
        if dep["name"] == crate and not dep["optional"]:
            sys.exit(0)
    version = pkg["version"]
    sys.exit(
        f"{carrier} {version} no longer requires {crate} unconditionally —"
        f" drop {crate} and delete the exception"
    )
sys.exit(f"{carrier} is not in this graph — delete the exception")
' "$2" "$3"
}

@test "every unmaintained-crate exception stays off the shipped path" {
  local failed=0 manifest entry crate
  while IFS= read -r manifest; do
    local shipped
    shipped="$(shipped_crates "$manifest")"
    for entry in "${UNMAINTAINED_EXCEPTIONS[@]}"; do
      crate="${entry%% *}"
      if printf '%s\n' "$shipped" | grep -Fxq "$crate"; then
        printf '%s ships the unmaintained crate %s\n' "$manifest" "$crate" >&2
        failed=1
      fi
    done
  done < <(resolving_manifests)
  [ "$failed" -eq 0 ]
}

# Oracle for the sweep above: a reader that returned nothing would pass it on
# every graph. These three crates pin the three answers it must tell apart.
@test "the shipped-graph reader tells a shipped crate from a dev-only one" {
  run shipped_crates "Cargo.toml"
  [ "$status" -eq 0 ]
  local shipped="$output"
  # serde is a normal dependency of neat-core: it ships.
  printf '%s\n' "$shipped" | grep -Fxq "serde"
  # criterion is a dev-dependency: resolved, but never shipped.
  ! printf '%s\n' "$shipped" | grep -Fxq "criterion"

  run resolved_crates "Cargo.toml"
  [ "$status" -eq 0 ]
  printf '%s\n' "$output" | grep -Fxq "criterion"
}

# The exception is only defensible while the carrier leaves no choice. When a
# future criterion makes tinytemplate optional — or drops it — this fails, and
# the fix is to remove the crate and this exception rather than to widen it.
@test "every unmaintained-crate exception is still forced by its carrier" {
  local entry crate carrier
  for entry in "${UNMAINTAINED_EXCEPTIONS[@]}"; do
    crate="${entry%% *}"
    carrier="${entry##* }"
    run carrier_forces_crate "Cargo.toml" "$carrier" "$crate"
    [ "$status" -eq 0 ] || {
      printf '%s\n' "$output" >&2
      return 1
    }
  done
}

@test "SECURITY.md documents every unmaintained-crate exception" {
  local section entry crate carrier
  section="$(python3 - "$SECURITY_MD" <<'PY'
import re
import sys

lines = open(sys.argv[1], encoding="utf-8").read().splitlines()
start = next(
    (
        i
        for i, l in enumerate(lines)
        if re.fullmatch(r"#{2,3}\s+Unmaintained transitive crates", l.strip(), re.I)
    ),
    None,
)
if start is None:
    sys.exit("SECURITY.md has no 'Unmaintained transitive crates' section")
end = next(
    (i for i in range(start + 1, len(lines)) if re.match(r"#{1,2} ", lines[i])),
    len(lines),
)
print("\n".join(lines[start:end]))
PY
)"
  for entry in "${UNMAINTAINED_EXCEPTIONS[@]}"; do
    crate="${entry%% *}"
    carrier="${entry##* }"
    printf '%s\n' "$section" | grep -Fq "\`${crate}\`"
    printf '%s\n' "$section" | grep -Fq "\`${carrier}\`"
  done
}
