# PR Summary: Fix panics unwinding across FFI boundary in wasm-bench exports (#736)

**Issue:** #736  
**PR:** #739  
**Branch:** `issue-736-wasm-bench-extern-c-exports-can-panic-across-the-f`

## Summary

Panics unwinding across a Rust `extern "C"` FFI boundary are undefined behaviour. This fix guards all public exports in `wasm-bench` against panicking on misuse by using `std::panic::catch_unwind` to catch panics and return sentinel values instead:

- **u32 exports** return `0xFFFFFFFF` on panic or validation failure
- **f64 exports** return `NaN` on panic or validation failure

The JavaScript driver (`runner.mjs`) detects these sentinels and fails loud with a clear error message, preventing undefined behaviour from silently propagating into benchmark results. All six public exports are now guarded, and the code follows a layered safety approach that combines safe indexing, exception handling, and driver-side sentinel detection.

**Breaking change:** `seed_activations()` signature changed from `-> ()` (void) to `-> u32` (status code). Callers must check the return value to detect failures.

## Files Changed

1. **wasm-bench/src/lib.rs** — Rust FFI exports with panic guards
   - Added `use std::panic::{AssertUnwindSafe, catch_unwind};`
   - Added `pub const FAILED: u32 = u32::MAX;` sentinel constant
   - Added `guarded()` helper that wraps export bodies with `catch_unwind` and option chaining
   - Guarded all 6 public exports: `setup()`, `neuron_count()`, `input_count()`, `record_count()`, `seed_activations()`, `bench_kernel()`, `bench_activate()`, `bench_score()`
   - **Breaking change:** `seed_activations()` now returns `u32` (0 on success, FAILED on failure) instead of void
   - Added 5 regression tests covering: invalid shape, missing fixture, NaN returns, zero records, valid setup

2. **wasm-bench/runner.mjs** — JavaScript driver with sentinel detection
   - Added `FAILED_U32 = 0xffffffff` sentinel constant
   - Added `checkU32(label, value)` helper that throws if value equals sentinel
   - Added `checkChecksum(label, value)` helper that throws if value is NaN
   - Wrapped all setup calls in sentinel checks via `checkU32()`
   - Wrapped all benchmark calls in sentinel checks via `checkChecksum()`
   - Sentinel detection immediately fails the benchmark with clear error message

3. **wasm-bench/README.md** — Documentation
   - Added new section "FFI panic safety (Issue #736)" explaining the problem and solution
   - Documented sentinel values and driver behaviour
   - Noted breaking change to `seed_activations()` signature

## Technical Approach

### Panic Safety at FFI Boundary

The fix uses `std::panic::catch_unwind(AssertUnwindSafe(...))` combined with option chaining to elegantly handle both validation failures (returning `None`) and caught panics (returning `Err`):

```rust
fn guarded<R>(sentinel: R, body: impl FnOnce() -> Option<R>) -> R {
    catch_unwind(AssertUnwindSafe(body))
        .ok()           // Convert Result<Option<R>, _> to Option<Option<R>>
        .flatten()      // Flatten Option<Option<R>> to Option<R>
        .unwrap_or(sentinel)  // Return sentinel if None or Err
}
```

### Layered Defense Strategy

1. **Safe indexing:** Use `slice::get()` instead of direct indexing to prevent panics in expected validation paths (e.g., `NETWORKS.get(...)` returns `None` instead of panicking)
2. **Exception handling:** `catch_unwind()` catches any panics that do occur, including those from out-of-bounds accesses
3. **Sentinel returns:** Panics and validation failures both return sentinel values that never look like valid results (u32::MAX is not a realistic synapse count; NaN is not a realistic checksum)
4. **Driver detection:** JavaScript driver checks sentinels and throws immediately (fail-loud principle)

### Breaking Change: seed_activations() Signature

The function previously returned void (`-> ()`), but now returns u32 to report success/failure:

```rust
// Before:
pub extern "C" fn seed_activations() -> ()

// After:
pub extern "C" fn seed_activations() -> u32  // 0 = success, FAILED = failure
```

This change is necessary because the function needs to distinguish between:
- Valid record loaded successfully → return 0
- Fixture not initialized → return FAILED
- Fixture has zero records → return FAILED

## Test Results

All quality gates pass:

- ✅ Formatting: `cargo fmt` clean
- ✅ Clippy: No warnings (workspace lint config enforced)
- ✅ Tests: **5 regression tests** all passing
  - `setup_with_an_out_of_range_shape_returns_the_sentinel` — validates shape range checking
  - `count_exports_before_setup_return_the_sentinel` — validates pre-setup state handling
  - `bench_exports_before_setup_return_nan` — validates f64 sentinel returns
  - `seeding_a_fixture_with_no_records_returns_the_sentinel` — validates record requirement
  - `a_valid_setup_reports_the_real_topology_and_finite_checksums` — validates happy path
- ✅ Documentation tests: 16 passed
- ✅ Full suite: `cargo test --workspace` clean
- ✅ Release build: successful

## Reproduction / Evidence

### Demonstrating Panic Safety

Before this fix, calling `bench_kernel()` before `setup()` would cause undefined behaviour. Now it returns `NaN`:

```javascript
// JavaScript driver
checkChecksum("bench_kernel", exports.bench_kernel());
// Throws: "bench_kernel returned NaN (Issue #736)"
```

### Demonstrating seed_activations() Status Code

The new signature allows callers to detect setup failures:

```javascript
// If setup succeeded but had zero records:
const result = exports.seed_activations();  // returns 0xFFFFFFFF
checkU32("seed_activations", result);
// Throws: "seed_activations returned the failure sentinel (Issue #736)"
```

## Verification

- Committed no forbidden hidden files (pre-commit gate verified)
- All three modified files verified readable and correct via Read tool
- Branch properly pushed to origin with `-u` flag
- PR #739 created and linked to Issue #736
- Full `./quality.sh` test suite executed and passed
- Working directory clean with no scratch files remaining

## Design Notes

1. **No silent failures:** Every export failure is detected and reported, never masked as a successful result
2. **Principle of least privilege:** Only the Rust side catches unwinding; the JavaScript side checks sentinels
3. **One guarded helper:** The `guarded()` helper is the single pattern for all exports, making it easy to audit and maintain
4. **Thread-local fixture:** The `RefCell<Option<Fixture>>` pattern allows safe mutation across FFI calls while maintaining Rust safety guarantees
5. **Fail-loud JavaScript:** The driver throws on any sentinel detection rather than folding it into a result row, following NEAT-AI's engineering principle that defects must fail loud rather than pass silently

---

**Co-Authored-By:** Claude Haiku 4.5 <noreply@anthropic.com>
