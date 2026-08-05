//! WASM `gather4` prototype benchmark harness (Issue #509).
//!
//! Exposes the three benchmarks as plain C exports so the driver can hold the
//! **control and the experimental module side by side in one process** and
//! alternate between them sample by sample. That is what makes the comparison
//! survive a busy machine: both variants see the same scheduler, the same
//! thermal state and the same drift, instead of being separated by seconds of
//! process start-up.
//!
//! Benchmarks:
//!
//! - `bench_kernel` — the isolated `gather4`-driven kernel: one
//!   `weighted_sum_simd` call per non-input neuron over the creature's real
//!   synapse spans, repeated [`KERNEL_REPS`] times.
//! - `bench_activate` — end-to-end single-record inference (`activate_into`),
//!   the production forward pass that calls that kernel.
//! - `bench_score` — end-to-end batched scoring (`score_records_flat`), the
//!   production training-scoring entry point (mostly the 8-record kernels, so
//!   it doubles as the "no material regression elsewhere" check).
//!
//! Fixtures come from `neat-core/benches/common/mod.rs` verbatim, so the
//! topology, weights and records are exactly the ones the committed Criterion
//! baseline uses.

use std::cell::RefCell;

use neat_core::network::CompiledNetwork;
use neat_core::simd::weighted_sum_simd;

// Reused verbatim; the backprop fixtures it also carries are unused here.
#[allow(dead_code)]
#[path = "../../neat-core/benches/common/mod.rs"]
mod common;

use common::{NETWORKS, build_network, build_records};

/// Fixed PRNG seed — the same one the Criterion harness builds its networks
/// with, so this harness and `hot_paths` measure the identical creature.
const SEED: u64 = 0x5EED;

/// Repeats of the whole-creature kernel sweep inside one isolated-kernel
/// sample, so a sample is long enough to dwarf the driver's clock resolution.
const KERNEL_REPS: usize = 200;

struct Fixture {
    net: CompiledNetwork,
    records: Vec<Vec<f32>>,
    flat: Vec<f32>,
    stride: usize,
    num_outputs: usize,
    out: Vec<f32>,
}

thread_local! {
    static FIXTURE: RefCell<Option<Fixture>> = const { RefCell::new(None) };
}

fn with_fixture<R>(f: impl FnOnce(&mut Fixture) -> R) -> R {
    FIXTURE.with(|cell| {
        let mut slot = cell.borrow_mut();
        // Fail loud (Issue #3234): benchmarking an unbuilt fixture would report
        // a meaningless zero rather than an error.
        let fixture = slot
            .as_mut()
            .expect("setup() must be called before any bench_* export");
        f(fixture)
    })
}

/// Build the fixture for `NETWORKS[shape]` with `records` input records.
/// Returns the creature's synapse count so the driver can report the topology.
///
/// # Safety
///
/// Plain C ABI export with scalar arguments; no pointers cross the boundary.
#[unsafe(no_mangle)]
pub extern "C" fn setup(shape: u32, records: u32) -> u32 {
    let spec = &NETWORKS[shape as usize];
    let net = build_network(spec, SEED);
    let stride = net.num_inputs;
    let records = build_records(stride, records as usize);
    let flat: Vec<f32> = records.iter().flatten().copied().collect();
    let num_outputs = spec.num_outputs;
    let synapses = net.synapses.len() as u32;

    FIXTURE.with(|cell| {
        *cell.borrow_mut() = Some(Fixture {
            net,
            records,
            flat,
            stride,
            num_outputs,
            out: vec![0.0f32; num_outputs],
        });
    });
    synapses
}

/// Total neurons in the fixture creature.
#[unsafe(no_mangle)]
pub extern "C" fn neuron_count() -> u32 {
    with_fixture(|f| f.net.num_neurons as u32)
}

/// Input arity of the fixture creature.
#[unsafe(no_mangle)]
pub extern "C" fn input_count() -> u32 {
    with_fixture(|f| f.stride as u32)
}

/// Records held by the fixture.
#[unsafe(no_mangle)]
pub extern "C" fn record_count() -> u32 {
    with_fixture(|f| f.records.len() as u32)
}

/// Re-seed the activation buffer from record 0, so a following `bench_kernel`
/// sums the same numbers every time. Called outside the driver's timed region.
#[unsafe(no_mangle)]
pub extern "C" fn seed_activations() {
    with_fixture(|f| {
        let record = f.records[0].clone();
        let mut out = std::mem::take(&mut f.out);
        f.net.activate_into(&record, &mut out);
        f.out = out;
    });
}

/// Isolated kernel: every non-input neuron's synapse span through
/// `weighted_sum_simd`, [`KERNEL_REPS`] times. Returns the checksum.
#[unsafe(no_mangle)]
pub extern "C" fn bench_kernel() -> f64 {
    with_fixture(|f| {
        let net = &f.net;
        let mut checksum = 0.0f64;
        for _ in 0..KERNEL_REPS {
            for neuron in &net.neurons {
                let start = neuron.start_synapse as usize;
                let end = start + neuron.num_synapses as usize;
                checksum +=
                    weighted_sum_simd(&net.synapses, &net.activations, start, end, neuron.bias)
                        as f64;
            }
        }
        checksum
    })
}

/// End-to-end single-record inference over every record. Returns the checksum.
#[unsafe(no_mangle)]
pub extern "C" fn bench_activate() -> f64 {
    with_fixture(|f| {
        let mut out = std::mem::take(&mut f.out);
        let mut checksum = 0.0f64;
        for record in &f.records {
            f.net.activate_into(record, &mut out);
            for value in &out {
                checksum += *value as f64;
            }
        }
        f.out = out;
        checksum
    })
}

/// End-to-end batched scoring over the whole flat record buffer. Returns the
/// checksum.
#[unsafe(no_mangle)]
pub extern "C" fn bench_score() -> f64 {
    with_fixture(|f| {
        let out = f
            .net
            .score_records_flat(&f.flat, f.stride, f.num_outputs);
        out.iter().map(|v| *v as f64).sum()
    })
}
