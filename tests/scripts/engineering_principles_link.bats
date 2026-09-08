#!/usr/bin/env bats
# Issue #593 — NEAT-AI-core must work from the same engineering song sheet as
# the rest of the family instead of maintaining a parallel agent-only policy
# copy. The canonical policy is `docs/ENGINEERING_PRINCIPLES.md` in the
# stSoftwareAU/NEAT-AI repository (NEAT-AI#3978); this repository links to it
# and keeps only the core-specific extensions locally.
#
# These are "what" tests: they read the published documentation artefacts and
# assert on the observable outcome a reader gets — the canonical policy is
# reachable from both audiences, the core-specific migration/rollback rules are
# stated by reference, the Rust/core invariants are still here, and the shared
# policy was not copied back in.
#
# Every Python assertion reads its input from the environment inside a **quoted**
# heredoc (AGENTS.md "Oracles and mutation evidence" rule 4), so the shell can
# neither interpolate the document text nor run the backticks in it.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  AGENTS="${REPO_ROOT}/AGENTS.md"
  README="${REPO_ROOT}/README.md"
  RELEASING="${REPO_ROOT}/RELEASING.md"
  # The canonical document lives in another repository, so the link must be an
  # absolute URL on that repository's default branch.
  CANONICAL_URL="https://github.com/stSoftwareAU/NEAT-AI/blob/Develop/docs/ENGINEERING_PRINCIPLES.md"
  DEFERRAL_HEADING="## Family-wide engineering principles"
}

# section_text <file> <heading> — print the body of the section introduced by
# <heading>, stopping at the next heading of the same or a higher level. Scopes
# an assertion to the section that must carry the rule, so a stray word
# elsewhere in a 700-line file cannot satisfy it.
section_text() {
  SECTION_FILE="$1" SECTION_HEADING="$2" python3 - <<'PY'
import os, sys

path = os.environ["SECTION_FILE"]
heading = os.environ["SECTION_HEADING"]
level = len(heading) - len(heading.lstrip("#"))
out, inside = [], False
for line in open(path, encoding="utf-8"):
    stripped = line.rstrip("\n")
    if stripped.startswith("#"):
        this_level = len(stripped) - len(stripped.lstrip("#"))
        if inside and this_level <= level:
            break
        if stripped == heading:
            inside = True
            continue
    if inside:
        out.append(stripped)
if not inside:
    sys.stderr.write(f"heading not found in {path}: {heading}\n")
    sys.exit(1)
sys.stdout.write("\n".join(out))
PY
}

# assert_section_matches <file> <heading> <label=regex>... — every regex must
# match the section body (case-insensitively), or the helper names the ones that
# did not and fails.
assert_section_matches() {
  local file="$1" heading="$2"
  shift 2
  local body
  body="$(section_text "$file" "$heading")" || return 1
  SECTION_BODY="$body" SECTION_RULES="$(printf '%s\n' "$@")" python3 - <<'PY'
import os, re, sys

# Link destinations are stripped first: the canonical anchors spell the rules
# out in their slugs ("#8-rollback-is-versioning-and-pinning-not-duplicate-code"),
# so a body matched with the URLs left in would pass on the link alone — a
# vacuous oracle. Only the prose a reader sees is matched.
body = re.sub(r"\]\([^)]*\)", "]", os.environ["SECTION_BODY"].lower())
body = re.sub(r"https?://\S+", " ", body)
# Prose wraps at ~80 columns, so a phrase can straddle a newline: flatten the
# whitespace before matching, or an assertion would depend on where the line
# happened to break.
body = re.sub(r"\s+", " ", body)
missing = []
for rule in os.environ["SECTION_RULES"].splitlines():
    if not rule.strip():
        continue
    label, _, pattern = rule.partition("=")
    if not re.search(pattern, body):
        missing.append(label)
if missing:
    sys.stderr.write("section is missing: " + "; ".join(missing) + "\n")
    sys.exit(1)
PY
}

# --- The canonical document is reachable from both audiences ----------------

@test "AGENTS.md links to the canonical family-wide principles" {
  run grep -qF "$CANONICAL_URL" "$AGENTS"
  [ "$status" -eq 0 ]
}

@test "README.md links to the canonical family-wide principles" {
  run grep -qF "$CANONICAL_URL" "$README"
  [ "$status" -eq 0 ]
}

