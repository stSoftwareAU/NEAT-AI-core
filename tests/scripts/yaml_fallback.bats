#!/usr/bin/env bats
# Tests for the vendored YAML subset parser and its wiring (Issue #642).
#
# Contract under test: the bats suite parses workflow YAML with
# `python3 … import yaml`, and a host whose python3 has no PyYAML — the
# unattended worker container — failed 121 of 535 tests with
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
  if python3 - <<'PY'
import os
import sys

try:
    import yaml
except ImportError:
    sys.exit(1)

vendored = os.path.join(os.environ["FALLBACK_DIR"], "yaml.py")
sys.exit(1 if os.path.realpath(yaml.__file__) == os.path.realpath(vendored) else 0)
PY
  then
    return 0
  fi
  # On CI the oracle is not optional: without PyYAML every workflow-contract
  # assertion would run on an unvalidated parser and the sweep would vanish
  # into a green skip. Fail there; skip only on a developer host.
  if [ -n "${CI:-}" ]; then
    echo "PyYAML must be installed on CI — it is the oracle for the vendored" \
      "parser, and skipping it would leave the parser unvalidated" >&2
    return 1
  fi
  skip "PyYAML is not installed on this host"
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
  # The reported symptom exactly: the parser is missing, not some other fault.
  [[ "$output" == *"No module named 'yaml'"* ]]
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
  [[ "$output" == *"PyYAML did not import"* ]]
  [[ "$output" == *"${FALLBACK_DIR}"* ]]
}

@test "the fallback notice reaches the bats output stream once per run" {
  # Every test file wires the parser afresh, so the notice goes to stderr each
  # time but to bats' always-shown stream (fd 3) only once — visible without
  # being 500 lines of noise.
  export BATS_RUN_TMPDIR="${BATS_TEST_TMPDIR}/run"
  mkdir -p "$BATS_RUN_TMPDIR"
  local shown="${BATS_TEST_TMPDIR}/fd3.log"
  yaml_fallback_notice "first" 3>"$shown"
  yaml_fallback_notice "second" 3>>"$shown"
  [ "$(cat "$shown")" = "first" ]

  run yaml_fallback_notice "third"
  [ "$status" -eq 0 ]
  [ "$output" = "third" ]
}

@test "wire_yaml_fallback leaves PyYAML in charge where it is installed" {
  require_python3
  use_real_pyyaml_only
  export PYTHONPATH=""
  wire_yaml_fallback
  [ -z "$PYTHONPATH" ]
  [ "$(python3 -c 'import yaml, sys; sys.stdout.write(yaml.__file__)')" != "${FALLBACK_DIR}/yaml.py" ]
}

