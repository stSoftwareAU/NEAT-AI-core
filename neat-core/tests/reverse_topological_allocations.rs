//! Allocation-count regression for `compute_reverse_topological_order`
//! (Issue #388).
//!
//! The function used to build its inward adjacency as a `Vec<Vec<u32>>` — one
//! heap allocation per neuron, plus the geometric regrowth of each inner `Vec`
//! as synapses were pushed. It now builds a CSR (compressed sparse row)
//! adjacency in two passes, so the allocation count is a small constant
//! independent of the neuron count.
//!
//! A counting global allocator (same pattern as `scoring_allocations.rs`)
//! records how many allocations happen inside the call. Growing the neuron
//! count 16× must not grow the allocation count: with the old layout it would
//! grow by thousands.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use neat_core::topology_ops::compute_reverse_topological_order;

/// Global allocator forwarding to the system allocator while counting every
/// allocation. Only the delta across two calls is observed.
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

/// Deterministic layered DAG: `num_inputs` inputs feeding a chain of non-input
/// neurons, each drawing `fan_in` incoming edges from strictly earlier neurons.
fn build_dag(num_neurons: usize, num_inputs: usize, fan_in: usize) -> (Vec<u32>, Vec<u32>) {
    let mut from = Vec::new();
    let mut to = Vec::new();
    for target in num_inputs..num_neurons {
        for k in 0..fan_in {
            // Spread the sources deterministically over the earlier neurons.
            let source = (target * 7 + k * 13) % target;
            from.push(source as u32);
            to.push(target as u32);
        }
    }
    (from, to)
}

/// Allocations consumed by one `compute_reverse_topological_order` call.
fn allocs_for(from: &[u32], to: &[u32], num_neurons: u32, num_inputs: u32) -> usize {
    let before = ALLOC_COUNT.load(Ordering::Relaxed);
    let order = compute_reverse_topological_order(from, to, num_neurons, num_inputs);
    // Keep the result alive across the measurement so its allocation is counted.
    std::hint::black_box(&order);
    let after = ALLOC_COUNT.load(Ordering::Relaxed);
    after - before
}

/// One test only: the allocation counter is process-global, so a second
/// concurrently-running test in this binary would pollute the measurement.
#[test]
fn reverse_topological_order_allocation_count_does_not_scale_with_neurons() {
    let (small_from, small_to) = build_dag(128, 16, 8);
    let (large_from, large_to) = build_dag(2048, 16, 8);

    let small_allocs = allocs_for(&small_from, &small_to, 128, 16);
    let large_allocs = allocs_for(&large_from, &large_to, 2048, 16);

    // The per-neuron `Vec<Vec<u32>>` layout allocated once per neuron, so the
    // 16× larger topology allocated ~7,900 more times. The CSR layout allocates
    // a fixed handful of buffers — inward_starts, cursor, inward_indices,
    // out_degree, queue, result, visited — regardless of size.
    let delta = large_allocs.saturating_sub(small_allocs);
    assert!(
        delta < 10,
        "16× the neurons allocated {large_allocs} vs {small_allocs} (delta \
         {delta}); expected a constant count — per-neuron adjacency allocation \
         has regressed"
    );

    // …and that constant is a single-digit handful, not a per-neuron count.
    assert!(
        large_allocs <= 12,
        "expected a small constant number of allocations, got {large_allocs}"
    );
}
