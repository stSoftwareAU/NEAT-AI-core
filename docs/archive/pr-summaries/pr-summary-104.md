# PR Summary — Issue #104

## Summary

Pinned the `taiki-e/install-action` step in `.github/workflows/upgrade-dependencies.yml`
to a 40-char commit SHA, removing the last floating `@v2` major tag in the repo.
Floating tags can be re-pointed by a compromised maintainer account, executing
arbitrary code with the runner's `ACTIONS_PUSH` secret. The step now matches the
identical, already-pinned step in `ci.yml` (v2.79.2). Closes #104.

```yaml
# before
uses: taiki-e/install-action@v2

# after
# taiki-e/install-action@v2.79.2
uses: taiki-e/install-action@213ccc1a076163c093f914550b94feb90fab916d
```

This was missed by the Issue #77 third-party-actions pinning sweep.

## Evidence

Backend/CI-only change — no web interface to screenshot. Verified via the
existing `bats` SHA-pinning suite.

Before the fix:

```
$ bats tests/scripts/workflow_sha_pinning.bats
not ok 1 every action in every workflow is pinned to a 40-char commit SHA
  Unpinned actions:
    upgrade-dependencies.yml job=upgrade uses=taiki-e/install-action@v2
```

After the fix:

```
$ bats tests/scripts/workflow_sha_pinning.bats
ok 1 every action in every workflow is pinned to a 40-char commit SHA
ok 2 every SHA-pinned action has a version comment alongside it
ok 3 no workflow uses an action pinned to a deprecated Node runtime
ok 4 SHA-pin regex rejects floating tags and branch refs
```

## Test Plan

- Existing test `tests/scripts/workflow_sha_pinning.bats::every action in every
  workflow is pinned to a 40-char commit SHA` reproduced the failure against the
  unfixed workflow and passes after the SHA pin. No new test was needed — the
  guard already exists and now goes green.
- Companion test "every SHA-pinned action has a version comment alongside it"
  confirms the added `# taiki-e/install-action@v2.79.2` comment satisfies the
  version-comment requirement.

## Out of scope / pre-existing failures

`./quality.sh` reports four pre-existing failures unrelated to this change, all
concerning `ci.yml`/quarantine wiring (`bump-deps.sh` invocation and the
`VIBE_BUMP_QUARANTINE_HOURS` knob) — tests #31, #32, #33, #37. These exist on the
base branch before this change and are outside the scope of issue #104, which is
limited to the `upgrade-dependencies.yml` floating-tag pin.
