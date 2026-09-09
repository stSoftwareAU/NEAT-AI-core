#!/usr/bin/env bats
# Tests for the vendored YAML subset parser and its wiring (Issue #642).
#
# Contract under test: the bats suite parses workflow YAML with
# `python3 … import yaml`, and a host whose python3 has no PyYAML — the
# unattended worker container — failed 111 of 507 tests with
# ModuleNotFoundError rather than running them, so ./quality.sh never reached
# its TypeScript, Mermaid, Deno or Rust stages. `helpers.bash` now puts
# `lib/yaml_fallback` on PYTHONPATH when, and only when, importing PyYAML
# fails, so those assertions are still *made* instead of skipped.
#
# These are "what" tests: they parse YAML and assert on the parsed values, and
# on the observable behaviour of the helpers that consume them. The oracle for
# the parser is PyYAML itself — an independent implementation, not a second
# copy of this one — swept over every YAML file the repository ships.

setup() {
  load helpers
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  export FALLBACK_DIR="${BATS_TEST_DIRNAME}/lib/yaml_fallback"
  # A directory that makes `import yaml` fail exactly as a host without PyYAML
  # does, so the missing-PyYAML path is reachable on a host that has it.
  export BLOCKER="${BATS_TEST_TMPDIR}/no-pyyaml"
  mkdir -p "$BLOCKER"
  printf 'raise ImportError("No module named %s")\n' "'yaml'" >"${BLOCKER}/yaml.py"
  FIXTURES="${BATS_TEST_TMPDIR}/fixtures"
  mkdir -p "$FIXTURES"
}

# Run python3 with the vendored parser shadowing PyYAML, so `import yaml` in
# the caller's heredoc is the live module under test rather than a copy of it.
vendored_python() {
  PYTHONPATH="${FALLBACK_DIR}${PYTHONPATH:+:${PYTHONPATH}}" python3 -
}

# Drop the vendored parser from PYTHONPATH and skip unless a real PyYAML is
# then importable. Two tests below need the genuine article: the oracle sweep,
# which would be vacuous if it compared the parser with itself, and the
# PyYAML-in-charge test, whose assertion only holds where PyYAML exists.
use_real_pyyaml_only() {
  local scrubbed="" entry entries=()
  IFS=':' read -r -a entries <<<"${PYTHONPATH:-}"
  for entry in ${entries[@]+"${entries[@]}"}; do
    [ -z "$entry" ] && continue
    [ "$entry" = "$FALLBACK_DIR" ] && continue
    scrubbed="${scrubbed:+${scrubbed}:}${entry}"
  done
  export PYTHONPATH="$scrubbed"
  python3 - <<'PY' || skip "PyYAML is not installed on this host"
import os
import sys

try:
    import yaml
except ImportError:
    sys.exit(1)

vendored = os.path.join(os.environ["FALLBACK_DIR"], "yaml.py")
sys.exit(1 if os.path.realpath(yaml.__file__) == os.path.realpath(vendored) else 0)
PY
}

# A minimal workflow with the shapes the helpers read: a permissions block, a
# step whose run: body is a literal block scalar, and a branches filter.
write_workflow() {
  cat >"${FIXTURES}/wf.yml" <<'YAML'
name: fixture
on:
  pull_request:
    branches: ["Develop", "milestone/**"]
permissions:
  contents: read
jobs:
  gate:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      pull-requests: write
    steps:
      - name: Say hello
        run: |
          set -euo pipefail
          echo "hello # not a comment"
YAML
}

# --- the reported failure, and its fix ---------------------------------------

@test "a YAML helper fails loudly when neither PyYAML nor the fallback is importable" {
  require_python3
  write_workflow
  PYTHONPATH="$BLOCKER" run assert_job_least_privilege "${FIXTURES}/wf.yml" gate pull-requests
  [ "$status" -ne 0 ]
  [[ "$output" == *"yaml"* ]]
}