@test "the link is absolute — no in-repo path that would 404 here" {
  # `docs/ENGINEERING_PRINCIPLES.md` does not exist in this repository, so a
  # relative link to it is a broken link for every reader.
  [ ! -f "${REPO_ROOT}/docs/ENGINEERING_PRINCIPLES.md" ]
  run env AGENTS_MD="$AGENTS" README_MD="$README" RELEASING_MD="$RELEASING" python3 - <<'PY'
import os, re, sys

bad = []
for path in (os.environ["AGENTS_MD"], os.environ["README_MD"], os.environ["RELEASING_MD"]):
    text = open(path, encoding="utf-8").read()
    for target in re.findall(r"\]\(([^)]*ENGINEERING_PRINCIPLES\.md[^)]*)\)", text):
        if not target.startswith("https://github.com/stSoftwareAU/NEAT-AI/"):
            bad.append(f"{path}: {target}")
if bad:
    sys.stderr.write("relative or foreign links to the canonical doc:\n" + "\n".join(bad) + "\n")
    sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

@test "README's contributor guidance points at the canonical policy" {
  # A reader of the contributor-facing TDD section must reach the canonical
  # policy from that section, not only from somewhere else in a 1100-line file.
  local body
  body="$(section_text "$README" "## Test-driven development")"
  run grep -qF "$CANONICAL_URL" <<<"$body"
  [ "$status" -eq 0 ]
}

@test "RELEASING.md's versioning policy points at the canonical rollback rule" {
  # Rollback by re-pinning is a release decision, so a releaser must meet the
  # canonical rule in the document they are already reading.
  run assert_section_matches "$RELEASING" "## Versioning policy" \
    "the canonical principle is named=rollback is versioning and pinning" \
    "rollback is a repin of a known-good revision=re-?pin\\w*[^.]*(revision|version)" \
    "not a revived duplicate implementation=(never|not)[^.]*(revi\\w+|duplicate|second)[^.]*(implementation|fallback)"
  [ "$status" -eq 0 ]
  run grep -qF "$CANONICAL_URL" "$RELEASING"
  [ "$status" -eq 0 ]
}

# --- The core-specific reading of the shared rules is stated by reference ----

@test "AGENTS.md carries the deferral section" {
  run section_text "$AGENTS" "$DEFERRAL_HEADING"
  [ "$status" -eq 0 ]
}

@test "the migration rule names parity, ownership, deletion and no fallback" {
  run assert_section_matches "$AGENTS" "$DEFERRAL_HEADING" \
    "parity or a justified improvement proven first=parit\w*[^.]*(superior|improvement)|(superior|improvement)\w*[^.]*parit" \
    "ownership transfers to neat-core=(ownership|owner)[^.]*neat-core" \
    "the superseded implementation is deleted=delet\w+[^.]*(superseded|typescript)" \
    "no runtime fallback or dual path=no [^.]*(fallback|dual path)"
  [ "$status" -eq 0 ]
}

@test "a post-migration defect starts with the smallest reproducing test" {
  run assert_section_matches "$AGENTS" "$DEFERRAL_HEADING" \
    "the smallest reproducing test comes first=(smallest|reproduc\w+)[^.]*test" \
    "the fix lands in the canonical implementation=canonical implementation"
  [ "$status" -eq 0 ]
}

@test "rollback is documented as re-pinning a revision, not duplicate code" {
  run assert_section_matches "$AGENTS" "$DEFERRAL_HEADING" \
    "rollback is a repin of a released revision=roll ?back[^.]*(pin|version|revision)" \
    "rollback is not a duplicate implementation=(not|never)[^.]*(duplicate|second|parallel)[^.]*(implementation|path)"
  [ "$status" -eq 0 ]
}

# --- DRY: link the shared policy, do not copy it -----------------------------

@test "the deferral section stays a pointer, not a copy of the policy" {
  local body line_count
  body="$(section_text "$AGENTS" "$DEFERRAL_HEADING")"
  line_count=$(printf '%s\n' "$body" | wc -l)
  # A pointer plus the core-specific extensions; anything longer is the shared
  # policy being copied back in.
  [ "$line_count" -le 45 ]
}

@test "the family checklist and principle titles are not restated here" {
  # The canonical document owns the numbered principle list and the
  # pre-pull-request checklist; neither is duplicated into this repository.
  # (The numbered headings this file does carry — the oracle rules — are
  # core-specific and have no counterpart in the shared document.)
  run grep -qiE '^#+ .*before you open a pull request' "$AGENTS" "$README" "$RELEASING"
  [ "$status" -ne 0 ]
  run env AGENTS_MD="$AGENTS" README_MD="$README" RELEASING_MD="$RELEASING" python3 - <<'PY'
import os, re, sys

# Titles owned by the canonical document; a local heading repeating one of them
# is the shared policy being copied back in.
CANONICAL_TITLES = [
    "test-driven development (tdd) comes first",
    "a post-release defect starts with the smallest reproducing test",
    "tests describe behaviour, not implementation",
    "one implementation owner per capability",
    "do not repeat yourself (dry) across the family",
    "migrate typescript",
    "no fallback, no shadow implementation",
    "rollback is versioning and pinning",
    "migrations are small, independently reviewable",
    "shared logic belongs in the lowest sensible reusable component",
    "public libraries stay application-agnostic",
]
copied = []
for path in (os.environ["AGENTS_MD"], os.environ["README_MD"], os.environ["RELEASING_MD"]):
    for line in open(path, encoding="utf-8"):
        if not line.startswith("#"):
            continue
        heading = re.sub(r"^#+\s*(?:\d+\.\s*)?", "", line).strip().lower()
        if any(heading.startswith(title) for title in CANONICAL_TITLES):
            copied.append(f"{path}: {line.strip()}")
if copied:
    sys.stderr.write("canonical principle restated locally:\n" + "\n".join(copied) + "\n")
    sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

# --- Core-specific invariants stay locally discoverable ----------------------

@test "the Rust/core invariants remain in AGENTS.md" {
  for heading in \
    "## Oracles and mutation evidence" \
    "## Build profiles (Issue #546)" \
    "## Ownership fence (Issue #544)" \
    "## Unsafe & SIMD invariants" \
    "## CI / secrets"; do
    run grep -qF "$heading" "$AGENTS"
    [ "$status" -eq 0 ]
  done
}

@test "the core TDD mechanics (cargo test, quality.sh) stay local" {
  run assert_section_matches "$AGENTS" "## TDD (required)" \
    "the workspace test command=cargo test --workspace" \
    "the local quality gate=\./quality\.sh"
  [ "$status" -eq 0 ]
}
