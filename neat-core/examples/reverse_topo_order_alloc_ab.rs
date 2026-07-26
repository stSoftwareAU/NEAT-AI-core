//! Allocation + wall-clock A/B for `compute_reverse_topological_order` (Issue #388).
//!
//! Measures the shipped CSR implementation against the pre-#388
//! `Vec<Vec<u32>>` reference on the production anchor topology named in the
//! issue — 1,666 neurons carrying 21,513 synapses — under a counting global
//! allocator. Both sides must return the identical order; the point of the
//! change is the allocation count, not the answer.
//!
//! Run with: `cargo run --release --example reverse_topo_order_alloc_ab`

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use neat_core::topology_ops::compute_reverse_topological_order;

// ---------------------------------------------------------------------------
// Counting allocator — records allocation count and peak live bytes.
// ---------------------------------------------------------------------------

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: `layout` is forwarded unchanged to the system allocator,
        // which is the documented delegation pattern for a wrapper allocator.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: `ptr`/`layout` come from a matching `alloc` on this
        // allocator, which delegates to `System`.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn reset_counters() {
    ALLOCS.store(0, Ordering::Relaxed);
    LIVE.store(0, Ordering::Relaxed);
    PEAK.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Pre-#388 reference: one `Vec<u32>` per neuron.
// ---------------------------------------------------------------------------

fn reference_reverse_topological_order(
    from_indices: &[u32],
    to_indices: &[u32],
    num_neurons: u32,
    num_inputs: u32,
) -> Vec<u32> {
    let n = num_neurons as usize;
    let input_count = num_inputs as usize;

    if from_indices.len() != to_indices.len() {
        return Vec::new();
    }
    if input_count > n {
        return Vec::new();
    }

    let mut out_degree = vec![0i32; n];
    let mut inward: Vec<Vec<u32>> = vec![Vec::new(); n];

    for i in 0..from_indices.len() {
        let from = from_indices[i] as usize;
        let to = to_indices[i] as usize;

        if from == to || from >= n || to >= n {
            continue;
        }
        if from >= input_count {
            out_degree[from] += 1;
        }
        inward[to].push(from as u32);
    }

    let mut queue: Vec<usize> = Vec::new();
    for i in input_count..n {
        if out_degree[i] == 0 {
            queue.push(i);
        }
    }

    let mut result: Vec<u32> = Vec::new();
    let mut visited = vec![false; n];
    let mut head = 0;

    while head < queue.len() {
        let idx = queue[head];
        head += 1;

        if visited[idx] {
            continue;
        }
        visited[idx] = true;
        result.push(idx as u32);

        for j in 0..inward[idx].len() {
            let from = inward[idx][j] as usize;
            if from < input_count || visited[from] {
                continue;
            }
            out_degree[from] -= 1;
            if out_degree[from] <= 0 {
                queue.push(from);
            }
        }
    }

    for i in input_count..n {
        if !visited[i] {
            result.push(i as u32);
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Deterministic production-shaped topology: n = 1,666, 21,513 synapses.
// ---------------------------------------------------------------------------

struct Lcg(u64);

impl Lcg {
    fn next_below(&mut self, bound: usize) -> usize {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        if bound == 0 {
            0
        } else {
            (z % bound as u64) as usize
        }
    }
}

fn build_topology(n: usize, input_count: usize, num_synapses: usize) -> (Vec<u32>, Vec<u32>) {
    let mut rng = Lcg(0x7061_7468);
    let non_inputs = n - input_count;
    let base = num_synapses / non_inputs;
    let remainder = num_synapses % non_inputs;

    let mut from_indices = Vec::with_capacity(num_synapses);
    let mut to_indices = Vec::with_capacity(num_synapses);

    for offset in 0..non_inputs {
        let to = input_count + offset;
        let fan = (base + usize::from(offset < remainder)).min(to);
        for _ in 0..fan {
            from_indices.push(rng.next_below(to) as u32);
            to_indices.push(to as u32);
        }
    }

    (from_indices, to_indices)
}

fn main() {
    const N: usize = 1666;
    const INPUTS: usize = 100;
    const SYNAPSES: usize = 21_513;
    const REPS: usize = 100;
    const ROUNDS: usize = 12;

    let (from, to) = build_topology(N, INPUTS, SYNAPSES);
    println!(
        "topology: n = {N}, inputs = {INPUTS}, synapses = {}\n",
        from.len()
    );

    // --- Correctness: both implementations must agree exactly. ---
    let want = reference_reverse_topological_order(&from, &to, N as u32, INPUTS as u32);
    let got = compute_reverse_topological_order(&from, &to, N as u32, INPUTS as u32);
    assert_eq!(want, got, "CSR order must be element-identical");
    println!("orders element-identical: {} entries\n", got.len());

    // --- Allocation counts: one call each. ---
    reset_counters();
    let out = reference_reverse_topological_order(&from, &to, N as u32, INPUTS as u32);
    let ref_allocs = ALLOCS.load(Ordering::Relaxed);
    let ref_peak = PEAK.load(Ordering::Relaxed);
    std::hint::black_box(&out);

    reset_counters();
    let out = compute_reverse_topological_order(&from, &to, N as u32, INPUTS as u32);
    let csr_allocs = ALLOCS.load(Ordering::Relaxed);
    let csr_peak = PEAK.load(Ordering::Relaxed);
    std::hint::black_box(&out);

    // --- Wall clock: alternating rounds, reported as the *fastest* round on
    // each side. Min-of-rounds is the noise-robust statistic here — a shared
    // machine only ever adds time, so the minimum is the closest estimate of
    // the true cost and the two sides see the same interference. ---
    let mut ref_best = f64::INFINITY;
    let mut csr_best = f64::INFINITY;
    for _ in 0..ROUNDS {
        let start = Instant::now();
        for _ in 0..REPS {
            let out = reference_reverse_topological_order(&from, &to, N as u32, INPUTS as u32);
            std::hint::black_box(&out);
        }
        ref_best = ref_best.min(start.elapsed().as_secs_f64());

        let start = Instant::now();
        for _ in 0..REPS {
            let out = compute_reverse_topological_order(&from, &to, N as u32, INPUTS as u32);
            std::hint::black_box(&out);
        }
        csr_best = csr_best.min(start.elapsed().as_secs_f64());
    }

    println!(
        "{:<26} {:>20} {:>20}",
        "metric", "in-example Vec<Vec>", "neat-core (shipped)"
    );
    println!(
        "{:<26} {:>20} {:>20}",
        "allocations / call", ref_allocs, csr_allocs
    );
    println!(
        "{:<26} {:>20} {:>20}",
        "peak live bytes", ref_peak, csr_peak
    );
    println!(
        "{:<26} {:>19.3}µs {:>19.3}µs",
        "wall clock / call (best)",
        ref_best * 1e6 / REPS as f64,
        csr_best * 1e6 / REPS as f64,
    );
}
