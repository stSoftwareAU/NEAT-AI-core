//! Allocation-count regression for the record-scoring hot path (Issue #229, #230).
//!
//! `score_records` drives every record through the batched SIMD forward pass
//! ([`CompiledNetwork::score_batch_into`], Issue #230), writing into one
//! pre-sized flat buffer instead of allocating a fresh output `Vec` per record
//! via `activate`'s `to_vec()`. This test proves the batch performs **no
//! per-record output allocation**: a counting global allocator records how many
//! allocations happen while scoring, and the count must not scale with the
//! record count.
//!
//! A revert to the old `to_vec()`-per-record path would allocate ~one `Vec` per
//! record, so scoring 10× the records would allocate ~10× more — the delta
//! assertion below would then fail loudly instead of silently regressing
//! throughput.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

#[path = "../benches/common/mod.rs"]
#[allow(dead_code)]
mod common;

use common::{NETWORKS, NetSpec, build_inputs, build_network};
use neat_core::network::CompiledNetwork;

/// Global allocator that forwards to the system allocator while counting every
/// allocation. Only the count is observed; the delta between two scoring calls
/// with different record counts reveals per-record allocation.
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

fn spec(label: &str) -> &'static NetSpec {
    NETWORKS
        .iter()
        .find(|s| s.label == label)
        .unwrap_or_else(|| panic!("no NetSpec labelled {label}"))
}

fn build_records(net: &CompiledNetwork, count: usize) -> Vec<Vec<f32>> {
    (0..count)
        .map(|i| build_inputs(net.num_inputs(), 0xA110_0000 + i as u64))
        .collect()
}

/// Allocation count consumed by scoring `records` (records/network built
/// beforehand so only the scoring call is measured).
fn allocs_for(net: &CompiledNetwork, records: &[Vec<f32>], num_outputs: usize) -> usize {
    let before = ALLOC_COUNT.load(Ordering::Relaxed);
    let out = net.score_records(records, num_outputs);
    // Keep the result alive across the measurement so its allocation is counted.
    std::hint::black_box(&out);
    let after = ALLOC_COUNT.load(Ordering::Relaxed);
    after - before
}

#[test]
fn score_records_has_no_per_record_output_allocation() {
    let s = spec("production");
    let net = build_network(s, 0x000A_110C);

    let small = build_records(&net, 100);
    let large = build_records(&net, 1000);

    // Warm up any one-off lazy initialisation so it is not counted below.
    std::hint::black_box(net.score_records(&small, s.num_outputs));

    let small_allocs = allocs_for(&net, &small, s.num_outputs);
    let large_allocs = allocs_for(&net, &large, s.num_outputs);

    // Per-record allocation would make scoring 10× the records allocate ~900
    // extra times; the flat-buffer path allocates a constant handful (one output
    // buffer + one set of lane scratch buffers) regardless of record count. A generous
    // threshold distinguishes "constant" from "grows with records" without being
    // brittle to allocator/test-harness noise.
    let delta = large_allocs.saturating_sub(small_allocs);
    assert!(
        delta < 50,
        "scoring 10× records allocated {large_allocs} vs {small_allocs} \
         (delta {delta}); expected a constant count — per-record output \
         allocation has regressed"
    );
}
