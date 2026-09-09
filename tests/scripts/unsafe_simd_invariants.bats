#!/usr/bin/env bats
# Regression assertion for Issue #259 (BP-bc92894e647a) — the SIMD / unsafe /
# buffer-reuse engineering campaign left durable invariants in the PR-summary
# archive that never reached the agent instruction files. These tests pin down
# that the invariants now live in AGENTS.md (unsafe/SIMD contract, buffer-reuse
# hazard) and that the load-time index-validation memory-safety note lives in
# SECURITY.md, so a future refactor cannot silently regress them.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  AGENTS_MD="${REPO_ROOT}/AGENTS.md"
  SECURITY_MD="${REPO_ROOT}/SECURITY.md"
}

# --- AGENTS.md: Unsafe & SIMD invariants section --------------------------

@test "AGENTS.md exists" {
  [ -f "$AGENTS_MD" ]
}

@test "AGENTS.md has an Unsafe & SIMD invariants section" {
  run grep -qi 'Unsafe .* SIMD invariants' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md records the unsafe_op_in_unsafe_fn deny floor" {
  run grep -q 'unsafe_op_in_unsafe_fn' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md states already-enabled intrinsics must not be wrapped in unsafe" {
  # The "don't wrap a safe intrinsic" rule must name the lint it trips.
  run grep -q 'unused_unsafe' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md documents the SAFETY-names-the-guard convention" {
  run grep -q '// SAFETY:' "$AGENTS_MD"
  [ "$status" -eq 0 ]
  run grep -q '_feature_detected!' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md documents the buffer-reuse reset + state-leak test rule" {
  run grep -qi 'buffer reuse' "$AGENTS_MD"
  [ "$status" -eq 0 ]
  run grep -qi 'state.leak' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md names get_unchecked and its load-time validation guard" {
  run grep -q 'get_unchecked' "$AGENTS_MD"
  [ "$status" -eq 0 ]
  run grep -q 'InvalidSynapseIndex' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md draws the invariant chain as a mermaid diagram" {
  run grep -q '```mermaid' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md states that a # Safety doc covers the unsafe blocks in its body" {
  # Issue #605 - the convention the live-source sweep enforces must be the one
  # written down here, or the gate and the doc drift apart.
  run grep -q 'covers every `unsafe {` block' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md names the live-source sweep that enforces the SAFETY rule" {
  run grep -q 'tests/scripts/unsafe_block_safety_notes.bats' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

@test "AGENTS.md records that a dispatch guard must detect every enabled feature" {
  # Issue #605 - AVX2 does not imply FMA.
  run grep -qi 'AVX2 does not imply' "$AGENTS_MD"
  [ "$status" -eq 0 ]
}

# --- SECURITY.md: load-time memory-safety invariant -----------------------

@test "SECURITY.md records the load-time memory-safety invariant" {
  run grep -q 'InvalidSynapseIndex' "$SECURITY_MD"
  [ "$status" -eq 0 ]
  run grep -q 'get_unchecked' "$SECURITY_MD"
  [ "$status" -eq 0 ]
}

@test "SECURITY.md states the from_index check must never be removed" {
  run grep -qi 'from_index' "$SECURITY_MD"
  [ "$status" -eq 0 ]
  run grep -qi 'never be removed' "$SECURITY_MD"
  [ "$status" -eq 0 ]
}

# --- Doc-vs-code agreement for the cited soundness claim (Issue #609) ------
#
# The claim above was written with a line number (`network.rs:326`) that the
# code moved away from — the guard now sits elsewhere in the same file, so an
# engineer following the citation reads unrelated code and may conclude the
# check is gone. These assertions read the published doc and the source it
# describes, then require the two to agree: docs cite *symbols*, every cited
# symbol still exists, and every unchecked-read site in the crate is named.

@test "no tracked Markdown cites a bare network.rs line number" {
  # Line numbers rot; symbols do not. The PR-summary archive is a historical
  # record of what was true when each PR landed, so it is exempt.
  offenders=""
  while IFS= read -r doc; do
    case "$doc" in
      docs/archive/*) continue ;;
    esac
    match="$(grep -nE 'network\.rs:[0-9]+' "${REPO_ROOT}/${doc}" || true)"
    [ -z "$match" ] || offenders="${offenders}${doc}:${match}"$'\n'
  done < <(cd "$REPO_ROOT" && git ls-files '*.md')
  [ -z "$offenders" ] || {
    echo "docs cite a network.rs line number instead of the symbol:"
    echo "$offenders"
    false
  }
}

@test "the symbols the soundness docs cite still exist in network.rs" {
  # SECURITY.md and AGENTS.md both say CompiledNetwork::new raises
  # NetworkError::InvalidSynapseIndex. Fail if either symbol is renamed away,
  # or if the guard leaves that constructor.
  run python3 - "${REPO_ROOT}/neat-core/src/network.rs" <<'PY'
import re
import sys

src = open(sys.argv[1], encoding="utf-8").read()

if not re.search(r"\bInvalidSynapseIndex\s*\{", src):
    sys.exit("network.rs declares no InvalidSynapseIndex variant")

# Locate `pub fn new` inside `impl CompiledNetwork` and read its body by brace
# depth, so the guard must live in the constructor the docs name.
impl_start = src.find("impl CompiledNetwork {")
if impl_start < 0:
    sys.exit("network.rs has no `impl CompiledNetwork` block")
new_start = src.find("pub fn new(", impl_start)
if new_start < 0:
    sys.exit("network.rs has no CompiledNetwork::new constructor")

body_start = src.index("{", src.index(")", new_start))
depth, end = 0, None
for i in range(body_start, len(src)):
    if src[i] == "{":
        depth += 1
    elif src[i] == "}":
        depth -= 1
        if depth == 0:
            end = i
            break
if end is None:
    sys.exit("could not read the body of CompiledNetwork::new")

body = src[body_start:end]
if "NetworkError::InvalidSynapseIndex" not in body:
    sys.exit("CompiledNetwork::new no longer raises NetworkError::"
             "InvalidSynapseIndex — the docs' soundness claim is stale")
PY
  [ "$status" -eq 0 ]
}

@test "SECURITY.md names every unchecked-read site in neat-core/src" {
  # Set equality, both directions: the bullet list in the memory-safety section
  # must name exactly the files that read unchecked, so a new unchecked-read
  # site fails this gate until it is documented, and a site that stops reading
  # unchecked cannot linger in the doc.
  run python3 - "$SECURITY_MD" "${REPO_ROOT}/neat-core/src" <<'PY'
import os
import re
import sys

security, src_dir = sys.argv[1], sys.argv[2]
doc = open(security, encoding="utf-8").read()

# What the code does: every .rs file with a get_unchecked outside a comment.
sites = set()
for root, _dirs, files in os.walk(src_dir):
    for name in sorted(files):
        if not name.endswith(".rs"):
            continue
        path = os.path.join(root, name)
        with open(path, encoding="utf-8") as fh:
            # Code only — a doc comment mentioning get_unchecked is prose, not
            # an unchecked read.
            code = "\n".join(line.split("//")[0] for line in fh)
        if "get_unchecked" in code:
            sites.add(os.path.relpath(path, os.path.dirname(security)))
if not sites:
    sys.exit("no unchecked-read site found in neat-core/src — the scan is "
             "broken or the invariant no longer applies")

# What the doc claims: the bulleted site list in the memory-safety section.
section = re.search(
    r"^## Memory safety of compiled-network loading$(.*?)^## ",
    doc, re.M | re.S)
if section is None:
    sys.exit("SECURITY.md has no 'Memory safety of compiled-network loading' "
             "section")
documented = set(re.findall(r"^- `(neat-core/src/[^`]+\.rs)`",
                            section.group(1), re.M))

if documented != sites:
    sys.exit("SECURITY.md's unchecked-read site list disagrees with the code\n"
             f"  undocumented sites: {sorted(sites - documented) or 'none'}\n"
             f"  documented but not unchecked: "
             f"{sorted(documented - sites) or 'none'}")
PY
  [ "$status" -eq 0 ]
}
