//! Data-parallel record scoring (Issue #179).
//!
//! Scoring a production-size dataset pushes many records through one large
//! creature — an embarrassingly parallel workload *across records*. The
//! per-record forward path ([`CompiledNetwork::activate`]) is single-threaded,
//! so on a multi-core host most of the machine sits idle. This module adds an
//! opt-in, native-only parallel scoring path that chunks records across the
//! `rayon` thread pool.
//!
//! # Feature gating
//!
//! The parallel path is gated `#[cfg(all(feature = "parallel", not(target_arch
//! = "wasm32")))]`. `rayon` is declared as a `cfg(not(wasm32))` optional
//! dependency, so:
//!
//! - the **default** build pulls in no `rayon` symbols (feature off), and
//! - the **`wasm32`** build is completely unaffected — it keeps the existing
//!   single-thread behaviour even if the feature is requested.
//!
//! When the feature is off (or on wasm), [`CompiledNetwork::score_records_parallel`]
//! transparently falls back to the sequential [`CompiledNetwork::score_records`].
//!
//! # Determinism
//!
//! Results are **identical** to the sequential path regardless of thread count:
//!
//! - Every record is scored by the same [`CompiledNetwork::activate_into`] call
//!   used sequentially. `activate_into` overwrites every non-input activation
//!   each call, so there is no cross-record state.
//! - No `&mut self` is shared across threads. `CompiledNetwork` is `Clone`, and
//!   the clone carries the per-call scratch buffers (`activations`,
//!   `hint_values_buffer`, the 4-way batch buffers). The immutable weights are
//!   read through `&self`; each rayon worker initialises **its own** cloned
//!   scratch context via `for_each_init` and owns its buffers for the duration.
//! - Output order matches input order — the flat output buffer is split into
//!   disjoint per-record chunks by index (`par_chunks_mut(num_outputs)` zipped
//!   with `par_iter()`), so record `i` always writes `[i * num_outputs ..]`.
//!
//! # Output layout (Issue #229)
//!
//! Both paths return one contiguous `Vec<f32>` of `records.len() * num_outputs`
//! elements rather than a `Vec<Vec<f32>>`. Record `i`'s outputs occupy
//! `[i * num_outputs .. (i + 1) * num_outputs]`. This flat buffer is written
//! through [`CompiledNetwork::activate_into`], so the batch performs a **single**
//! output allocation instead of one heap allocation per record.

use crate::batch_scoring::BatchScratch;
use crate::network::CompiledNetwork;

/// Records per rayon task on the parallel path. A multiple of 8 so every task's
/// interior runs full 8-record SIMD batches (only the final task's tail can be
/// short), and small enough that a 2048-record batch splits into many chunks for
/// work-stealing balance across cores.
#[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
const PARALLEL_CHUNK_RECORDS: usize = 64;

impl CompiledNetwork {
    /// Score every record sequentially, returning a single flat output buffer.
    ///
    /// The result is one contiguous `Vec<f32>` of `records.len() * num_outputs`
    /// elements: record `i`'s outputs occupy `[i * num_outputs .. (i + 1) *
    /// num_outputs]`.
    ///
    /// The batch is driven through the across-records SIMD path (Issue #230):
    /// records are grouped into 8s (then a 4-record group, then a scalar tail)
    /// and forwarded through the batched `weighted_sum_simd_8records` /
    /// `weighted_sum_simd_4records` kernels, loading each synapse weight once and
    /// applying it across the lanes. The output layout and the single output
    /// allocation (Issue #229) are unchanged; standard-squash results match the
    /// per-record reference within a small `f32` tolerance (see
    /// [`crate::batch_scoring`]).
    ///
    /// This is the fallback used when the `parallel` feature is off or when
    /// building for `wasm32`, and the reference path the parallel results must
    /// match exactly.
    pub fn score_records(&self, records: &[Vec<f32>], num_outputs: usize) -> Vec<f32> {
        let mut outputs = vec![0.0f32; records.len() * num_outputs];
        let mut scratch = BatchScratch::new(self.num_neurons);
        self.score_batch_into(&mut scratch, records, num_outputs, &mut outputs);
        outputs
    }

    /// Score every record across the `rayon` thread pool, writing into a single
    /// flat output buffer in input order.
    ///
    /// Layout matches [`CompiledNetwork::score_records`]: record `i`'s outputs
    /// live in `[i * num_outputs .. (i + 1) * num_outputs]`. Records are split
    /// into fixed-size chunks (`PARALLEL_CHUNK_RECORDS`) across the rayon pool;
    /// each worker initialises its own [`BatchScratch`] via `for_each_init` and
    /// drives its chunk through the same batched forward pass
    /// ([`CompiledNetwork::score_batch_into`]) the sequential path uses, writing
    /// only its own disjoint output slice. No `&mut self` is shared: immutable
    /// weights are read through `&self` while every worker owns its lane
    /// buffers. Because each chunk boundary is a multiple of the batch size and
    /// the forward pass carries no cross-record state, results are identical to
    /// the sequential path regardless of thread count.
    ///
    /// Available with the `parallel` feature on native targets.
    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    pub fn score_records_parallel(&self, records: &[Vec<f32>], num_outputs: usize) -> Vec<f32> {
        use rayon::prelude::*;
        let mut outputs = vec![0.0f32; records.len() * num_outputs];
        outputs
            .par_chunks_mut(PARALLEL_CHUNK_RECORDS * num_outputs)
            .zip(records.par_chunks(PARALLEL_CHUNK_RECORDS))
            .for_each_init(
                || BatchScratch::new(self.num_neurons),
                |scratch, (out_chunk, rec_chunk)| {
                    self.score_batch_into(scratch, rec_chunk, num_outputs, out_chunk)
                },
            );
        outputs
    }

    /// Sequential fallback for [`CompiledNetwork::score_records_parallel`] when
    /// the `parallel` feature is disabled or building for `wasm32` (where
    /// `rayon` is unavailable). Same signature and identical results to the
    /// feature-on path — just single-threaded.
    #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
    pub fn score_records_parallel(&self, records: &[Vec<f32>], num_outputs: usize) -> Vec<f32> {
        self.score_records(records, num_outputs)
    }
}
