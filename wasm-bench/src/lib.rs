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
//!   `weighted_sum_simd_unchecked` call per non-input neuron over the creature's real
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
//!
//! Issue #736 — no export unwinds across the C ABI (that is undefined
//! behaviour). Every body runs inside [`guarded`], and a misuse or a panic
//! comes back as a sentinel instead: [`FAILED`] for the `u32` exports, `NaN`
//! for the `f64` ones. The driver (`runner.mjs`) throws on either, so a
//! sentinel never reaches a result row.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};

use neat_core::network::CompiledNetwork;
use neat_core::simd::weighted_sum_simd_unchecked;

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

/// Sentinel every `u32` export returns on failure (Issue #736). No real count
/// reaches it, and JS reads it as `-1` (`>>> 0` gives `0xFFFFFFFF`).
pub const FAILED: u32 = u32::MAX;

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

/// Run an export body without letting a panic unwind across the C ABI
/// (Issue #736). `None` — a refused call — and a caught panic both answer
/// `sentinel`, which the driver treats as a failure.
fn guarded<R>(sentinel: R, body: impl FnOnce() -> Option<R>) -> R {
    catch_unwind(AssertUnwindSafe(body))
        .ok()
        .flatten()
        .unwrap_or(sentinel)
}

/// Borrow the fixture, or `None` when `setup()` has not built one.
///
/// Fail loud (Issue #3234): benchmarking an unbuilt fixture would report a
/// meaningless zero, so the caller turns `None` into its sentinel instead.
fn with_fixture<R>(f: impl FnOnce(&mut Fixture) -> R) -> Option<R> {
    FIXTURE.with(|cell| cell.borrow_mut().as_mut().map(f))
}

/// Build the fixture for `NETWORKS[shape]` with `records` input records.
/// Returns the creature's synapse count so the driver can report the topology,
/// or [`FAILED`] for a `shape` outside `NETWORKS` (the previous fixture is
/// dropped either way, so a refused setup leaves nothing to benchmark).
///
/// # Safety
///
/// Plain C ABI export with scalar arguments; no pointers cross the boundary.
#[unsafe(no_mangle)]
pub extern "C" fn setup(shape: u32, records: u32) -> u32 {
    FIXTURE.with(|cell| *cell.borrow_mut() = None);
    guarded(FAILED, || {
        let spec = NETWORKS.get(usize::try_from(shape).ok()?)?;
        let net = build_network(spec, SEED);
        let stride = net.num_inputs();
        let records = build_records(stride, records as usize);
        let flat: Vec<f32> = records.iter().flatten().copied().collect();
        let num_outputs = spec.num_outputs;
        let synapses = u32::try_from(net.synapses().len()).ok()?;

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
        Some(synapses)
    })
}

/// Total neurons in the fixture creature, or [`FAILED`] before `setup()`.
#[unsafe(no_mangle)]
pub extern "C" fn neuron_count() -> u32 {
    guarded(FAILED, || with_fixture(|f| f.net.num_neurons() as u32))
}

/// Input arity of the fixture creature, or [`FAILED`] before `setup()`.
#[unsafe(no_mangle)]
pub extern "C" fn input_count() -> u32 {
    guarded(FAILED, || with_fixture(|f| f.stride as u32))
}

/// Records held by the fixture, or [`FAILED`] before `setup()`.
#[unsafe(no_mangle)]
pub extern "C" fn record_count() -> u32 {
    guarded(FAILED, || with_fixture(|f| f.records.len() as u32))
}

/// Re-seed the activation buffer from record 0, so a following `bench_kernel`
/// sums the same numbers every time. Called outside the driver's timed region.
/// Returns `0`, or [`FAILED`] before `setup()` or when the fixture holds no
/// records.
#[unsafe(no_mangle)]
pub extern "C" fn seed_activations() -> u32 {
    guarded(FAILED, || {
        with_fixture(|f| {
            let record = f.records.first()?.clone();
            let mut out = std::mem::take(&mut f.out);
            f.net.activate_into(&record, &mut out);
            f.out = out;
            Some(0)
        })
        .flatten()
    })
}

