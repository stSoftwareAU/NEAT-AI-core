#!/usr/bin/env bats
# Tests for the Deno module cache behind every `denoland/setup-deno` step
# (Issue #737).
#
# `denoland/setup-deno` installs the runtime only — it has no cache input — so
# every job re-downloaded its whole JSR/npm module graph on every run. Each job
# that installs Deno must follow it with an `actions/cache` step over the
# default `DENO_DIR` (`~/.cache/deno`), keyed exactly on `deno.lock` so a
# dependency change is a new cache entry, with a `restore-keys` prefix so a
# lockfile bump still starts from a warm seed instead of cold.
#
# These are "what" tests: the workflows are parsed as YAML and the step order
# inside each job is asserted. The checker has exactly one definition
# (`DENO_CACHE_CHECKER`, AGENTS.md oracle rule 4): the sweep over the real
# workflows and the synthetic good/bad literals both run it, so weakening the
# checker turns the literal tests red too.

load helpers

setup() {
  require_python3
  REPO_ROOT="${BATS_TEST_DIRNAME}/../.."
  export WORKFLOWS_DIR="${REPO_ROOT}/.github/workflows"
  # Usage: python3 -c "$DENO_CACHE_CHECKER" <workflow.yml>...
  # Prints `jobs=<n>` (jobs installing Deno) and one line per violation; exits
  # 1 on any violation, 2 when no job installs Deno at all (a vacuous sweep).
  export DENO_CACHE_CHECKER='
import os, sys, yaml

def is_deno_cache(step):
    uses = str(step.get("uses") or "")
    if not uses.startswith("actions/cache@"):
        return False
    w = step.get("with") or {}
    paths = [p.strip() for p in str(w.get("path") or "").splitlines()]
    return ("~/.cache/deno" in paths
            and "hashFiles(\x27deno.lock\x27)" in str(w.get("key") or "")
            and bool(str(w.get("restore-keys") or "").strip()))

jobs_seen, failures = 0, []
for path in sys.argv[1:]:
    with open(path) as fh:
        data = yaml.safe_load(fh) or {}
    for name, job in (data.get("jobs") or {}).items():
        steps = (job or {}).get("steps") or []
        idx = [i for i, s in enumerate(steps)
               if str(s.get("uses") or "").startswith("denoland/setup-deno@")]
        if not idx:
            continue
        jobs_seen += 1
        if not any(is_deno_cache(s) for s in steps[idx[0] + 1:]):
            failures.append(f"{os.path.basename(path)} job={name}: no actions/cache "
                            "of ~/.cache/deno keyed on hashFiles(\x27deno.lock\x27) "
                            "with restore-keys after setup-deno")

print(f"jobs={jobs_seen}")
for f in failures:
    print(f)
sys.exit(1 if failures else (2 if jobs_seen == 0 else 0))
'
}

write_workflow() {
  printf '%s\n' "$1" > "${BATS_TEST_TMPDIR}/wf.yml"
}

@test "every job that installs Deno caches ~/.cache/deno keyed on deno.lock" {
  run python3 -c "$DENO_CACHE_CHECKER" "${WORKFLOWS_DIR}"/*.yml
  echo "$output"
  [ "$status" -eq 0 ]
  # Five jobs install Deno today; a sweep that finds none proves nothing.
  [[ "$output" =~ jobs=([0-9]+) ]]
  [ "${BASH_REMATCH[1]}" -ge 1 ]
}

@test "checker accepts a Deno job with a lock-keyed cache after setup-deno" {
  write_workflow "jobs:
  j:
    steps:
      - uses: denoland/setup-deno@0000000000000000000000000000000000000000
      - uses: actions/cache@0000000000000000000000000000000000000000
        with:
          path: ~/.cache/deno
          key: \${{ runner.os }}-deno-\${{ hashFiles('deno.lock') }}
          restore-keys: |
            \${{ runner.os }}-deno-"
  run python3 -c "$DENO_CACHE_CHECKER" "${BATS_TEST_TMPDIR}/wf.yml"
  [ "$status" -eq 0 ]
  [[ "$output" == *"jobs=1"* ]]
}

@test "checker rejects a Deno job with no cache step" {
  write_workflow "jobs:
  j:
    steps:
      - uses: denoland/setup-deno@0000000000000000000000000000000000000000
      - run: deno check"
  run python3 -c "$DENO_CACHE_CHECKER" "${BATS_TEST_TMPDIR}/wf.yml"
  [ "$status" -eq 1 ]
  [[ "$output" == *"job=j"* ]]
}

@test "checker rejects a cache placed before setup-deno" {
  write_workflow "jobs:
  j:
    steps:
      - uses: actions/cache@0000000000000000000000000000000000000000
        with:
          path: ~/.cache/deno
          key: deno-\${{ hashFiles('deno.lock') }}
          restore-keys: deno-
      - uses: denoland/setup-deno@0000000000000000000000000000000000000000"
  run python3 -c "$DENO_CACHE_CHECKER" "${BATS_TEST_TMPDIR}/wf.yml"
  [ "$status" -eq 1 ]
}

@test "checker rejects a cache whose key ignores deno.lock" {
  write_workflow "jobs:
  j:
    steps:
      - uses: denoland/setup-deno@0000000000000000000000000000000000000000
      - uses: actions/cache@0000000000000000000000000000000000000000
        with:
          path: ~/.cache/deno
          key: \${{ runner.os }}-deno
          restore-keys: deno-"
  run python3 -c "$DENO_CACHE_CHECKER" "${BATS_TEST_TMPDIR}/wf.yml"
  [ "$status" -eq 1 ]
}

@test "checker rejects a cache of the wrong directory or without restore-keys" {
  write_workflow "jobs:
  a:
    steps:
      - uses: denoland/setup-deno@0000000000000000000000000000000000000000
      - uses: actions/cache@0000000000000000000000000000000000000000
        with:
          path: ~/.npm
          key: deno-\${{ hashFiles('deno.lock') }}
          restore-keys: deno-
  b:
    steps:
      - uses: denoland/setup-deno@0000000000000000000000000000000000000000
      - uses: actions/cache@0000000000000000000000000000000000000000
        with:
          path: ~/.cache/deno
          key: deno-\${{ hashFiles('deno.lock') }}"
  run python3 -c "$DENO_CACHE_CHECKER" "${BATS_TEST_TMPDIR}/wf.yml"
  [ "$status" -eq 1 ]
  [[ "$output" == *"job=a"* ]]
  [[ "$output" == *"job=b"* ]]
}

@test "checker refuses a vacuous sweep with no Deno job" {
  write_workflow "jobs:
  j:
    steps:
      - run: echo hi"
  run python3 -c "$DENO_CACHE_CHECKER" "${BATS_TEST_TMPDIR}/wf.yml"
  [ "$status" -eq 2 ]
}
