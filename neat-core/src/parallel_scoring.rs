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
//! - Every record is scored by the same [`CompiledNetwork::activate`] call used
//!   sequentially. `activate` overwrites every non-input activation each call,
//!   so there is no cross-record state.
//! - No `&mut self` is shared across threads. `CompiledNetwork` is `Clone`, and
//!   the clone carries the per-call scratch buffers (`activations`,
//!   `hint_values_buffer`, the 4-way batch buffers). The immutable weights are
//!   read through `&self`; each rayon worker initialises **its own** cloned
//!   scratch context via `map_init` and owns its buffers for the duration.
//! - Output order matches input order — `par_iter().map(...).collect()`
//!   preserves indexed order.

use crate::network::CompiledNetwork;

impl CompiledNetwork {
    /// Score every record sequentially, returning one output vector per record.
    ///
    /// Shared read-only weights (`&self`) plus a single owned scratch context
    /// (one clone of the network). This is the fallback used when the `parallel`
    /// feature is off or when building for `wasm32`, and the reference path the
    /// parallel results must match exactly.
    ///
    /// Each record is scored with [`CompiledNetwork::activate`]; the first
    /// `num_outputs` … (`num_neurons - num_outputs ..`) activations are returned.
    pub fn score_records(&self, records: &[Vec<f32>], num_outputs: usize) -> Vec<Vec<f32>> {
        let mut scratch = self.clone();
        records
            .iter()
            .map(|record| scratch.activate(record, num_outputs))
            .collect()
    }

    /// Score every record across the `rayon` thread pool, returning one output
    /// vector per record in input order.
    ///
    /// Each rayon worker initialises its own cloned scratch context via
    /// `map_init`, so no `&mut self` is shared across threads: immutable weights
    /// are read through `&self` while every worker owns its activation buffers.
    /// Because each record is scored by the same [`CompiledNetwork::activate`]
    /// used by [`CompiledNetwork::score_records`] and there is no cross-record
    /// state, the results are identical to the sequential path.
    ///
    /// Available with the `parallel` feature on native targets.
    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    pub fn score_records_parallel(
        &self,
        records: &[Vec<f32>],
        num_outputs: usize,
    ) -> Vec<Vec<f32>> {
        use rayon::prelude::*;
        records
            .par_iter()
            .map_init(
                || self.clone(),
                |scratch, record| scratch.activate(record, num_outputs),
            )
            .collect()
    }

    /// Sequential fallback for [`CompiledNetwork::score_records_parallel`] when
    /// the `parallel` feature is disabled or building for `wasm32` (where
    /// `rayon` is unavailable). Same signature and identical results to the
    /// feature-on path — just single-threaded.
    #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
    pub fn score_records_parallel(
        &self,
        records: &[Vec<f32>],
        num_outputs: usize,
    ) -> Vec<Vec<f32>> {
        self.score_records(records, num_outputs)
    }
}
