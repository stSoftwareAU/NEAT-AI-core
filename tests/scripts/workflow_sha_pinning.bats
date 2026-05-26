#!/usr/bin/env bats
# Tests for SHA-pinning of GitHub Actions across every workflow (Issue #77).
#
# Rationale: floating tags such as `@v4`, `@v2`, or branch refs like `@stable`
# are mutable. A compromised maintainer account or supply-chain attack on the
# action's release pipeline can re-point them at malicious code, which would
# then execute with our workflow secrets (ACTIONS_PUSH, GITHUB_TOKEN,
# GITLEAKS_LICENSE, SEMGREP_APP_TOKEN). Pinning every `uses:` to a 40-char
# commit SHA — the pattern already used in markdown-lint.yml — removes that
# attack surface.
#
# These are "what" tests: they parse the YAML and assert on observable
# outcomes (the `uses:` ref shape), not on source-text heuristics.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  WORKFLOWS_DIR="${REPO_ROOT}/.github/workflows"
}

# Helper: assert every uses: in every workflow under WORKFLOWS_DIR is pinned
# to a 40-char hex SHA. Local reusable workflows (./.github/workflows/*.yml)
# are exempt because they live in this repo.
@test "every action in every workflow is pinned to a 40-char commit SHA" {
  if ! command -v python3 &>/dev/null; then
    skip "python3 required for YAML parsing"
  fi
  run python3 - <<PY
import glob, os, re, sys, yaml

workflows_dir = "$WORKFLOWS_DIR"
sha_re = re.compile(r"^[A-Za-z0-9_.\-/]+@[0-9a-f]{40}$")
local_re = re.compile(r"^\./")

failures = []
files = sorted(glob.glob(os.path.join(workflows_dir, "*.yml")))
assert files, f"no workflow files found in {workflows_dir}"

for path in files:
    with open(path) as fh:
        data = yaml.safe_load(fh)
    jobs = data.get("jobs", {}) or {}
    for job_name, job in jobs.items():
        # Reusable workflow call (uses at the job level) — only flag if external.
        job_uses = job.get("uses") if isinstance(job, dict) else None
        if job_uses and not local_re.match(job_uses):
            if not sha_re.match(job_uses):
                failures.append(f"{os.path.basename(path)} job={job_name} uses={job_uses}")
        for step in (job.get("steps", []) or []) if isinstance(job, dict) else []:
            uses = step.get("uses")
            if uses is None:
                continue
            if local_re.match(uses):
                continue
            if not sha_re.match(uses):
                failures.append(f"{os.path.basename(path)} job={job_name} uses={uses}")

if failures:
    sys.stderr.write("Unpinned actions:\n  " + "\n  ".join(failures) + "\n")
    sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

# Every external uses: line should be followed (in the raw YAML) by an
# adjacent human-readable version comment so Dependabot / reviewers can see
# which release the SHA corresponds to. Pattern matches markdown-lint.yml.
@test "every SHA-pinned action has a version comment alongside it" {
  run python3 - <<PY
import glob, os, re, sys

workflows_dir = "$WORKFLOWS_DIR"
uses_re = re.compile(r"^(\s*)(?:-\s*)?uses:\s*([^\s#]+)(?:\s*#\s*(.+))?\s*$")
sha_at_re = re.compile(r"@[0-9a-f]{40}$")

failures = []
for path in sorted(glob.glob(os.path.join(workflows_dir, "*.yml"))):
    with open(path) as fh:
        lines = fh.readlines()
    for i, line in enumerate(lines):
        m = uses_re.match(line)
        if not m:
            continue
        ref = m.group(2)
        inline_comment = m.group(3)
        if ref.startswith("./"):
            continue
        if not sha_at_re.search(ref):
            continue
        # Need a version annotation: either inline trailing comment on the
        # uses: line, or a # comment somewhere within the current step block
        # (scan backwards until we hit another step boundary or the job).
        if inline_comment:
            continue
        found = False
        for j in range(i - 1, max(-1, i - 6), -1):
            prev_raw = lines[j]
            prev = prev_raw.strip()
            if prev.startswith("#"):
                found = True
                break
            # Step boundary: a bare "-" item (new step) two or more lines back
            # OR a job-level key. Stop scanning.
            if j < i - 1 and prev.lstrip().startswith("- "):
                break
            if prev == "":
                break
        if not found:
            failures.append(f"{os.path.basename(path)}:{i + 1}: missing version comment for {ref}")

if failures:
    sys.stderr.write("Missing version comments:\n  " + "\n  ".join(failures) + "\n")
    sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

# Issue #97 regression: actions/checkout pins must not point at SHAs whose
# runtime is the deprecated Node.js 20 (EOL 2026-09-16). Maintain a denylist
# of known-deprecated commits and assert no workflow uses them. New entries
# go here whenever a Node-runtime EOL is announced.
@test "no workflow uses actions/checkout pinned to a deprecated Node runtime" {
  run python3 - <<PY
import glob, os, re, sys

workflows_dir = "$WORKFLOWS_DIR"
# SHA -> reason. Add deprecated runtime SHAs here as Node versions reach EOL.
DEPRECATED = {
    # actions/checkout@v4.3.1 — runs on Node 20 (EOL on GitHub-hosted
    # runners 2026-09-16). Bumped to v6.0.2 (Node 24) for Issue #97.
    "34e114876b0b11c390a56381ad16ebd13914f8d5": "actions/checkout@v4.3.1 (node20, EOL 2026-09-16)",
}
uses_re = re.compile(r"uses:\s*([^\s#]+)")

failures = []
for path in sorted(glob.glob(os.path.join(workflows_dir, "*.yml"))):
    with open(path) as fh:
        for lineno, line in enumerate(fh, 1):
            m = uses_re.search(line)
            if not m:
                continue
            ref = m.group(1)
            if "@" not in ref:
                continue
            sha = ref.rsplit("@", 1)[1]
            if sha in DEPRECATED:
                failures.append(
                    f"{os.path.basename(path)}:{lineno}: {ref} -> {DEPRECATED[sha]}"
                )

if failures:
    sys.stderr.write("Deprecated-runtime action SHAs in use:\n  " + "\n  ".join(failures) + "\n")
    sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

# Behavioural sanity check: the regex actually rejects a known-bad ref. If
# someone weakens the regex this test catches it.
@test "SHA-pin regex rejects floating tags and branch refs" {
  run python3 - <<PY
import re
sha_re = re.compile(r"^[A-Za-z0-9_.\-/]+@[0-9a-f]{40}$")
bad = [
    "actions/checkout@v4",
    "actions/checkout@v4.1.1",
    "dtolnay/rust-toolchain@stable",
    "taiki-e/install-action@v2",
    "Swatinem/rust-cache@v2",
]
for ref in bad:
    assert not sha_re.match(ref), f"regex incorrectly accepted {ref}"

good = [
    "actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd",
    "denoland/setup-deno@667a34cdef165d8d2b2e98dde39547c9daac7282",
]
for ref in good:
    assert sha_re.match(ref), f"regex incorrectly rejected {ref}"
PY
  [ "$status" -eq 0 ]
}