/// Isolated kernel: every non-input neuron's synapse span through
/// `weighted_sum_simd_unchecked`, [`KERNEL_REPS`] times. Returns the checksum,
/// or `NaN` before `setup()`.
///
/// Issue #613 - the `_unchecked` form is what the forward pass runs; the safe
/// `weighted_sum_simd` of the same name adds an `O(end - start)` bounds
/// pre-pass for callers holding no loaded network, which would measure
/// something this harness is not about.
#[unsafe(no_mangle)]
pub extern "C" fn bench_kernel() -> f64 {
    guarded(f64::NAN, || {
        with_fixture(|f| {
            let net = &f.net;
            let mut checksum = 0.0f64;
            for _ in 0..KERNEL_REPS {
                for neuron in net.neurons() {
                    let start = neuron.start_synapse as usize;
                    let end = start + neuron.num_synapses as usize;
                    // SAFETY: `net` came from `CompiledNetwork::new`, which rejects
                    // any `from_index >= num_neurons`, and `activations` is sized to
                    // `num_neurons`.
                    checksum += unsafe {
                        weighted_sum_simd_unchecked(
                            net.synapses(),
                            net.activations(),
                            start,
                            end,
                            neuron.bias,
                        )
                    } as f64;
                }
            }
            checksum
        })
    })
}

/// End-to-end single-record inference over every record. Returns the checksum,
/// or `NaN` before `setup()`.
#[unsafe(no_mangle)]
pub extern "C" fn bench_activate() -> f64 {
    guarded(f64::NAN, || {
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
    })
}

/// End-to-end batched scoring over the whole flat record buffer. Returns the
/// checksum, or `NaN` before `setup()`.
#[unsafe(no_mangle)]
pub extern "C" fn bench_score() -> f64 {
    guarded(f64::NAN, || {
        with_fixture(|f| {
            let out = f.net.score_records_flat(&f.flat, f.stride, f.num_outputs);
            out.iter().map(|v| *v as f64).sum()
        })
    })
}

#[cfg(test)]
mod tests {
    //! Issue #736: no export may unwind across the C ABI. Each misuse must come
    //! back as the documented sentinel so the driver can fail loud.
    use super::*;

    /// Start every test from "setup() never called", whatever ran before on
    /// this thread.
    fn clear_fixture() {
        FIXTURE.with(|cell| *cell.borrow_mut() = None);
    }

    #[test]
    fn setup_with_an_out_of_range_shape_returns_the_sentinel() {
        clear_fixture();
        assert_eq!(setup(NETWORKS.len() as u32, 8), FAILED);
        assert_eq!(setup(u32::MAX, 8), FAILED);
        // A refused setup leaves no fixture behind to benchmark.
        assert_eq!(record_count(), FAILED);
    }

    #[test]
    fn count_exports_before_setup_return_the_sentinel() {
        clear_fixture();
        assert_eq!(neuron_count(), FAILED);
        assert_eq!(input_count(), FAILED);
        assert_eq!(record_count(), FAILED);
        assert_eq!(seed_activations(), FAILED);
    }

    #[test]
    fn bench_exports_before_setup_return_nan() {
        clear_fixture();
        assert!(bench_kernel().is_nan());
        assert!(bench_activate().is_nan());
        assert!(bench_score().is_nan());
    }

    #[test]
    fn a_panicking_body_answers_the_sentinel_instead_of_unwinding() {
        assert_eq!(
            guarded(FAILED, || -> Option<u32> { panic!("boom") }),
            FAILED
        );
        assert!(guarded(f64::NAN, || -> Option<f64> { panic!("boom") }).is_nan());
    }

    #[test]
    fn a_panic_inside_a_public_export_answers_the_sentinel() {
        clear_fixture();
        assert_eq!(
            setup(0, 8),
            build_network(&NETWORKS[0], SEED).synapses().len() as u32
        );
        // Holding the fixture borrow makes the export's own `borrow_mut` panic
        // (`BorrowMutError`) — a real panic raised inside the export body.
        FIXTURE.with(|cell| {
            let _held = cell.borrow_mut();
            assert_eq!(neuron_count(), FAILED);
            assert!(bench_score().is_nan());
        });
        // The guard left the fixture intact: the next call succeeds.
        assert_eq!(record_count(), 8);
    }

    #[test]
    fn seeding_a_fixture_with_no_records_returns_the_sentinel() {
        clear_fixture();
        let spec = &NETWORKS[0];
        let synapses = build_network(spec, SEED).synapses().len() as u32;
        assert_eq!(setup(0, 0), synapses);
        assert_eq!(record_count(), 0);
        assert_eq!(seed_activations(), FAILED);
    }

    #[test]
    fn a_valid_setup_reports_the_real_topology_and_finite_checksums() {
        clear_fixture();
        let spec = &NETWORKS[0];
        let expected = build_network(spec, SEED);
        assert_eq!(setup(0, 8), expected.synapses().len() as u32);
        assert_eq!(neuron_count(), expected.num_neurons() as u32);
        assert_eq!(input_count(), expected.num_inputs() as u32);
        assert_eq!(record_count(), 8);
        assert_eq!(seed_activations(), 0);
        assert!(bench_kernel().is_finite());
        assert!(bench_activate().is_finite());
        assert!(bench_score().is_finite());
    }
}
