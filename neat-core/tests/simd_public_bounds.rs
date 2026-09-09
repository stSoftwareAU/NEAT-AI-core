//! Issue #613 — the safe public SIMD surface must not read out of bounds.
//!
//! Every kernel re-exported from `neat_core::simd` is a **safe** `pub fn`, so by
//! Rust's soundness convention no combination of safe arguments may trigger
//! undefined behaviour. Before this fix their bodies reached
//! `activations.get_unchecked(from_index)` on the strength of a precondition
//! only `CompiledNetwork::new` upheld, which left any caller that does not hold
//! a loaded network able to drive an out-of-bounds read from entirely safe code.
//!
//! Two obligations make up that precondition and both are covered here: every
//! `from_index` in `start..end` indexes the activation buffer, and
//! `end <= synapses.len()`. Each is now refused with a panic rather than read
//! unchecked, so `expect_refusal` is the shape of every regression test below —
//! against the unfixed code those calls read past the buffer instead (observed
//! as SIGABRT / SIGSEGV on the aarch64 NEON host this was verified on).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Once;

use neat_core::network::{SynapseData, hot_synapse_soa};
use neat_core::simd::bounds::{interleaved_span_in_bounds, span_in_bounds, span_in_bounds_multi};
use neat_core::simd::{
    weighted_sum_interleaved_8, weighted_sum_interleaved_8_unchecked, weighted_sum_no_bias_simd,
    weighted_sum_no_bias_simd_unchecked, weighted_sum_of_squares_simd,
    weighted_sum_of_squares_simd_unchecked, weighted_sum_of_squares_v2_simd,
    weighted_sum_of_squares_v2_simd_unchecked, weighted_sum_simd, weighted_sum_simd_4records,
    weighted_sum_simd_4records_unchecked, weighted_sum_simd_8records,
    weighted_sum_simd_8records_unchecked, weighted_sum_simd_unchecked,
};

fn synapse(from_index: u16, weight: f32) -> SynapseData {
    SynapseData {
        weight,
        from_index,
        synapse_type: 0,
    }
}

/// The issue's reproducer: eight synapses all sourcing neuron 9,999 — enough to
/// drive the 8-wide chunk, the 4-wide chunk and the scalar tail of every
/// single-record kernel.
fn out_of_range_span() -> (Vec<SynapseData>, Vec<f32>) {
    (vec![synapse(9_999, 1.0); 8], vec![0.25_f32; 1])
}

/// A well-formed span: `from_index` 0..8 against an 8-long activation buffer.
fn valid_span() -> (Vec<SynapseData>, Vec<f32>) {
    (
        (0..8).map(|i| synapse(i, 0.5)).collect(),
        (0..8).map(|i| i as f32).collect(),
    )
}

/// Silence panic output for the whole test binary, once.
///
/// Installed through a [`Once`] rather than taken and restored per call: the
/// harness runs these tests concurrently on separate threads, and the panic
/// hook is process-global, so take/restore pairs would interleave and one test
/// could restore the hook while another is inside `catch_unwind`.
fn silence_panics() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| std::panic::set_hook(Box::new(|_| {})));
}

/// Run `f` and assert it was refused, rather than answering from a read outside
/// the buffer.
fn expect_refusal<T>(what: &str, f: impl FnOnce() -> T) {
    silence_panics();
    let outcome = catch_unwind(AssertUnwindSafe(f));
    assert!(
        outcome.is_err(),
        "{what} accepted an out-of-bounds span instead of failing loud"
    );
}

// ---------------------------------------------------------------------------
// The safe entry points, and the two refusals each one owes
// ---------------------------------------------------------------------------

/// One safe entry point under test, with the name each of its two refusal
/// cases is reported and cited under.
///
/// The six kernels differ only in arity and in the scalars they carry, so
/// `call` adapts each to one shape — `synapses[0..end]` against `acts`, answer
/// discarded, because a call that reaches this table must not produce one.
/// Driving both obligations from a single table is what stops a kernel being
/// added with only half of the unchecked contract covered.
struct SafeKernel {
    /// Case name for the out-of-range-`from_index` refusal — the issue's
    /// reproducer, run by
    /// [`every_safe_kernel_rejects_an_out_of_range_from_index`].
    name: &'static str,
    /// Case name for the `end > synapses.len()` refusal, run by
    /// [`every_safe_kernel_rejects_a_span_past_the_synapse_slice`].
    span_name: &'static str,
    /// Calls the safe entry point over `synapses[0..end]` against `acts`,
    /// passing `acts` for every record of the multi-record kernels.
    call: fn(&[SynapseData], &[f32], usize),
}

