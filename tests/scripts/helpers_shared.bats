#!/usr/bin/env bats
# Unit tests for the shared BATS helpers (Issue #477).
#
# The helpers in helpers.bash replace bodies that were copy-pasted across up to
# six test files. Sharing them is only an improvement if the single copy is
# itself pinned, so these tests exercise each helper against purpose-built
# fixtures — a workflow that satisfies the contract and one that violates it —
# and assert on the helper's exit status, not on its source text.

load helpers

setup() {
  FIXTURES="${BATS_TEST_TMPDIR}/fixtures"
  mkdir -p "$FIXTURES"
}

# --- GitHub branch-filter glob model ----------------------------------------

@test "glob model: a single star does not cross a slash" {
  require_python3
  run github_glob_matches '*' 'Develop'
  [ "$status" -eq 0 ]
  run github_glob_matches '*' 'milestone/clean-up-23-jul'
  [ "$status" -ne 0 ]
}

@test "glob model: a double star crosses slashes" {
  require_python3
  run github_glob_matches '**' 'milestone/clean-up-23-jul'
  [ "$status" -eq 0 ]
  run github_glob_matches 'milestone/**' 'milestone/a/b'
  [ "$status" -eq 0 ]
  run github_glob_matches 'milestone/*' 'milestone/a/b'
  [ "$status" -ne 0 ]
}

@test "glob model: non-glob characters are literal and the match is anchored" {
  require_python3
  # A '.' is a literal dot, not the regex "any character".
  run github_glob_matches 'v1.0' 'v1x0'
  [ "$status" -ne 0 ]
  run github_glob_matches 'v1.0' 'v1.0'
  [ "$status" -eq 0 ]
  # Anchored at both ends: a prefix match is not a match.
  run github_glob_matches 'Develop' 'Development'
  [ "$status" -ne 0 ]
}

@test "assert_pr_branch_filter_matches accepts a milestone-aware filter and rejects a star-only one" {
  require_python3
  cat >"${FIXTURES}/good.yml" <<'YAML'
name: good
on:
  pull_request:
    branches: [Develop, main, "milestone/**"]
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: "true"
YAML
  cat >"${FIXTURES}/bad.yml" <<'YAML'
name: bad
on:
  pull_request:
    branches: ["*"]
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: "true"
YAML
  run assert_pr_branch_filter_matches "${FIXTURES}/good.yml" \
    "milestone/clean-up-23-jul" Develop main
  [ "$status" -eq 0 ]

  run assert_pr_branch_filter_matches "${FIXTURES}/bad.yml" \
    "milestone/clean-up-23-jul" Develop
  [ "$status" -ne 0 ]
}

# --- Least-privilege permissions --------------------------------------------

@test "assert_job_least_privilege accepts a read-only job and its declared writes" {
  require_python3
  cat >"${FIXTURES}/perms.yml" <<'YAML'
name: perms
on: [push]
jobs:
  readonly:
    runs-on: ubuntu-latest
    permissions:
      contents: read
    steps:
      - run: "true"
  commenter:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      pull-requests: write
    steps:
      - run: "true"
YAML
  run assert_job_least_privilege "${FIXTURES}/perms.yml" readonly
  [ "$status" -eq 0 ]
  run assert_job_least_privilege "${FIXTURES}/perms.yml" commenter pull-requests
  [ "$status" -eq 0 ]
}

@test "assert_job_least_privilege rejects an undeclared write scope" {
  require_python3
  cat >"${FIXTURES}/perms.yml" <<'YAML'
name: perms
on: [push]
jobs:
  writer:
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - run: "true"
YAML
  # contents: write fails the read requirement outright.
  run assert_job_least_privilege "${FIXTURES}/perms.yml" writer
  [ "$status" -ne 0 ]
  # An extra write scope beyond the declared allowance also fails.
  run assert_job_least_privilege "${FIXTURES}/perms.yml" writer contents
  [ "$status" -ne 0 ]
}

@test "assert_job_least_privilege rejects a missing permissions block and a missing job" {
  require_python3
  cat >"${FIXTURES}/perms.yml" <<'YAML'
name: perms
on: [push]
jobs:
  inherits:
    runs-on: ubuntu-latest
    steps:
      - run: "true"
YAML
  run assert_job_least_privilege "${FIXTURES}/perms.yml" inherits
  [ "$status" -ne 0 ]
  run assert_job_least_privilege "${FIXTURES}/perms.yml" absent
  [ "$status" -ne 0 ]
}

