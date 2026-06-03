## Summary

Added `// SAFETY:` comments to the four SIMD dispatch `unsafe` blocks in
`neat-core/src/simd_native.rs`. Each comment names the runtime feature-detection
guard (`is_x86_feature_detected!` / `is_aarch64_feature_detected!`) that proves
the corresponding `#[target_feature(...)]` precondition documented on the callee,
recording the soundness proof next to the code it protects. This is a
documentation-only change — no behaviour, control flow, or generated code is
affected. Closes #112.

The four blocks updated:

| Dispatch | Guard | Callee precondition |
|----------|-------|---------------------|
| 8-record AVX2 | `is_x86_feature_detected!("avx2")` | `#[target_feature(enable = "avx2")]` |
| 8-record NEON | `is_aarch64_feature_detected!("neon")` | `#[target_feature(enable = "neon")]` |
| 4-record FMA  | `is_x86_feature_detected!("fma")` | `#[target_feature(enable = "fma")]` |
| 4-record NEON | `is_aarch64_feature_detected!("neon")` | `#[target_feature(enable = "neon")]` |

## Evidence

Backend/library change with no web interface to screenshot. Verified by:

- `cargo fmt --all -- --check` — clean (exit 0).
- `cargo clippy --workspace --all-targets` — no warnings.
- `cargo test --package neat-core simd_native` — `native_4_matches_scalar_small`
  and `native_8_matches_scalar_small` pass, confirming the SIMD dispatch paths
  still produce results matching the scalar reference.

Each comment's `#[target_feature]` text was cross-checked against the actual
attributes on the callee functions (lines 81, 124, 163, 212) to ensure accuracy.

Note: `quality.sh` reports four pre-existing failures (tests 31, 32, 33, 37)
relating to `ci.yml`/`bump-deps.sh`. These were confirmed present on the clean
tree before this change (via `git stash`) and are unrelated to this issue.

## Test Plan

No new tests — this is a comment-only change with no observable behaviour to
assert on (per AGENTS.md, "what" tests cover behaviour; comments have none).
Existing coverage exercising the touched dispatch paths:

- `simd::simd_native::tests::native_8_matches_scalar_small`
- `simd::simd_native::tests::native_4_matches_scalar_small`