/// Every safe entry point that reads `synapses[start..end]` against plain
/// activation buffers. `weighted_sum_interleaved_8` is not here: its contract
/// is over the hot SoA arrays and a tile-major buffer, so it keeps the two
/// bespoke cases below.
const SAFE_KERNELS: &[SafeKernel] = &[
    SafeKernel {
        name: "weighted_sum_simd_rejects_out_of_range_from_index",
        span_name: "weighted_sum_simd_rejects_a_span_past_the_synapse_slice",
        call: |synapses, acts, end| {
            weighted_sum_simd(synapses, acts, 0, end, 0.0);
        },
    },
    SafeKernel {
        name: "weighted_sum_no_bias_simd_rejects_out_of_range_from_index",
        span_name: "weighted_sum_no_bias_simd_rejects_a_span_past_the_synapse_slice",
        call: |synapses, acts, end| {
            weighted_sum_no_bias_simd(synapses, acts, 0, end);
        },
    },
    SafeKernel {
        name: "weighted_sum_of_squares_simd_rejects_out_of_range_from_index",
        span_name: "weighted_sum_of_squares_simd_rejects_a_span_past_the_synapse_slice",
        call: |synapses, acts, end| {
            weighted_sum_of_squares_simd(synapses, acts, 0, end);
        },
    },
    SafeKernel {
        name: "weighted_sum_of_squares_v2_simd_rejects_out_of_range_from_index",
        span_name: "weighted_sum_of_squares_v2_simd_rejects_a_span_past_the_synapse_slice",
        call: |synapses, acts, end| {
            weighted_sum_of_squares_v2_simd(synapses, acts, 0, end, 0.5);
        },
    },
    SafeKernel {
        name: "weighted_sum_simd_4records_rejects_out_of_range_from_index",
        span_name: "weighted_sum_simd_4records_rejects_a_span_past_the_synapse_slice",
        call: |synapses, acts, end| {
            weighted_sum_simd_4records(synapses, acts, acts, acts, acts, 0, end, 0.0);
        },
    },
    SafeKernel {
        name: "weighted_sum_simd_8records_rejects_out_of_range_from_index",
        span_name: "weighted_sum_simd_8records_rejects_a_span_past_the_synapse_slice",
        call: |synapses, acts, end| {
            weighted_sum_simd_8records(
                synapses, acts, acts, acts, acts, acts, acts, acts, acts, 0, end, 0.0,
            );
        },
    },
];

// ---------------------------------------------------------------------------
// Obligation 1: every `from_index` in `start..end` indexes the activation buffer
// ---------------------------------------------------------------------------

/// The issue's reproducer, run against every safe entry point: eight synapses
/// sourcing neuron 9,999 against a 1-long activation buffer must be refused,
/// not read unchecked. Against the unfixed code each of these calls reads past
/// the buffer instead.
#[test]
fn every_safe_kernel_rejects_an_out_of_range_from_index() {
    let (synapses, activations) = out_of_range_span();
    assert!(!span_in_bounds(
        &synapses,
        0,
        synapses.len(),
        activations.len()
    ));
    for kernel in SAFE_KERNELS {
        expect_refusal(kernel.name, || {
            (kernel.call)(&synapses, &activations, synapses.len())
        });
    }
}

#[test]
fn weighted_sum_interleaved_8_rejects_out_of_range_from_index() {
    let (synapses, _) = out_of_range_span();
    let (hot_weights, hot_from) = hot_synapse_soa(&synapses);
    // One neuron's worth of 8-lane tile; every `from_index` is 9,999.
    let inter = vec![0.25_f32; 8];
    expect_refusal("weighted_sum_interleaved_8", || {
        weighted_sum_interleaved_8(&hot_weights, &hot_from, &inter, 0, hot_from.len(), 0.0)
    });
}

