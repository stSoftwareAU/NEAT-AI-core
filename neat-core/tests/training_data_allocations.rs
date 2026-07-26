//! Allocation-count regression for the binary training-data reader (Issue #385).
//!
//! The reader used to allocate three `Vec`s and one fully-discarded copy per
//! record: a whole-record intermediate, then two copied-out halves. This test
//! pins the two rewritten paths against that regression using a counting global
//! allocator over a **production-width** corpus (2461 inputs + 1 output ≈ 9.8
//! KiB/record, matching `benches/common/mod.rs`):
//!
//! - the batch path ([`read_file`]) allocates **at most two** `Vec`s per record
//!   (the `inputs` and `outputs` buffers) — no intermediate, no discarded copy;
//! - the streaming in-place path
//!   ([`TrainingDataIterator::next_record_into`]) is **allocation-free** per
//!   record once its reusable buffers reach record width.
//!
//! A revert to the old intermediate-plus-copy parse would scale allocations
//! with the record count and fail the delta assertions loudly instead of
//! silently regressing throughput.

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use neat_core::training_data::{
    TrainingDataConfig, TrainingDataIterator, TrainingRecord, read_file,
};

/// Global allocator that forwards to the system allocator while counting every
/// allocation. Only the count is observed; the delta between two reads with
/// different record counts reveals per-record allocation.
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

// Production creature width from `benches/common/mod.rs`: 2461 inputs + 1
// output = 2462 f32 = 9848 bytes per record.
const NUM_INPUTS: usize = 2461;
const NUM_OUTPUTS: usize = 1;
const SMALL: usize = 100;
const LARGE: usize = 1100;

/// Write `num_records` production-width records of little-endian `f32`s.
fn write_corpus(path: &Path, num_records: usize) {
    let vals_per = NUM_INPUTS + NUM_OUTPUTS;
    let mut bytes: Vec<u8> = Vec::with_capacity(num_records * vals_per * 4);
    for i in 0..num_records {
        for j in 0..vals_per {
            let v = (i * vals_per + j) as f32;
            bytes.extend_from_slice(&v.to_le_bytes());
        }
    }
    std::fs::write(path, &bytes).unwrap();
}

/// Allocations consumed by [`read_file`] over a `num_records`-record corpus
/// (corpus written beforehand so only the read is measured).
fn read_file_allocs(path: &Path, config: &TrainingDataConfig) -> usize {
    let before = ALLOC_COUNT.load(Ordering::Relaxed);
    let records = read_file(path, config).unwrap();
    // Keep the result alive across the measurement so its allocation is counted.
    std::hint::black_box(&records);
    ALLOC_COUNT.load(Ordering::Relaxed) - before
}

/// Drain a single-file directory with `next_record_into`, returning the
/// allocations consumed *after* the reusable buffers have reached record width.
fn streaming_allocs(dir: &Path, config: &TrainingDataConfig) -> usize {
    let mut iter = TrainingDataIterator::new(dir, config.clone()).unwrap();
    let mut record = TrainingRecord {
        inputs: Vec::new(),
        outputs: Vec::new(),
    };
    // Prime: the first record grows both buffers to record width. Exclude that
    // one-off growth from the measurement.
    assert!(iter.next_record_into(&mut record).unwrap());

    let before = ALLOC_COUNT.load(Ordering::Relaxed);
    while iter.next_record_into(&mut record).unwrap() {
        std::hint::black_box(&record);
    }
    let after = ALLOC_COUNT.load(Ordering::Relaxed);
    after - before
}

/// Both reader paths are measured inside a **single** test so the shared
/// counting global allocator is never read while another test thread is
/// allocating on it — parallel `#[test]`s would otherwise contaminate each
/// other's deltas (the harness runs test fns concurrently). This mirrors the
/// single-test design of `scoring_allocations.rs`.
#[test]
fn reader_paths_hold_their_per_record_allocation_ceilings() {
    let config = TrainingDataConfig::new(NUM_INPUTS, NUM_OUTPUTS);
    let dir = tempfile::tempdir().unwrap();
    let small_path = dir.path().join("small.bin");
    let large_path = dir.path().join("large.bin");
    write_corpus(&small_path, SMALL);
    write_corpus(&large_path, LARGE);

    // Warm up any one-off lazy initialisation so it is not counted below.
    std::hint::black_box(read_file(&small_path, &config).unwrap());

    let extra_records = LARGE - SMALL;

    // -- Batch path: ≤2 allocations per record --------------------------------
    let small_batch = read_file_allocs(&small_path, &config);
    let large_batch = read_file_allocs(&large_path, &config);
    let batch_delta = large_batch - small_batch;

    // Per-record cost is exactly two `Vec`s (inputs + outputs). Anything above
    // ~2 per record means the intermediate `Vec` and/or the discarded copy has
    // been reintroduced. Small constant slack covers the once-per-call
    // whole-file read buffer and the result vector.
    assert!(
        batch_delta <= 2 * extra_records + 8,
        "read_file allocated {batch_delta} for {extra_records} extra records \
         (small={small_batch}, large={large_batch}); expected ≤2 per record — \
         an intermediate Vec or discarded copy has regressed"
    );
    // Sanity: it really does allocate the two per-record buffers (guards against
    // the assertion passing vacuously if the reader stopped producing records).
    assert!(
        batch_delta >= extra_records,
        "read_file allocated only {batch_delta} for {extra_records} extra \
         records — expected ~2 per record"
    );

    // -- Streaming path: allocation-free per record ---------------------------
    let small_dir = tempfile::tempdir().unwrap();
    let large_dir = tempfile::tempdir().unwrap();
    write_corpus(&small_dir.path().join("0.bin"), SMALL);
    write_corpus(&large_dir.path().join("0.bin"), LARGE);

    let small_stream = streaming_allocs(small_dir.path(), &config);
    let large_stream = streaming_allocs(large_dir.path(), &config);
    let stream_delta = large_stream.saturating_sub(small_stream);

    // 1000 extra records must add ~no allocations: the input/output buffers are
    // reused in place. A generous ceiling distinguishes "constant" from "grows
    // with records" without being brittle to allocator/test-harness noise.
    assert!(
        stream_delta < 50,
        "next_record_into allocated {stream_delta} extra for {extra_records} \
         extra records (small={small_stream}, large={large_stream}); expected \
         allocation-free per record"
    );
}
