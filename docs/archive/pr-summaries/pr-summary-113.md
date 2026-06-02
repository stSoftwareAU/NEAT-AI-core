# Add `repository` metadata to Cargo workspace package (Issue #113)

## Summary

Completed the standard Rust API Guidelines (C-METADATA) metadata set by adding a
`repository` field to the shared `[workspace.package]` table in the root
`Cargo.toml`, and inheriting it in `neat-core/Cargo.toml` via
`repository.workspace = true` (alongside the existing
`version`/`edition`/`license` inheritances). Consumers, `cargo doc`, registry
pages, and supply-chain provenance tooling can now mechanically locate the
upstream source from the manifest. Closes #113.

## Evidence

Backend/manifest change — no web interface to screenshot. Verified by a new
"what" test that asserts on the observable Cargo metadata (the `CARGO_PKG_*`
values Cargo resolves from the manifest, including workspace inheritance):

```
running 2 tests
test core_metadata_set_is_complete ... ok
test repository_metadata_points_at_upstream_source ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

The three pre-existing `quality.sh` failures (BATS 31–33, concerning
`ci.yml`/`bump-deps.sh`) are unrelated to this change — they reproduce on the
clean base tree before any edit.

## Test Plan

- Added `neat-core/tests/package_metadata.rs`:
  - `repository_metadata_points_at_upstream_source` — asserts
    `CARGO_PKG_REPOSITORY` equals the upstream GitHub URL.
  - `core_metadata_set_is_complete` — asserts `description`, `license`
    (`Apache-2.0`) and `repository` are all present (C-METADATA completeness).