/// A multi-record caller passing seven long buffers and one short one must not
/// slip through: the predicate binds on the shortest.
#[test]
fn weighted_sum_simd_8records_rejects_one_short_buffer() {
    let (synapses, _) = valid_span();
    let long = vec![1.0_f32; 8];
    let short = vec![1.0_f32; 4];
    expect_refusal("weighted_sum_simd_8records (one short buffer)", || {
        weighted_sum_simd_8records(
            &synapses, &long, &long, &long, &long, &long, &short, &long, &long, 0, 8, 0.0,
        )
    });
}

// ---------------------------------------------------------------------------
// Obligation 2: `end <= synapses.len()`
// ---------------------------------------------------------------------------

/// The second half of the unchecked contract, run against every safe entry
/// point. The scalar reference would silently truncate this span and answer
/// from the synapses that do exist, so each kernel refuses it instead of
/// returning a plausible number for a span the caller never had.
#[test]
fn every_safe_kernel_rejects_a_span_past_the_synapse_slice() {
    let (synapses, activations) = valid_span();
    assert!(!span_in_bounds(&synapses, 0, 32, activations.len()));
    for kernel in SAFE_KERNELS {
        expect_refusal(kernel.span_name, || {
            (kernel.call)(&synapses, &activations, 32)
        });
    }
}

#[test]
fn weighted_sum_interleaved_8_rejects_a_span_past_the_hot_arrays() {
    let (synapses, _) = valid_span();
    let (hot_weights, hot_from) = hot_synapse_soa(&synapses);
    let inter = vec![0.25_f32; 8 * 8];
    expect_refusal(
        "weighted_sum_interleaved_8 (end past the hot arrays)",
        || weighted_sum_interleaved_8(&hot_weights, &hot_from, &inter, 0, 32, 0.0),
    );
}

// ---------------------------------------------------------------------------
// Happy path: the pre-pass changes no answer
// ---------------------------------------------------------------------------

/// The safe entry point must be **bit-identical** to its `*_unchecked` twin on
/// any span that satisfies the precondition — the pre-pass decides whether to
/// dispatch, and changes nothing about the arithmetic.
#[test]
fn safe_entry_points_match_their_unchecked_twins_on_valid_spans() {
    let (synapses, activations) = valid_span();
    let acts = &activations;
    for end in 0..=synapses.len() {
        assert!(span_in_bounds(&synapses, 0, end, acts.len()));

        // SAFETY: the span was just checked in bounds by `span_in_bounds`.
        let want = unsafe { weighted_sum_simd_unchecked(&synapses, acts, 0, end, 1.0) };
        assert_eq!(
            weighted_sum_simd(&synapses, acts, 0, end, 1.0).to_bits(),
            want.to_bits(),
            "weighted_sum_simd drifted from its unchecked twin at end={end}"
        );

        // SAFETY: as above.
        let want = unsafe { weighted_sum_no_bias_simd_unchecked(&synapses, acts, 0, end) };
        assert_eq!(
            weighted_sum_no_bias_simd(&synapses, acts, 0, end).to_bits(),
            want.to_bits(),
            "weighted_sum_no_bias_simd drifted at end={end}"
        );

        // SAFETY: as above.
        let want = unsafe { weighted_sum_of_squares_simd_unchecked(&synapses, acts, 0, end) };
        assert_eq!(
            weighted_sum_of_squares_simd(&synapses, acts, 0, end).to_bits(),
            want.to_bits(),
            "weighted_sum_of_squares_simd drifted at end={end}"
        );

        // SAFETY: as above.
        let want =
            unsafe { weighted_sum_of_squares_v2_simd_unchecked(&synapses, acts, 0, end, -0.5) };
        assert_eq!(
            weighted_sum_of_squares_v2_simd(&synapses, acts, 0, end, -0.5).to_bits(),
            want.to_bits(),
            "weighted_sum_of_squares_v2_simd drifted at end={end}"
        );

        // SAFETY: as above.
        let want = unsafe {
            weighted_sum_simd_4records_unchecked(&synapses, acts, acts, acts, acts, 0, end, 0.25)
        };
        assert_eq!(
            weighted_sum_simd_4records(&synapses, acts, acts, acts, acts, 0, end, 0.25),
            want,
            "weighted_sum_simd_4records drifted at end={end}"
        );

        // SAFETY: as above.
        let want = unsafe {
            weighted_sum_simd_8records_unchecked(
                &synapses, acts, acts, acts, acts, acts, acts, acts, acts, 0, end, 0.25,
            )
        };
        assert_eq!(
            weighted_sum_simd_8records(
                &synapses, acts, acts, acts, acts, acts, acts, acts, acts, 0, end, 0.25,
            ),
            want,
            "weighted_sum_simd_8records drifted at end={end}"
        );
    }
}

