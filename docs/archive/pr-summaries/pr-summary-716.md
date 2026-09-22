## Summary

Fixed the `SEMGREP_IMAGE` pin in `.github/workflows/semgrep.yml` so the release
tag sits beside the digest instead of the ref being a bare
`semgrep/semgrep@sha256:…`. The ref is now
`semgrep/semgrep:1.170.1@sha256:98c2572f…`, byte-for-byte immutable **and**
trackable by Renovate/Dependabot (both resolve version bumps from the tag, then
rewrite the digest beside it). Closes #716.

## Evidence

The change is a workflow YAML value plus a regression test; no web interface to
screenshot.

- **Regression test** —
  `tests/scripts/semgrep_workflow.bats::SEMGREP_IMAGE pin carries its release tag beside the digest`
  — added, observed failing against the pre-fix bare-digest ref and passing after
  the fix. It asserts the ref matches
  `<image>:<tag>@sha256:<64-hex>` and that the tag equals `SEMGREP_VERSION`, so
  the container path and the PyPI fallback keep scanning the same release.
- **`bats tests/scripts/semgrep_workflow.bats`** — 10/10 pass (including the new
  test).
- **`bats tests/scripts/workflow_container_pinning.bats`** — 4/4 pass: the
  tagged `:1.170.1@sha256:…` ref still satisfies the digest-pin gate and its
  version-comment rule.
- **`codespell`** on both changed files — clean.
- **`cargo check --workspace --all-targets --all-features`** — passes (13.9s).

### Full gate status

`./quality.sh < /dev/null` was run once. It reaches two failure groups that are
**pre-existing and environmental**, neither caused by this change:

1. `tests/scripts/build_wasm_bundle_wasm64.bats` — needs a nightly `rustup`
   toolchain, and no `rustup` binary exists in this container.
2. `tests/scripts/markdown_lint_workflow.bats` ("markdownlint-cli2 passes against
   the current tree") — fails only on the locally generated, **gitignored**
   `graft/` directory (0 tracked files under `graft/`; `git ls-files` confirms).
   CI checks out the repo fresh and never sees it.

Both are outside this change's scope; the workflow-YAML + bats stages this
change actually touches are green (see above).

## Test Plan

- Added `SEMGREP_IMAGE pin carries its release tag beside the digest` in
  `tests/scripts/semgrep_workflow.bats` — asserts the ref carries a release tag
  beside its digest and that the tag matches `SEMGREP_VERSION`.
- No existing tests were modified or removed.
