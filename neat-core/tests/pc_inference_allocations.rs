//! Allocation-count regression for the predictive-coding settling loop
//! (Issue #389).
//!
//! Before the fix, `compute_errors` allocated a fresh `predictions` and `errors`
//! `Vec` on every settling step, so an `infer` run allocated `2 × (steps + 1)`
//! vectors — allocation grew with `inference_steps`. After hoisting those
//! buffers out of the loop (`compute_errors_into`) and reusing one scratch set
//! across `infer_batch` samples, the per-call allocation count is O(1) in the
//! number of steps.
//!
//! These tests drive `infer`/`infer_batch` with a small and a large step count
//! on an identical topology and assert the allocation count barely moves. A
//! revert to per-step allocation would make the large-step run allocate
//! hundreds more times and fail the delta assertions loudly.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use neat_core::pc_inference::{PcConnection, PcNeuron, PredictiveCodingEngine};
use neat_core::squash::SquashType;

/// Global allocator forwarding to the system allocator while counting `alloc`
/// calls. Only the count is observed.
struct CountingAllocator;

static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        // SAFETY: forwarding an unchanged layout to the system allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr`/`layout` came from `System.alloc` above.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// `ALLOC_COUNT` is a *process-global* counter incremented by every thread.
/// Under `cargo test --test-threads=2` the two measuring tests in this binary
/// would otherwise run concurrently, so the sibling test's allocations would
/// leak into each measured delta and make the assertions flaky (observed on CI
/// as a spurious delta of 47). Holding this lock across the whole measured
/// region serialises the two tests so each count reflects only its own work.
static MEASURE_LOCK: Mutex<()> = Mutex::new(());

/// 2 inputs → 2 hidden (multi-fan-in) → 1 output. `energy_threshold` of 0 keeps
/// the loop from converging early so it always runs `inference_steps` steps.
fn engine(inference_steps: u32) -> PredictiveCodingEngine {
    let neurons = vec![
        PcNeuron {
            bias: 0.1,
            squash_type: SquashType::Tanh,
            is_hidden: true,
            conn_start: 0,
            conn_count: 2,
        },
        PcNeuron {
            bias: -0.1,
            squash_type: SquashType::Tanh,
            is_hidden: true,
            conn_start: 2,
            conn_count: 2,
        },
        PcNeuron {
            bias: 0.0,
            squash_type: SquashType::Identity,
            is_hidden: false,
            conn_start: 4,
            conn_count: 2,
        },
    ];
    let connections = vec![
        PcConnection {
            from: 0,
            weight: 0.5,
        },
        PcConnection {
            from: 1,
            weight: -0.3,
        },
        PcConnection {
            from: 0,
            weight: -0.4,
        },
        PcConnection {
            from: 1,
            weight: 0.6,
        },
        PcConnection {
            from: 2,
            weight: 1.0,
        },
        PcConnection {
            from: 3,
            weight: -0.5,
        },
    ];
    PredictiveCodingEngine::new_from_parts(2, 1, neurons, connections, inference_steps, 0.05, 0.0)
}

fn allocs_for_infer(steps: u32, input: &[f32]) -> usize {
    let engine = engine(steps);
    let before = ALLOC_COUNT.load(Ordering::Relaxed);
    let out = engine.infer(input, None);
    std::hint::black_box(&out);
    ALLOC_COUNT.load(Ordering::Relaxed) - before
}

fn allocs_for_batch(steps: u32, inputs: &[&[f32]]) -> usize {
    let engine = engine(steps);
    let before = ALLOC_COUNT.load(Ordering::Relaxed);
    let out = engine.infer_batch(inputs, None);
    std::hint::black_box(&out);
    ALLOC_COUNT.load(Ordering::Relaxed) - before
}

#[test]
fn infer_allocation_is_constant_in_steps() {
    // Serialise against the sibling test so the process-global allocation
    // counter is not contaminated by concurrent allocations. Recover from
    // poisoning: a panic in the other test must not mask this one.
    let _guard = MEASURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let input = [1.0f32, 0.5];

    // Warm up any one-off lazy initialisation so it is not counted below.
    std::hint::black_box(engine(10).infer(&input, None));

    let few = allocs_for_infer(10, &input);
    let many = allocs_for_infer(500, &input);

    // Per-step allocation would add ~2 vectors per extra step: 490 extra steps
    // would allocate ~980 more times. The hoisted-buffer path allocates a
    // constant handful (latents + predictions + errors + energy_history)
    // regardless of step count.
    let delta = many.saturating_sub(few);
    assert!(
        delta < 20,
        "infer with 500 steps allocated {many} vs {few} with 10 steps \
         (delta {delta}); expected a constant count — per-step Vec allocation \
         has regressed"
    );
}

#[test]
fn infer_batch_allocation_is_constant_in_steps() {
    let a = [1.0f32, 0.5];
    let b = [-1.0f32, 2.0];
    let c = [0.0f32, -0.7];
    let d = [3.0f32, -2.0];
    let inputs: Vec<&[f32]> = vec![&a, &b, &c, &d];

    // Serialise against the sibling test — see `infer_allocation_is_constant_in_steps`.
    let _guard = MEASURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    std::hint::black_box(engine(10).infer_batch(&inputs, None));

    let few = allocs_for_batch(10, &inputs);
    let many = allocs_for_batch(500, &inputs);

    // One scratch set is reused across all samples, so extra steps add no
    // allocations. A per-step or per-sample-scratch regression would make the
    // 500-step batch allocate hundreds more times.
    let delta = many.saturating_sub(few);
    assert!(
        delta < 20,
        "infer_batch with 500 steps allocated {many} vs {few} with 10 steps \
         (delta {delta}); expected a constant count — settling-loop allocation \
         has regressed"
    );
}