#[test]
fn interleaved_safe_entry_point_matches_its_unchecked_twin() {
    let (synapses, _) = valid_span();
    let (hot_weights, hot_from) = hot_synapse_soa(&synapses);
    let inter: Vec<f32> = (0..8 * 8).map(|i| i as f32 * 0.125).collect();
    for end in 0..=hot_from.len() {
        // SAFETY: `hot_from` holds 0..8 and `inter` is 8 neurons × 8 lanes.
        let want = unsafe {
            weighted_sum_interleaved_8_unchecked(&hot_weights, &hot_from, &inter, 0, end, 0.5)
        };
        assert_eq!(
            weighted_sum_interleaved_8(&hot_weights, &hot_from, &inter, 0, end, 0.5),
            want,
            "weighted_sum_interleaved_8 drifted from its unchecked twin at end={end}"
        );
    }
}

// ---------------------------------------------------------------------------
// The predicates themselves
// ---------------------------------------------------------------------------

/// An empty or reversed span reads nothing, so it is in bounds whatever the
/// buffers hold — the saturating `scalar::synapse_count` guard returns before
/// the first load.
#[test]
fn empty_and_reversed_spans_are_in_bounds() {
    let synapses = vec![synapse(9_999, 1.0); 4];
    assert!(span_in_bounds(&synapses, 0, 0, 0));
    assert!(span_in_bounds(&synapses, 9, 4, 0));
    assert_eq!(weighted_sum_simd(&synapses, &[], 9, 4, 0.25), 0.25);
    assert_eq!(weighted_sum_simd(&synapses, &[], 0, 0, 0.25), 0.25);
}

#[test]
fn span_in_bounds_rejects_both_halves_of_the_contract() {
    let synapses = vec![synapse(0, 1.0), synapse(3, 1.0)];
    assert!(span_in_bounds(&synapses, 0, 2, 4));
    // `from_index = 3` does not index a 3-long buffer.
    assert!(!span_in_bounds(&synapses, 0, 2, 3));
    // `end` past the synapse slice.
    assert!(!span_in_bounds(&synapses, 0, 3, 4));
}

/// The multi-record predicate must bind on the **shortest** activation buffer,
/// not the first one it is handed.
#[test]
fn multi_record_predicate_binds_on_the_shortest_buffer() {
    let synapses = vec![synapse(3, 1.0); 4];
    assert!(span_in_bounds_multi(&synapses, 0, 4, &[4, 4, 4, 4]));
    assert!(!span_in_bounds_multi(&synapses, 0, 4, &[4, 4, 2, 4]));
    assert!(!span_in_bounds_multi(&synapses, 0, 4, &[]));
}

/// The interleaved predicate must reject a `from` whose tile runs past `inter`,
/// and must not wrap when `from * lanes` would overflow `usize`.
#[test]
fn interleaved_predicate_rejects_partial_and_overflowing_tiles() {
    let hot_weights = vec![1.0_f32; 2];
    let hot_from = vec![0_u16, 1];
    assert!(interleaved_span_in_bounds(
        &hot_weights,
        &hot_from,
        16,
        8,
        0,
        2
    ));
    // Neuron 1's tile needs `inter[8..16]`; a 12-float buffer only half covers it.
    assert!(!interleaved_span_in_bounds(
        &hot_weights,
        &hot_from,
        12,
        8,
        0,
        2
    ));
    // A span reaching past the hot arrays is rejected before any tile maths.
    assert!(!interleaved_span_in_bounds(
        &hot_weights,
        &hot_from,
        16,
        8,
        0,
        3
    ));
    // `usize::MAX` lanes would overflow the `from * lanes` product; checked
    // arithmetic must refuse rather than wrap into range.
    assert!(!interleaved_span_in_bounds(
        &hot_weights,
        &hot_from,
        16,
        usize::MAX,
        0,
        2
    ));
    // An empty span reads nothing, so it is in bounds.
    assert!(interleaved_span_in_bounds(
        &hot_weights,
        &hot_from,
        0,
        8,
        2,
        2
    ));
}