@test "wire_yaml_fallback restores YAML parsing when PyYAML is missing" {
  require_python3
  write_workflow
  export PYTHONPATH="$BLOCKER"
  wire_yaml_fallback

  run assert_job_least_privilege "${FIXTURES}/wf.yml" gate pull-requests
  [ "$status" -eq 0 ]
  run assert_job_least_privilege "${FIXTURES}/wf.yml" gate
  [ "$status" -ne 0 ]

  run assert_pr_branch_filter_matches "${FIXTURES}/wf.yml" Develop milestone/some-slug
  [ "$status" -eq 0 ]

  extract_step "${FIXTURES}/wf.yml" "Say hello" "$FIXTURES"
  [ "$(cat "${FIXTURES}/step.sh")" = 'set -euo pipefail
echo "hello # not a comment"' ]
  [ "$(cat "${FIXTURES}/shell.cmd")" = "bash -e" ]
}

@test "wire_yaml_fallback names the parser it fell back to" {
  require_python3
  export PYTHONPATH="$BLOCKER"
  run wire_yaml_fallback
  [ "$status" -eq 0 ]
  [[ "$output" == *"PyYAML is not installed"* ]]
  [[ "$output" == *"${FALLBACK_DIR}"* ]]
}

@test "wire_yaml_fallback leaves PyYAML in charge where it is installed" {
  require_python3
  use_real_pyyaml_only
  export PYTHONPATH=""
  wire_yaml_fallback
  [ -z "$PYTHONPATH" ]
  [ "$(python3 -c 'import yaml, sys; sys.stdout.write(yaml.__file__)')" != "${FALLBACK_DIR}/yaml.py" ]
}

# --- the parser, against an independent oracle -------------------------------

@test "the vendored parser and PyYAML agree on every YAML file in the repository" {
  require_python3
  use_real_pyyaml_only
  REPO_ROOT="$REPO_ROOT" FALLBACK_DIR="$FALLBACK_DIR" python3 - <<'PY'
import importlib.util
import os
import pathlib
import sys

import yaml as pyyaml

vendored = os.path.join(os.environ["FALLBACK_DIR"], "yaml.py")
# Rule 1 of AGENTS.md: an oracle must not be the code under test. Were PyYAML
# to resolve to the vendored parser, every comparison below would be trivially
# equal — fail loud rather than pass vacuously.
assert os.path.realpath(pyyaml.__file__) != os.path.realpath(vendored), (
    "the oracle resolved to the parser under test: %s" % pyyaml.__file__)

spec = importlib.util.spec_from_file_location("yaml_fallback", vendored)
fallback = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fallback)

root = pathlib.Path(os.environ["REPO_ROOT"], ".github")
files = sorted(p for p in root.rglob("*.yml"))
assert len(files) >= 10, f"expected the workflow set, found {files}"


def typed(node):
    """Compare values *and* their types — `20` as a string would slip past ==."""
    if isinstance(node, dict):
        return {"map": sorted((repr(k), typed(v)) for k, v in node.items())}
    if isinstance(node, list):
        return [typed(v) for v in node]
    return (type(node).__name__, node)


for path in files:
    text = path.read_text(encoding="utf-8")
    expected = typed(pyyaml.safe_load(text))
    actual = typed(fallback.safe_load(text))
    assert expected == actual, f"{path} parses differently:\n{expected}\n{actual}"
PY
}

# --- the parser, construct by construct --------------------------------------

@test "PYTHONPATH makes the vendored parser the module an import of yaml resolves to" {
  require_python3
  run vendored_python <<'PY'
import os
import sys

import yaml

sys.stdout.write(yaml.__file__)
PY
  [ "$status" -eq 0 ]
  [ "$output" = "${FALLBACK_DIR}/yaml.py" ]
}

@test "literal block scalars keep their lines, blank lines and hashes" {
  require_python3
  vendored_python <<'PY'
import yaml

data = yaml.safe_load(
    "clip: |\n"
    "  one\n"
    "\n"
    "  # two is content, not a comment\n"
    "strip: |-\n"
    "  no trailing newline\n"
    "keep: |+\n"
    "  body\n"
    "\n"
    "after: 1\n"
)
assert data["clip"] == "one\n\n# two is content, not a comment\n", repr(data["clip"])
assert data["strip"] == "no trailing newline", repr(data["strip"])
assert data["keep"] == "body\n\n", repr(data["keep"])
assert data["after"] == 1, repr(data["after"])
PY
}

@test "folded block scalars join plain lines with a space and keep blank-line breaks" {
  require_python3
  vendored_python <<'PY'
import yaml

data = yaml.safe_load(
    "description: >-\n"
    "  one\n"
    "  two\n"
    "\n"
    "  four\n"
    "next: 2\n"
)
assert data["description"] == "one two\nfour", repr(data["description"])
assert data["next"] == 2, repr(data["next"])
PY
}