@test "helpers.bash aborts when neither PyYAML nor the vendored parser imports" {
  require_python3
  # A copy of the live helpers.bash beside a vendored parser that cannot be
  # imported: the one state in which the suite must refuse to run at all
  # rather than let YAML gates pass with no parser behind them.
  broken="${BATS_TEST_TMPDIR}/broken"
  mkdir -p "${broken}/lib/yaml_fallback"
  cp "${BATS_TEST_DIRNAME}/helpers.bash" "${broken}/helpers.bash"
  printf 'raise ImportError("vendored parser is broken")\n' \
    >"${broken}/lib/yaml_fallback/yaml.py"

  PYTHONPATH="$BLOCKER" run bash -c 'source "$1"' _ "${broken}/helpers.bash"
  [ "$status" -ne 0 ]
  [[ "$output" == *"no YAML parser"* ]]
  [[ "$output" == *"vendored parser is broken"* ]]

  # The same copy, with the real parser beside it, sources cleanly.
  cp "${FALLBACK_DIR}/yaml.py" "${broken}/lib/yaml_fallback/yaml.py"
  PYTHONPATH="$BLOCKER" run bash -c 'source "$1"' _ "${broken}/helpers.bash"
  [ "$status" -eq 0 ]
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

root = pathlib.Path(os.environ["REPO_ROOT"])
skipped = ("target", ".git", "node_modules")
files = sorted(
    path
    for pattern in ("*.yml", "*.yaml")
    for path in root.rglob(pattern)
    if not any(part in skipped for part in path.parts)
)
assert len(files) >= 10, f"expected at least the workflow set, found {files}"


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

@test "the vendored parser matches PyYAML across the constructs workflow YAML uses" {
  require_python3
  use_real_pyyaml_only
  FALLBACK_DIR="$FALLBACK_DIR" python3 - <<'PY'
import importlib.util
import math
import os

import yaml as pyyaml

vendored = os.path.join(os.environ["FALLBACK_DIR"], "yaml.py")
assert os.path.realpath(pyyaml.__file__) != os.path.realpath(vendored), (
    "the oracle resolved to the parser under test: %s" % pyyaml.__file__)
spec = importlib.util.spec_from_file_location("yaml_fallback", vendored)
fallback = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fallback)

# The repository's own workflows exercise only part of the subset, so these
# documents carry the constructs a workflow may grow tomorrow. Each is judged
# against PyYAML, never against a hand-written expectation.
AGREE = {
    "flow mapping as a sequence item": "include:\n  - {os: ubuntu-latest, rust: stable}\n",
    "flow sequence as a sequence item": "a:\n  - [1, 2]\n",
    "tab inside a block scalar body": (
        "steps:\n  - run: |\n      cat <<'EOF'\n      \tindented with a tab\n      EOF\n"),
    "trailing comment containing a colon": "a:\n  - hello # see: docs\n",
    "trailing comment containing a colon, on a value": "a: hello # see: docs\n",
    "bare dash is a null entry": "a:\n  -\n  - x\n",
    "bare dash before a mapping entry": "a:\n  -\n  - x: 1\n",
    "not-a-number": "a: .nan\n",
    "infinities": "a: .inf\nb: -.Inf\n",
    "whitespace-only line inside a literal block": "key: |\n  a\n     \n  b\n",
    "folded block, blank line before a more-indented line": "key: >\n  a\n\n   b\n",
    "folded block, a more-indented run": "key: >\n  a\n   b\n  c\n",
    "colon inside a flow scalar": "a: [x:y]\n",
    "digest pin inside a flow mapping": "a: {image: node@sha256:abc}\n",
    "nested block sequence": "a:\n  - - x\n    - y\n",
    "nested block sequence at the document root": "- - a\n- b\n",
    "byte-order mark": "\ufeffa: 1\n",
    "quoted sequence entries": "a:\n  - \"x: y\"\n  - 'z'\n",
    "empty flow collections as entries": "a:\n  - {}\n  - []\n",
}

# PyYAML refuses these. Accepting them would let a workflow that fails the gate
# on CI pass on a host running the vendored parser.
BOTH_REJECT = {
    "block indent smaller on a later line": "key: |\n    a\n  b\n",
    "folded indent smaller on a later line": "key: >\n   a\n  b\n",
    "a second colon in a plain scalar": "run: echo foo: bar\n",
    "an expression followed by a colon": "name: ${{ github.workflow }}: build\n",
    "a trailing colon": "a: b:\n",
    "a dash as a value": "a: -\n",
    "a sequence opened on the key's line": "a: - x\n",
    "a tab between key and value": "a:\tb\n",
    "tab indentation": "jobs:\n\tgate: 1\n",
    "an unterminated quote": 'name: "unclosed\n',
    "stray text after a mapping": "a: 1\njust text\n",
    "an over-indented mapping entry": "a: 1\n    b: 2\n",
    "two documents": "a: 1\n---\nb: 2\n",
}

# PyYAML parses these; the subset deliberately refuses them rather than
# returning a value that differs from what CI would see.
REFUSED = {
    "anchors and aliases": "base: &d\n  a: 1\nuse: *d\n",
    "tags": "value: !!binary aGk=\n",
    "a date": "d: 2024-01-15\n",
    "a timestamp": "d: 2024-01-15 10:20:30\n",
    "a flow collection spanning lines": "a: [\n  x,\n  y,\n]\n",
}


def same(left, right):
    if isinstance(left, float) and isinstance(right, float):
        return (math.isnan(left) and math.isnan(right)) or left == right
    if type(left) is not type(right):
        return False
    if isinstance(left, dict):
        return len(left) == len(right) and all(
            key in right and same(value, right[key]) for key, value in left.items())
    if isinstance(left, list):
        return len(left) == len(right) and all(
            same(a, b) for a, b in zip(left, right))
    return left == right


for name, text in AGREE.items():
    expected = pyyaml.safe_load(text)          # fails loud if the case is bogus
    actual = fallback.safe_load(text)
    assert same(expected, actual), f"{name}: {expected!r} != {actual!r}"

for name, text in BOTH_REJECT.items():
    try:
        parsed = pyyaml.safe_load(text)
    except Exception:
        pass
    else:
        raise AssertionError(f"{name}: PyYAML accepts it, parsing to {parsed!r}")
    try:
        parsed = fallback.safe_load(text)
    except fallback.YAMLError:
        continue
    raise AssertionError(f"{name}: accepted, parsing to {parsed!r}")

for name, text in REFUSED.items():
    pyyaml.safe_load(text)                     # fails loud if the case is bogus
    try:
        parsed = fallback.safe_load(text)
    except fallback.YAMLError:
        continue
    raise AssertionError(f"{name}: silently accepted, parsing to {parsed!r}")
PY
}

# --- the parser, construct by construct --------------------------------------

@test "PYTHONPATH makes the vendored parser the module an import of yaml resolves to" {
  require_python3
  run vendored_python <<'PY'
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
