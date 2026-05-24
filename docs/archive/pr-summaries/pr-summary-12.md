## Summary
Carried over `simd_native.rs` from the `makefaster` branch to add native SIMD fast paths for multi-record weighted sums:

- **x86_64**: AVX2 + FMA (`_mm256_fmadd_ps`) for 8-wide; FMA + SSE (`_mm_fmadd_ps`) for 4-wide.
- **aarch64**: NEON (`vfmaq_f32` over two `float32x4` lanes) for 8-wide; single `float32x4` for 4-wide.
- Runtime feature detection (`is_x86_feature_detected!` / `is_aarch64_feature_detected!`) gates every `unsafe` path; scalar fallback otherwise.

`simd.rs` no longer carries the scalar fallback bodies for `weighted_sum_simd_4records` / `weighted_sum_simd_8records` — it re-exports them from `simd_native`. The `simd128`/wasm path is unchanged.

Closes #12.

## Evidence
Pure backend SIMD code — no UI to screenshot.

- `cargo test --workspace` passes (all 126 tests, incl. 2 new in `simd_native`).
- `./quality.sh` passes: rustfmt, clippy (`-D warnings`), cargo-deny, tests, docs, release build.
- Tests compare native dispatcher output to the bundled scalar reference using exact equality on small inputs that fit FMA equivalence.

## Test Plan
- `simd::simd_native::tests::native_8_matches_scalar_small` — dispatcher output matches scalar for 8 records.
- `simd::simd_native::tests::native_4_matches_scalar_small` — dispatcher output matches scalar for 4 records.
- All existing `simd` and workspace tests continue to pass.
