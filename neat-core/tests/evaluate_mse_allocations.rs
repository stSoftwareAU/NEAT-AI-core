//! Allocation-count regression for the dataset-offload evaluation path (Issue #386).
//!
//! `TrainingDataset::evaluate_mse` used to score **one record at a time** via
//! `CompiledNetwork::activate`, whose `to_vec()` allocates a fresh output `Vec`
//! per record — exactly the per-record allocation Issue #229 removed from the
//! batch scoring path, reintroduced on the >4 GB Memory64 offload lane. It now
//! drives the flat batched path instead, so its allocation count is **constant**
//! in the record count.
//!
//! Modelled on `tests/scoring_allocations.rs`: a counting global allocator
//! measures two evaluations whose record counts differ 10×; a per-record
//! allocation would show up as a proportional delta.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use neat_core::network::CompiledNetwork;
use neat_core::squash::SquashType;
use neat_core::training_data::TrainingDataConfig;
use neat_core::wasm_dataset::TrainingDataset;

/// Global allocator forwarding to the system allocator while counting every
/// allocation. Only the count is observed.
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

/// Single `Tanh` output neuron summing every input — enough topology to drive
/// the batched forward pass, small enough that the fixture build is cheap.
fn summing_tanh_network(num_inputs: usize) -> CompiledNetwork {
    let mut data = Vec::new();
    data.extend_from_slice(&((num_inputs + 1) as u32).to_le_bytes());
    data.extend_from_slice(&(num_inputs as u32).to_le_bytes());
    data.extend_from_slice(&0.05_f64.to_le_bytes());
    data.push(SquashType::Tanh as u8);
    data.push(0); // is_constant
    data.extend_from_slice(&(num_inputs as u16).to_le_bytes());
    for from_index in 0..num_inputs as u16 {
        data.extend_from_slice(&from_index.to_le_bytes());
        data.push(0); // synapse_type
        data.push(0); // padding
        data.extend_from_slice(&0.3_f64.to_le_bytes());
    }
    CompiledNetwork::new(&data).expect("network should parse")
}

/// Deterministic dataset of `count` records: `num_inputs` inputs + 1 target.
fn dataset(num_inputs: usize, count: usize) -> TrainingDataset {
    let inputs: Vec<f32> = (0..count * num_inputs)
        .map(|i| (i as f32 * 0.019).sin() * 0.7)
        .collect();
    let targets: Vec<f32> = (0..count).map(|r| (r as f32 * 0.011).cos()).collect();
    TrainingDataset::from_soa(inputs, targets, TrainingDataConfig::new(num_inputs, 1))
        .expect("fixture should be well-formed")
}

/// Allocations consumed by one `evaluate_mse` call over the whole dataset.
fn allocs_for(dataset: &TrainingDataset, network: &mut CompiledNetwork) -> usize {
    let count = dataset.num_records();
    let before = ALLOC_COUNT.load(Ordering::Relaxed);
    let mse = dataset
        .evaluate_mse(network, 0, count)
        .expect("batch is in range");
    std::hint::black_box(mse);
    ALLOC_COUNT.load(Ordering::Relaxed) - before
}

#[test]
fn evaluate_mse_has_no_per_record_allocation() {
    let num_inputs = 16;
    let mut net = summing_tanh_network(num_inputs);
    let small = dataset(num_inputs, 100);
    let large = dataset(num_inputs, 1000);

    // Warm up any one-off lazy initialisation so it is not counted below.
    std::hint::black_box(allocs_for(&small, &mut net));

    let small_allocs = allocs_for(&small, &mut net);
    let large_allocs = allocs_for(&large, &mut net);

    // The per-record `activate` path allocated one output `Vec` per record, so
    // 10× the records allocated ~900 more times. The batched flat path allocates
    // a constant handful (lane scratch + a bounded output buffer) regardless of
    // record count. A generous threshold separates "constant" from "grows with
    // records" without being brittle to allocator/harness noise.
    let delta = large_allocs.saturating_sub(small_allocs);
    assert!(
        delta < 50,
        "evaluating 10× the records allocated {large_allocs} vs {small_allocs} \
         (delta {delta}); expected a constant count — per-record allocation has \
         regressed into evaluate_mse"
    );
}