@test "plain scalars resolve to the YAML 1.1 types PyYAML produces" {
  require_python3
  vendored_python <<'PY'
import yaml

data = yaml.safe_load(
    "on:\n"
    "  push: ~\n"
    "timeout-minutes: 20 # a trailing comment\n"
    "persist-credentials: false\n"
    "enabled: On\n"
    "version: 1.170.1\n"
    "ratio: 3.5\n"
    "mask: 0x1f\n"
    "digest: sha256:98c2572f\n"
    "missing:\n"
)
# PyYAML resolves the bare `on` key to the boolean True — the helpers depend on
# that, so the fallback must not "helpfully" keep it a string.
assert True in data and "on" not in data, sorted(map(repr, data))
assert data[True] == {"push": None}, repr(data[True])
assert data["timeout-minutes"] == 20 and isinstance(data["timeout-minutes"], int)
assert data["persist-credentials"] is False
assert data["enabled"] is True
assert data["version"] == "1.170.1", repr(data["version"])
assert data["ratio"] == 3.5 and isinstance(data["ratio"], float)
assert data["mask"] == 31, repr(data["mask"])
assert data["digest"] == "sha256:98c2572f", repr(data["digest"])
assert data["missing"] is None
PY
}

@test "quoted scalars keep hashes, honour escapes, and drop a trailing comment" {
  require_python3
  vendored_python <<'PY'
import yaml

data = yaml.safe_load(
    "hash: \"keeps # this\" # but not this\n"
    "single: 'it''s here'\n"
    "escaped: \"a\\tb\\nc\"\n"
    "plainhash: keeps#this\n"
    "expression: ${{ github.workflow }}-${{ github.ref }}\n"
)
assert data["hash"] == "keeps # this", repr(data["hash"])
assert data["single"] == "it's here", repr(data["single"])
assert data["escaped"] == "a\tb\nc", repr(data["escaped"])
assert data["plainhash"] == "keeps#this", repr(data["plainhash"])
assert data["expression"] == "${{ github.workflow }}-${{ github.ref }}"
PY
}

@test "flow collections, nested sequences and mappings parse" {
  require_python3
  vendored_python <<'PY'
import yaml

data = yaml.safe_load(
    "branches: [\"*\", \"milestone/*\"]\n"
    "needs: [version-increment, auto-format]\n"
    "with: {}\n"
    "empty: []\n"
    "matrix: {os: ubuntu-latest, jobs: 2}\n"
    "steps:\n"
    "  - uses: actions/checkout@abc\n"
    "    with:\n"
    "      persist-credentials: false\n"
    "  - name: run it\n"
    "    run: echo hi\n"
    "sequence-at-key-indent:\n"
    "- one\n"
    "- two\n"
)
assert data["branches"] == ["*", "milestone/*"]
assert data["needs"] == ["version-increment", "auto-format"]
assert data["with"] == {} and data["empty"] == []
assert data["matrix"] == {"os": "ubuntu-latest", "jobs": 2}
assert data["steps"][0]["with"]["persist-credentials"] is False
assert data["steps"][1] == {"name": "run it", "run": "echo hi"}
assert data["sequence-at-key-indent"] == ["one", "two"]
PY
}

@test "input outside the supported subset is rejected, never guessed at" {
  require_python3
  vendored_python <<'PY'
import yaml

BAD = {
    "anchor": "base: &defaults\n  a: 1\nuse: *defaults\n",
    "tag": "value: !!binary aGk=\n",
    "tabs": "jobs:\n\tgate: 1\n",
    "indent": "a: 1\n    b: 2\n",
    "unterminated": 'name: "unclosed\n',
    "not-a-mapping": "a: 1\njust text\n",
    "two-documents": "a: 1\n---\nb: 2\n",
}
for name, text in BAD.items():
    try:
        parsed = yaml.safe_load(text)
    except yaml.YAMLError:
        continue
    raise AssertionError(f"{name} was accepted, parsing to {parsed!r}")
PY
}

@test "safe_dump keeps a multi-line run body searchable" {
  require_python3
  vendored_python <<'PY'
import re

import yaml

step = {
    "name": "Push",
    "env": {"TOKEN": "${{ secrets.ACTIONS_PUSH }}"},
    "run": 'set -euo pipefail\ngit push "https://x-access-token:${TOKEN}@github.com/o/r"\n',
}
blob = yaml.safe_dump(step, default_flow_style=False)
# `assert_just_in_time_push_credential` searches the dump of a step for the
# secret expression, so it must survive serialisation unwrapped and unescaped.
assert re.search(r"\$\{\{[^}]*\bsecrets\.ACTIONS_PUSH\b", blob), blob
assert "git push \"https://" in blob, blob
assert yaml.safe_load(blob)["run"] == step["run"], blob
PY
}
