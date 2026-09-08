//! Crate-boundary bounds validation for the public SIMD kernels (Issue #613).
//!
//! The SIMD weighted-sum kernels read the activation buffer with **unchecked**
//! indexing, which is sound only while every `from_index` in the span is a
//! valid index into that buffer. `CompiledNetwork::new` establishes exactly
//! that at load time (`AGENTS.md`, "Unsafe & SIMD invariants"), so the
//! forward-pass hot path can call the `*_unchecked` kernels with no per-call
//! cost.
//!
//! A caller that does **not** hold a loaded network has nothing establishing
//! the invariant, so the safe kernels re-exported from [`crate::simd`] run the
//! predicates below before dispatching. They are the one place the crate
//! decides whether a span may be read unchecked; each is `O(end - start)` and
//! is paid only by the safe entry points, never by the validated hot path.

use crate::network::SynapseData;

/// Fail loud when a safe kernel is handed a synapse span it may not read.
///
/// Reached only from the safe entry points, and only once a predicate below has
/// refused the span, so no valid caller can hit it. Panicking rather than
/// quietly answering from a truncated or partial span keeps a caller bug
/// visible instead of folding it into a plausible-looking number.
#[cold]
#[inline(never)]
pub fn reject_span(kernel: &str) -> ! {
    panic!(
        "{kernel}: out-of-bounds synapse span (Issue #613) — `end` must be <= the synapse \
         slice length, and every `from_index` in `start..end` must index the activation \
         buffer. `CompiledNetwork::new` upholds both for a loaded network."
    )
}

/// [`reject_span`] for the record-interleaved tile, whose contract is stated
/// over `inter` rather than a per-record activation buffer.
#[cold]
#[inline(never)]
pub fn reject_interleaved_span(kernel: &str) -> ! {
    panic!(
        "{kernel}: out-of-bounds interleaved tile (Issue #613) — `end` must be <= both hot \
         synapse arrays, and every `hot_from` entry in `start..end` must satisfy \
         `from * R + R <= inter.len()`. `CompiledNetwork::new` upholds both for a loaded \
         network."
    )
}

/// Whether `synapses[start..end]` may be read with unchecked indexing against
/// an activation buffer of `activation_len` entries.
///
/// An empty or reversed span reads nothing and is always in bounds, matching
/// the saturating [`crate::simd::scalar::synapse_count`] guard the kernels use.
#[inline]
#[must_use]
pub fn span_in_bounds(
    synapses: &[SynapseData],
    start: usize,
    end: usize,
    activation_len: usize,
) -> bool {
    if start >= end {
        return true;
    }
    if end > synapses.len() {
        return false;
    }
    synapses[start..end]
        .iter()
        .all(|s| (s.from_index as usize) < activation_len)
}

/// [`span_in_bounds`] for a multi-record kernel, whose one span indexes
/// **every** activation buffer — so the shortest buffer is the binding one.
#[inline]
#[must_use]
pub fn span_in_bounds_multi(
    synapses: &[SynapseData],
    start: usize,
    end: usize,
    activation_lens: &[usize],
) -> bool {
    let shortest = activation_lens.iter().copied().min().unwrap_or(0);
    span_in_bounds(synapses, start, end, shortest)
}

/// Whether the record-interleaved tile `hot_from[start..end]` may be read with
/// unchecked indexing against a transposed buffer of `inter_len` floats and a
/// tile width of `lanes`.
///
/// Each synapse reads `inter[from * lanes .. from * lanes + lanes]`, so the
/// product is computed with checked arithmetic. `lanes` is a caller-supplied
/// `usize` on this public predicate — the crate only ever passes a tile width
/// `R <= MAX_INTERLEAVED_LANES`, but a caller passing a large one must fail the
/// predicate rather than wrap the product back into range.
#[inline]
#[must_use]
pub fn interleaved_span_in_bounds(
    hot_weights: &[f32],
    hot_from: &[u16],
    inter_len: usize,
    lanes: usize,
    start: usize,
    end: usize,
) -> bool {
    if start >= end {
        return true;
    }
    if end > hot_weights.len() || end > hot_from.len() {
        return false;
    }
    hot_from[start..end].iter().all(|&from| {
        (from as usize)
            .checked_mul(lanes)
            .and_then(|base| base.checked_add(lanes))
            .is_some_and(|top| top <= inter_len)
    })
}
