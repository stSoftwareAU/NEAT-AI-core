#!/usr/bin/env bats
# Tests for CODEOWNERS coverage of privileged CI paths (Issue #208).
#
# Rationale: this repo ships privileged workflows — `id-token: write` for
# keyless Sigstore signing (wasm-bundle.yml) and non-GITHUB_TOKEN secrets
# ACTIONS_PUSH (ci.yml, upgrade-dependencies.yml) and SEMGREP_APP_TOKEN
# (semgrep.yml). Without a CODEOWNERS rule covering `.github/workflows/` a
# pull request can quietly edit one of those workflows and, once required
# code-owner review is enabled on the branch, merge on a single
# self-approval — the exact path used to exfiltrate secrets or mint a
# signed artefact. A CODEOWNERS rule forces review from a designated owner.
#
# These are "what" tests: they parse the CODEOWNERS file and assert on the
# observable outcome (which paths are covered and that every rule names an
# owner), not on source-text heuristics.

setup() {
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  # GitHub recognises CODEOWNERS in exactly these three locations.
  CODEOWNERS=""
  for candidate in \
    "${REPO_ROOT}/CODEOWNERS" \
    "${REPO_ROOT}/.github/CODEOWNERS" \
    "${REPO_ROOT}/docs/CODEOWNERS"; do
    if [ -f "$candidate" ]; then
      CODEOWNERS="$candidate"
      break
    fi
  done
}

@test "a CODEOWNERS file exists in a GitHub-recognised location" {
  [ -n "$CODEOWNERS" ]
}

@test "CODEOWNERS covers .github/workflows/ with an owner" {
  [ -n "$CODEOWNERS" ]
  run python3 - "$CODEOWNERS" <<'PY'
import re, sys

path = sys.argv[1]
# A CODEOWNERS rule for the workflows dir. Accept the leading-slash anchored
# form (/.github/workflows/) or the unanchored form (.github/workflows/).
pat = re.compile(r"^/?\.github/workflows/\s+\S")
owner = re.compile(r"(@[\w.-]+(?:/[\w.-]+)?|[^@\s]+@[^@\s]+\.[^@\s]+)")

for line in open(path, encoding="utf-8"):
    stripped = line.strip()
    if not stripped or stripped.startswith("#"):
        continue
    if pat.match(stripped):
        # The rule must name at least one owner beyond the path token.
        owners = owner.findall(stripped.split(None, 1)[1])
        assert owners, f"workflows rule has no owner: {stripped!r}"
        sys.exit(0)

sys.stderr.write("no CODEOWNERS rule covers .github/workflows/\n")
sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

@test "CODEOWNERS covers .github/actions/ with an owner" {
  [ -n "$CODEOWNERS" ]
  run python3 - "$CODEOWNERS" <<'PY'
import re, sys

path = sys.argv[1]
pat = re.compile(r"^/?\.github/actions/\s+\S")
owner = re.compile(r"(@[\w.-]+(?:/[\w.-]+)?|[^@\s]+@[^@\s]+\.[^@\s]+)")

for line in open(path, encoding="utf-8"):
    stripped = line.strip()
    if not stripped or stripped.startswith("#"):
        continue
    if pat.match(stripped):
        owners = owner.findall(stripped.split(None, 1)[1])
        assert owners, f"actions rule has no owner: {stripped!r}"
        sys.exit(0)

sys.stderr.write("no CODEOWNERS rule covers .github/actions/\n")
sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

@test "every CODEOWNERS rule names at least one owner" {
  [ -n "$CODEOWNERS" ]
  run python3 - "$CODEOWNERS" <<'PY'
import re, sys

path = sys.argv[1]
owner = re.compile(r"(@[\w.-]+(?:/[\w.-]+)?|[^@\s]+@[^@\s]+\.[^@\s]+)")

failures = []
for lineno, line in enumerate(open(path, encoding="utf-8"), 1):
    stripped = line.strip()
    if not stripped or stripped.startswith("#"):
        continue
    parts = stripped.split(None, 1)
    if len(parts) < 2 or not owner.findall(parts[1]):
        failures.append(f"{lineno}: rule without owner: {stripped!r}")

if failures:
    sys.stderr.write("Ownerless CODEOWNERS rules:\n  " + "\n  ".join(failures) + "\n")
    sys.exit(1)
PY
  [ "$status" -eq 0 ]
}

@test "owners are concrete accounts, not the unresolved placeholder team" {
  # The filed issue suggested @stSoftwareAU/maintainers, but that team does
  # not exist in the org, so a CODEOWNERS entry referencing it would be
  # invalid and silently unenforced. Guard against that regression.
  [ -n "$CODEOWNERS" ]
  run grep -q "@stSoftwareAU/maintainers" "$CODEOWNERS"
  [ "$status" -ne 0 ]
}
