//! Allocation regression for `scan_available_connections` (Issue #387).
//!
//! The pre-#387 implementation answered "does this connection already exist?"
//! from a dense `n × n` boolean matrix — 2.78 MB zeroed per call at the
//! production topology (n = 1,666) to record 21,513 synapses, a fill factor
//! under 0.8% — and then grew the result with `Vec::new()`, reallocating and
//! copying its way up to an ~11 MB flat pair list.
//!
//! Both costs are observable without touching the implementation: a counting
//! global allocator measures the peak concurrently-live bytes and the number of
//! allocations across one scan. A reintroduced `n²` matrix shows up as peak
//! bytes well above the result size; a reintroduced realloc chain shows up as
//! both a peak overshoot (old and new buffers live at once) and a large
//! allocation count.
//!
//! Modelled on `tests/scoring_allocations.rs`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use neat_core::topology_ops::scan_available_connections;

/// Global allocator forwarding to the system allocator while tracking live
/// bytes, peak live bytes, and the allocation count.
///
/// `realloc` is deliberately *not* forwarded to `System.realloc` — the default
/// `GlobalAlloc::realloc` allocates, copies and frees, which is the behaviour a
/// growing `Vec` must be assumed to have. That makes the peak figure
/// deterministic across platforms instead of depending on whether the system
/// allocator happened to extend a block in place.
struct TrackingAllocator;

static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);
static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        let live = LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
        PEAK_BYTES.fetch_max(live, Ordering::Relaxed);
        // SAFETY: forwarding an unchanged layout to the system allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        let live = LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
        PEAK_BYTES.fetch_max(live, Ordering::Relaxed);
        // SAFETY: forwarding an unchanged layout to the system allocator.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: `ptr`/`layout` came from an `alloc` call above.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;

/// Production anchor from Issue #387: 1,666 neurons carrying 21,513 synapses.
const PRODUCTION_NEURONS: usize = 1666;
const PRODUCTION_SYNAPSES: usize = 21_513;
const PRODUCTION_INPUTS: usize = 100;

/// Deterministic SplitMix64 draw so the fixture is reproducible.
fn next_u64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Sorted forward-only production-shaped topology.
fn production_topology() -> (Vec<u32>, Vec<u32>, Vec<u8>) {
    let mut state = 0x7061_7468_u64;
    let num_non_inputs = PRODUCTION_NEURONS - PRODUCTION_INPUTS;
    let base = PRODUCTION_SYNAPSES / num_non_inputs;
    let remainder = PRODUCTION_SYNAPSES % num_non_inputs;

    let mut per_from: Vec<Vec<u32>> = vec![Vec::new(); PRODUCTION_NEURONS];
    for offset in 0..num_non_inputs {
        let to = PRODUCTION_INPUTS + offset;
        let fan = (base + usize::from(offset < remainder)).min(to);
        let mut drawn = 0usize;
        let mut attempts = 0usize;
        while drawn < fan && attempts < fan * 8 + 16 {
            attempts += 1;
            let from = (next_u64(&mut state) % to as u64) as usize;
            if per_from[from].contains(&(to as u32)) {
                continue;
            }
            per_from[from].push(to as u32);
            drawn += 1;
        }
    }

    let mut from_indices = Vec::new();
    let mut to_indices = Vec::new();
    for (from, targets) in per_from.iter_mut().enumerate() {
        targets.sort_unstable();
        for &to in targets.iter() {
            from_indices.push(from as u32);
            to_indices.push(to);
        }
    }

    let is_constant = (0..PRODUCTION_NEURONS)
        .map(|i| u8::from(i >= PRODUCTION_INPUTS && i % 97 == 0))
        .collect();

    (from_indices, to_indices, is_constant)
}

#[test]
fn scan_available_connections_peak_allocation_tracks_result_size() {
    let (from_indices, to_indices, is_constant) = production_topology();

    LIVE_BYTES.store(0, Ordering::Relaxed);
    PEAK_BYTES.store(0, Ordering::Relaxed);
    ALLOC_COUNT.store(0, Ordering::Relaxed);

    let available = scan_available_connections(
        &from_indices,
        &to_indices,
        &is_constant,
        PRODUCTION_NEURONS as u32,
        PRODUCTION_INPUTS as u32,
    );

    let peak = PEAK_BYTES.load(Ordering::Relaxed);
    let allocations = ALLOC_COUNT.load(Ordering::Relaxed);
    let result_bytes = available.len() * size_of::<u32>();

    assert!(!available.is_empty(), "scan should find candidate slots");

    // The result itself is unavoidable. Everything else — the old `n²` matrix
    // (2.78 MB here) and the realloc chain's transient second buffer (~50% of
    // the result) — must be gone. 1 MiB of headroom covers the O(n) scratch
    // (adjacency, prefix sums, ~21.5k targets) with room to spare.
    let budget = result_bytes + (1 << 20);
    assert!(
        peak <= budget,
        "peak allocation {peak} B exceeded budget {budget} B \
         (result {result_bytes} B, n = {PRODUCTION_NEURONS}); \
         an n^2 matrix or a realloc chain has been reintroduced"
    );

    // The result vector is pre-sized, so it is allocated exactly once. A
    // growth chain over an ~11 MB vector would be ~21 allocations on its own.
    assert!(
        allocations <= 16,
        "expected a handful of allocations, saw {allocations}; \
         the result vector is no longer pre-sized"
    );
}