@test "assert_job_least_privilege falls back to the workflow-level permissions block" {
  require_python3
  cat >"${FIXTURES}/perms.yml" <<'YAML'
name: perms
on: [push]
permissions:
  contents: read
jobs:
  inherits:
    runs-on: ubuntu-latest
    steps:
      - run: "true"
YAML
  run assert_job_least_privilege "${FIXTURES}/perms.yml" inherits
  [ "$status" -eq 0 ]
}

# --- Step extraction ---------------------------------------------------------

@test "extract_step writes the real script and the shell GitHub would launch it with" {
  require_python3
  cat >"${FIXTURES}/steps.yml" <<'YAML'
name: steps
on: [push]
jobs:
  work:
    runs-on: ubuntu-latest
    steps:
      - name: Do the thing
        run: echo done
      - name: Do it with pipefail
        shell: bash
        run: echo piped
YAML
  run extract_step "${FIXTURES}/steps.yml" "Do the thing" "$FIXTURES"
  [ "$status" -eq 0 ]
  [ "$(cat "${FIXTURES}/step.sh")" = "echo done" ]
  [ "$(cat "${FIXTURES}/shell.cmd")" = "bash -e" ]

  run extract_step "${FIXTURES}/steps.yml" "pipefail" "$FIXTURES"
  [ "$status" -eq 0 ]
  [ "$(cat "${FIXTURES}/shell.cmd")" = "bash --noprofile --norc -eo pipefail" ]
}

@test "extract_step fails loud when the named step is absent" {
  require_python3
  cat >"${FIXTURES}/steps.yml" <<'YAML'
name: steps
on: [push]
jobs:
  work:
    runs-on: ubuntu-latest
    steps:
      - name: Do the thing
        run: echo done
YAML
  run extract_step "${FIXTURES}/steps.yml" "Never named this" "$FIXTURES"
  [ "$status" -ne 0 ]
}

# --- Pinned CLI install ------------------------------------------------------

@test "assert_pinned_cli_install accepts a pinned install and rejects a floating one" {
  require_python3
  cat >"${FIXTURES}/pinned.yml" <<'YAML'
name: pinned
on: [push]
env:
  TOOL_VERSION: "1.2.3"
  TOOL_SHA256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
jobs:
  install:
    runs-on: ubuntu-latest
    steps:
      - name: Install tool
        run: |
          curl -sSL -o tool.tar.gz "https://example.test/releases/download/v${TOOL_VERSION}/tool.tar.gz"
          echo "${TOOL_SHA256}  tool.tar.gz" | sha256sum -c -
YAML
  cat >"${FIXTURES}/floating.yml" <<'YAML'
name: floating
on: [push]
jobs:
  install:
    runs-on: ubuntu-latest
    steps:
      - name: Install tool
        run: curl -sSL https://example.test/install.sh | sh
YAML
  url_re='example\.test/releases/download/v(\$\{?TOOL_VERSION\}?|[0-9]+\.[0-9]+\.[0-9]+)/'
  run assert_pinned_cli_install "${FIXTURES}/pinned.yml" TOOL "$url_re"
  [ "$status" -eq 0 ]

  run assert_pinned_cli_install "${FIXTURES}/floating.yml" TOOL "$url_re"
  [ "$status" -ne 0 ]
}

# --- Comment stripping -------------------------------------------------------

@test "strip_comments removes commented-out lines so they cannot satisfy a grep" {
  printf 'uses: safe/action@sha\n# uses: unsafe/action@sha\nrun: echo hi # trailing\n' \
    >"${FIXTURES}/commented.yml"
  run strip_comments "${FIXTURES}/commented.yml"
  [ "$status" -eq 0 ]
  [[ "$output" != *"unsafe/action"* ]]
  [[ "$output" == *"safe/action@sha"* ]]
  [[ "$output" != *"trailing"* ]]
}

# --- CycloneDX SBOM release --------------------------------------------------

@test "assert_cyclonedx_sbom_release rejects an unpinned install and an SBOM built after the release" {
  require_python3
  cat >"${FIXTURES}/sbom.yml" <<'YAML'
name: sbom
on: [push]
jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - name: Publish
        run: gh release create "$TAG" bundle.tar.gz sbom.cdx.json
      - name: SBOM
        run: |
          cargo install cargo-cyclonedx
          cargo cyclonedx --format json
YAML
  run assert_cyclonedx_sbom_release "${FIXTURES}/sbom.yml" publish
  [ "$status" -ne 0 ]
}
