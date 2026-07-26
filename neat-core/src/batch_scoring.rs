//! Batched record scoring through the 8-record SIMD path (Issue #230).
//!
//! The production topology is *wide, shallow, sparse, varied fan-in* —
//! gather-bound (see `benches/README.md`). The single-record forward pass
//! ([`CompiledNetwork::activate_into`]) re-reads each synapse's weight and
//! metadata once **per record**. This module drives the forward pass through
//! the existing across-records SIMD primitives
//! ([`weighted_sum_simd_8records`] / [`weighted_sum_simd_4records`]), loading
//! each synapse weight once and applying it across 8 (then 4) records. That
//! amortises the weight load and synapse-index read across the batch, cutting
//! gather traffic on the dominant standard-squash neurons.
//!
//! # Numerics
//!
//! Standard-squash neurons accumulate their weighted sum across records rather
//! than across synapses, so the summation is re-associated relative to the
//! single-record [`weighted_sum_simd`] kernel. In `f32` that yields results
//! that match the per-record reference **within a small tolerance**, not
//! bit-for-bit (Issue #230 acceptance criteria explicitly allow SIMD
//! reordering / `f32` accumulation differences). Every other numeric step is
//! identical to [`CompiledNetwork::activate_into`]:
//!
//! - The **squash** for the covered types is evaluated across all 8 (then 4)
//!   batch lanes at once through the vectorised `squash_x8` / `squash_x4`
//!   approximations (Issue #243, mirroring `mse_sum_batch_packed`). Those
//!   approximations stay within `SQUASH_SIMD_MAX_ABS_ERR` of scalar
//!   `apply_squash`, so results match the per-record reference within the same
//!   SIMD tolerance the batched weighted sums already introduce (Issue #230).
//!   Every other type keeps the scalar inline branch (Identity / ReLU /
//!   Logistic / Tanh, else `apply_squash`), so its squash stays bit-identical.
//! - **Aggregate** squashes (Minimum, Maximum, If, Hypotenuse, HypotenuseV2,
//!   Mean) and the **scalar tail** (`records.len() % 8` after the 4-way step)
//!   run the exact single-record path via `neuron_activation_scalar`, so those
//!   neurons and those records are bit-identical to the reference.
//!
//! # Determinism
//!
//! Weights are read through `&self`; all mutable state lives in a caller-owned
//! [`BatchScratch`]. Record `i`'s outputs are always written to
//! `out[i * num_outputs .. (i + 1) * num_outputs]`, so output order matches
//! input order regardless of batching or thread count.

use crate::network::{CompiledNetwork, NeuronData, SynapseData};
use crate::range::{apply_get_range, apply_limit_range, apply_limit_range_bounds};
use crate::simd::{
    weighted_sum_interleaved_8, weighted_sum_no_bias_simd, weighted_sum_of_squares_simd,
    weighted_sum_of_squares_v2_simd, weighted_sum_simd, weighted_sum_simd_4records,
    weighted_sum_simd_8records,
};
use crate::squash::{SquashType, apply_squash};
use crate::squash_simd::{squash_x4, squash_x8};
use crate::synapse_type::SynapseType;

/// Number of records processed per SIMD batch (one lane each).
pub(crate) const SCORING_LANES: usize = 8;

/// Input records for one scoring batch, in either supported layout (Issue #386).
///
/// The kernels below only ever need `&[f32]` per record, so both layouts feed
/// the *same* forward pass and produce bit-identical results:
///
/// - [`RecordBatch::PerRecord`] — one owned `Vec<f32>` per record, the original
///   layout kept for existing callers.
/// - [`RecordBatch::Flat`] — a single contiguous buffer where record `i`
///   occupies `inputs[i * stride .. i * stride + stride]`, mirroring the flat
///   *output* contract from Issue #229 and the packed input layout the fused
///   loss lane already takes ([`crate::loss::mse_sum_batch_packed`]). Callers
///   that already hold a contiguous buffer — the WASM dataset offload path
///   ([`crate::wasm_dataset::TrainingDataset`]) — pass it straight through with
///   no per-record allocation and no re-marshalling.
#[derive(Clone, Copy)]
pub(crate) enum RecordBatch<'a> {
    /// One owned vector per record.
    PerRecord(&'a [Vec<f32>]),
    /// Records packed contiguously at a fixed stride.
    Flat {
        /// Packed inputs — exactly `record_count * stride` values.
        inputs: &'a [f32],
        /// Values per record. Non-zero, and a divisor of `inputs.len()`.
        stride: usize,
    },
}

impl<'a> RecordBatch<'a> {
    /// Build a flat-layout batch, failing loud (Issue #3234) on a stride that
    /// cannot describe records at all or a buffer that is not a whole number of
    /// records — either would silently mis-slice every record downstream.
    ///
    /// # Panics
    ///
    /// Panics if `stride` is zero or `inputs.len()` is not a multiple of it.
    #[inline]
    pub(crate) fn flat(inputs: &'a [f32], stride: usize) -> Self {
        assert!(stride > 0, "flat record stride must be greater than zero");
        assert!(
            inputs.len().is_multiple_of(stride),
            "flat record buffer of {} values is not a whole number of records at stride {stride}",
            inputs.len()
        );
        Self::Flat { inputs, stride }
    }

    /// Number of records in the batch.
    #[inline]
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::PerRecord(records) => records.len(),
            Self::Flat { inputs, stride } => inputs.len() / stride,
        }
    }

    /// Inputs for record `index`.
    #[inline]
    pub(crate) fn record(&self, index: usize) -> &'a [f32] {
        match self {
            Self::PerRecord(records) => &records[index],
            Self::Flat { inputs, stride } => &inputs[index * stride..(index + 1) * stride],
        }
    }
}

/// Reusable per-worker scratch for the batched scoring forward pass.
///
/// Owning the buffers here lets the sequential path allocate once for a whole
/// batch and the parallel path allocate once per rayon worker (via
/// `for_each_init`), instead of once per chunk.
///
/// Two layouts are held so scoring can pick the cheaper gather per network
/// (Issue #287):
///
/// - `inter` is the **record-interleaved** buffer used by the fast path when the
///   network has no aggregate-squash neurons: lane `l` of neuron `n` lives at
///   `inter[n * SCORING_LANES + l]`, so a synapse's eight records are contiguous
///   and each gather reads one cache line instead of eight scattered buffers.
/// - `acts` is the original eight-buffer (one-per-lane) layout, retained for the
///   fallback path that scores networks containing aggregate squashes
///   (Minimum/Maximum/If/Hypotenuse/HypotenuseV2/Mean), whose exact
///   single-record kernels index a contiguous per-lane activation slice.
pub struct BatchScratch {
    acts: [Vec<f32>; 8],
    inter: Vec<f32>,
}

impl BatchScratch {
    /// Allocate the zeroed activation buffers sized to the network: eight
    /// per-lane buffers plus one interleaved buffer of `num_neurons * 8`.
    pub fn new(num_neurons: usize) -> Self {
        Self {
            acts: std::array::from_fn(|_| vec![0.0f32; num_neurons]),
            inter: vec![0.0f32; num_neurons * SCORING_LANES],
        }
    }
}

/// Inline squash matching [`CompiledNetwork::activate_into`] exactly: the four
/// hot types are branched directly, everything else defers to [`apply_squash`].
#[inline]
fn inline_squash(squash_type: u8, squash: SquashType, sum: f32) -> f32 {
    match squash_type {
        0 => sum,                        // IDENTITY
        1 => sum.max(0.0),               // ReLU
        6 => 1.0 / (1.0 + (-sum).exp()), // LOGISTIC
        7 => sum.tanh(),                 // TANH
        _ => apply_squash(squash, sum),  // Other (fallback)
    }
}

/// Single-record activation for one neuron, byte-for-byte identical to the body
/// of [`CompiledNetwork::activate_into`]. Reused for constant neurons, aggregate
/// squashes and the scalar tail so those results exactly match the reference.
#[inline]
fn neuron_activation_scalar(synapses: &[SynapseData], act: &[f32], neuron: &NeuronData) -> f32 {
    if neuron.is_constant {
        return apply_limit_range(SquashType::Identity, neuron.bias);
    }

    let squash = SquashType::from(neuron.squash_type);
    let start = neuron.start_synapse as usize;
    let end = start + neuron.num_synapses as usize;

    let activation = match squash {
        SquashType::Minimum => {
            let mut min_val = f32::INFINITY;
            for s in &synapses[start..end] {
                let val = act[s.from_index as usize] * s.weight;
                if val < min_val {
                    min_val = val;
                }
            }
            if min_val == f32::INFINITY {
                neuron.bias
            } else {
                min_val + neuron.bias
            }
        }
        SquashType::Maximum => {
            let mut max_val = f32::NEG_INFINITY;
            for s in &synapses[start..end] {
                let val = act[s.from_index as usize] * s.weight;
                if val > max_val {
                    max_val = val;
                }
            }
            if max_val == f32::NEG_INFINITY {
                neuron.bias
            } else {
                max_val + neuron.bias
            }
        }
        SquashType::If => {
            let mut condition_sum = 0.0f32;
            let mut positive_sum = 0.0f32;
            let mut negative_sum = 0.0f32;
            for s in &synapses[start..end] {
                let val = act[s.from_index as usize] * s.weight;
                match SynapseType::from(s.synapse_type) {
                    SynapseType::Condition => condition_sum += val,
                    SynapseType::Negative => negative_sum += val,
                    SynapseType::Positive | SynapseType::Standard => positive_sum += val,
                }
            }
            if condition_sum > 0.0 {
                positive_sum + neuron.bias
            } else {
                negative_sum + neuron.bias
            }
        }
        SquashType::Hypotenuse => {
            let sum_sq = weighted_sum_of_squares_simd(synapses, act, start, end);
            sum_sq.sqrt() + neuron.bias
        }
        SquashType::HypotenuseV2 => {
            let sum_sq = weighted_sum_of_squares_v2_simd(synapses, act, start, end, neuron.bias);
            sum_sq.sqrt()
        }
        SquashType::Mean => {
            let n = (end - start) as f32;
            if n <= 0.0 {
                neuron.bias
            } else {
                let sum = weighted_sum_no_bias_simd(synapses, act, start, end);
                sum / n + neuron.bias
            }
        }
        _ => {
            let sum = weighted_sum_simd(synapses, act, start, end, neuron.bias);
            inline_squash(neuron.squash_type, squash, sum)
        }
    };

    apply_limit_range(squash, activation)
}

/// Copy record inputs into a lane buffer, matching [`CompiledNetwork::activate_into`]
/// (which copies `min(len, num_inputs)` input values). Any input slot the record
/// does not cover is zeroed so each record is scored statelessly — buffers are
/// reused across batches, and every non-input neuron is overwritten during the
/// forward pass, so no further reset is required.
#[inline]
fn load_record(act: &mut [f32], record: &[f32], num_inputs: usize) {
    let in_len = record.len().min(num_inputs);
    act[..in_len].copy_from_slice(&record[..in_len]);
    act[in_len..num_inputs].fill(0.0);
}

impl CompiledNetwork {
    /// Score a batch of records through the batched SIMD path.
    ///
    /// `records` may be in either input layout ([`RecordBatch`]) — per-record
    /// `Vec`s or one contiguous flat buffer at a fixed stride. The kernels read
    /// each record as `&[f32]`, so the two layouts are bit-identical.
    ///
    /// Record `i` (0-based within `records`) writes its outputs to
    /// `out[i * num_outputs .. (i + 1) * num_outputs]`; `out` must be exactly
    /// `records.len() * num_outputs` long.
    ///
    /// Dispatches between two numerically-equivalent layouts (Issue #287):
    ///
    /// - **Interleaved fast path** ([`Self::score_batch_interleaved`]) when the
    ///   network has no aggregate-squash neurons — the common case, including the
    ///   all-standard-squash production topology. Records are transposed so each
    ///   synapse gather reads one cache line.
    /// - **Per-lane fallback** ([`Self::score_batch_per_lane`]) otherwise, which
    ///   keeps aggregate squashes (Minimum/Maximum/If/Hypotenuse/HypotenuseV2/
    ///   Mean) on the exact single-record kernels.
    ///
    /// Both group records into 8s then a 4-record group then a scalar tail, and
    /// both are bit-identical on the covered standard-squash neurons.
    pub(crate) fn score_batch_into(
        &self,
        scratch: &mut BatchScratch,
        records: RecordBatch<'_>,
        num_outputs: usize,
        out: &mut [f32],
    ) {
        if self.has_aggregate_squash() {
            self.score_batch_per_lane(scratch, records, num_outputs, out);
        } else {
            self.score_batch_interleaved(scratch, records, num_outputs, out);
        }
    }

    /// True when any non-constant neuron uses an aggregate squash whose exact
    /// kernel needs a contiguous per-lane activation slice (so the interleaved
    /// fast path does not apply). O(neurons); the scoring batch dwarfs it.
    ///
    /// `pub(crate)` so the fused MSE loss lane can share the same dispatch
    /// decision (Issue #384): standard-only networks route through the
    /// interleaved gather, aggregate networks stay on their exact per-lane path.
    pub(crate) fn has_aggregate_squash(&self) -> bool {
        self.neurons.iter().any(|neuron| {
            !neuron.is_constant
                && matches!(
                    SquashType::from(neuron.squash_type),
                    SquashType::Minimum
                        | SquashType::Maximum
                        | SquashType::If
                        | SquashType::Hypotenuse
                        | SquashType::HypotenuseV2
                        | SquashType::Mean
                )
        })
    }

    /// Record-interleaved scoring fast path (Issue #287).
    ///
    /// Transposes each group of eight records into `scratch.inter`
    /// (`inter[n * 8 + l]` = lane `l` of neuron `n`) so every synapse gather in
    /// [`weighted_sum_interleaved_8`] reads one contiguous cache line rather than
    /// eight scattered per-lane buffers, and each neuron's eight outputs are a
    /// contiguous store. Full 8-record groups are bit-identical to the per-lane
    /// 8-record path (same FMA order and vectorised squash); the trailing
    /// `records.len() % 8` records run the exact single-record kernel
    /// ([`neuron_activation_scalar`]) so they stay bit-for-bit identical to
    /// `activate` — matching the per-lane fallback's scalar-tail guarantee. Only
    /// called when [`Self::has_aggregate_squash`] is false, so the group path
    /// needs no aggregate kernel.
    fn score_batch_interleaved(
        &self,
        scratch: &mut BatchScratch,
        records: RecordBatch<'_>,
        num_outputs: usize,
        out: &mut [f32],
    ) {
        const L: usize = SCORING_LANES;
        let num_inputs = self.num_inputs;
        let num_neurons = self.num_neurons;
        let output_start = num_neurons - num_outputs;
        let n = records.len();

        // `inter` (fast group path) and `acts[0]` (exact scalar tail) are disjoint
        // fields, so borrow both at once.
        let BatchScratch { acts, inter } = scratch;
        let tail_act = &mut acts[0];

        let mut base = 0usize;

        // ---- full 8-record groups -------------------------------------------
        while base + L <= n {
            // Transpose L records into the interleaved buffer.
            for l in 0..L {
                let rec = records.record(base + l);
                let in_len = rec.len().min(num_inputs);
                for i in 0..in_len {
                    inter[i * L + l] = rec[i];
                }
                for i in in_len..num_inputs {
                    inter[i * L + l] = 0.0;
                }
            }

            // Forward pass, all L lanes at once, through the shared kernel.
            self.interleaved_forward_8(inter);

            // Scatter each lane's outputs to the flat buffer.
            for l in 0..L {
                let dst = (base + l) * num_outputs;
                for o in 0..num_outputs {
                    out[dst + o] = inter[(output_start + o) * L + l];
                }
            }

            base += L;
        }

        // ---- exact single-record tail (records.len() % 8) -------------------
        // Bit-identical to `activate`, so a lone or partial trailing group keeps
        // the strict single-record parity guarantee.
        while base < n {
            load_record(tail_act, records.record(base), num_inputs);
            for (neuron_idx, neuron) in self.neurons.iter().enumerate() {
                let actual_idx = num_inputs + neuron_idx;
                tail_act[actual_idx] = neuron_activation_scalar(&self.synapses, tail_act, neuron);
            }
            let dst = base * num_outputs;
            out[dst..dst + num_outputs]
                .copy_from_slice(&tail_act[output_start..output_start + num_outputs]);
            base += 1;
        }
    }

    /// Shared record-interleaved forward pass over a loaded `inter` buffer
    /// (Issue #287 kernel, factored out for reuse by the fused MSE loss lane in
    /// Issue #384).
    ///
    /// The caller must have transposed the eight records' inputs into
    /// `inter[i * 8 + l]` for every input neuron `i` and lane `l`. This fills
    /// each non-input neuron's eight output lanes at
    /// `inter[(num_inputs + neuron_idx) * 8 + l]`, gathering through
    /// [`weighted_sum_interleaved_8`] so each synapse reads one cache line.
    ///
    /// Only valid when [`Self::has_aggregate_squash`] is false — every
    /// non-constant neuron is treated as standard-squash (vectorised
    /// `squash_x8` with the scalar inline fallback), so aggregate networks must
    /// use the exact per-lane path instead. Bit-identical to the per-lane
    /// 8-record path ([`weighted_sum_simd_8records`]) on the covered neurons.
    pub(crate) fn interleaved_forward_8(&self, inter: &mut [f32]) {
        const L: usize = SCORING_LANES;
        let num_inputs = self.num_inputs;

        for (neuron_idx, neuron) in self.neurons.iter().enumerate() {
            let out_base = (num_inputs + neuron_idx) * L;

            if neuron.is_constant {
                let v = apply_limit_range(SquashType::Identity, neuron.bias);
                for slot in inter[out_base..out_base + L].iter_mut() {
                    *slot = v;
                }
                continue;
            }

            let squash = SquashType::from(neuron.squash_type);
            let start = neuron.start_synapse as usize;
            let end = start + neuron.num_synapses as usize;
            let sums = weighted_sum_interleaved_8(&self.synapses, inter, start, end, neuron.bias);

            // Vectorised squash across all 8 lanes for the covered types
            // (Issue #243); scalar inline fallback otherwise.
            let squashed = squash_x8(squash, sums).unwrap_or_else(|| {
                let st = neuron.squash_type;
                sums.map(|s| inline_squash(st, squash, s))
            });

            // Resolve the output range once per neuron (Issue #245) and clamp
            // all 8 lanes; the writes land in one contiguous cache line.
            let (low, high) = apply_get_range(squash);
            for l in 0..L {
                inter[out_base + l] = apply_limit_range_bounds(low, high, squashed[l]);
            }
        }
    }

    /// Per-lane fallback scoring path — the original eight-buffer layout, kept
    /// verbatim for networks containing aggregate squashes (Issue #287). Record
    /// `i` writes `out[i * num_outputs ..]`; records are grouped into 8s
    /// ([`weighted_sum_simd_8records`]), then a 4-record group
    /// ([`weighted_sum_simd_4records`]), then a scalar tail.
    fn score_batch_per_lane(
        &self,
        scratch: &mut BatchScratch,
        records: RecordBatch<'_>,
        num_outputs: usize,
        out: &mut [f32],
    ) {
        let num_neurons = self.num_neurons;
        let num_inputs = self.num_inputs;
        let output_start = num_neurons - num_outputs;
        let n = records.len();

        let [act0, act1, act2, act3, act4, act5, act6, act7] = &mut scratch.acts;

        let mut base = 0usize;

        // ---- 8-record batches ------------------------------------------------
        while base + 8 <= n {
            load_record(act0, records.record(base), num_inputs);
            load_record(act1, records.record(base + 1), num_inputs);
            load_record(act2, records.record(base + 2), num_inputs);
            load_record(act3, records.record(base + 3), num_inputs);
            load_record(act4, records.record(base + 4), num_inputs);
            load_record(act5, records.record(base + 5), num_inputs);
            load_record(act6, records.record(base + 6), num_inputs);
            load_record(act7, records.record(base + 7), num_inputs);

            for (neuron_idx, neuron) in self.neurons.iter().enumerate() {
                let actual_idx = num_inputs + neuron_idx;

                if neuron.is_constant {
                    let v = apply_limit_range(SquashType::Identity, neuron.bias);
                    act0[actual_idx] = v;
                    act1[actual_idx] = v;
                    act2[actual_idx] = v;
                    act3[actual_idx] = v;
                    act4[actual_idx] = v;
                    act5[actual_idx] = v;
                    act6[actual_idx] = v;
                    act7[actual_idx] = v;
                    continue;
                }

                let squash = SquashType::from(neuron.squash_type);
                match squash {
                    SquashType::Minimum
                    | SquashType::Maximum
                    | SquashType::If
                    | SquashType::Hypotenuse
                    | SquashType::HypotenuseV2
                    | SquashType::Mean => {
                        // Aggregate squashes stay on the exact single-record path.
                        act0[actual_idx] = neuron_activation_scalar(&self.synapses, act0, neuron);
                        act1[actual_idx] = neuron_activation_scalar(&self.synapses, act1, neuron);
                        act2[actual_idx] = neuron_activation_scalar(&self.synapses, act2, neuron);
                        act3[actual_idx] = neuron_activation_scalar(&self.synapses, act3, neuron);
                        act4[actual_idx] = neuron_activation_scalar(&self.synapses, act4, neuron);
                        act5[actual_idx] = neuron_activation_scalar(&self.synapses, act5, neuron);
                        act6[actual_idx] = neuron_activation_scalar(&self.synapses, act6, neuron);
                        act7[actual_idx] = neuron_activation_scalar(&self.synapses, act7, neuron);
                    }
                    _ => {
                        let start = neuron.start_synapse as usize;
                        let end = start + neuron.num_synapses as usize;
                        let (s0, s1, s2, s3, s4, s5, s6, s7) = weighted_sum_simd_8records(
                            &self.synapses,
                            act0,
                            act1,
                            act2,
                            act3,
                            act4,
                            act5,
                            act6,
                            act7,
                            start,
                            end,
                            neuron.bias,
                        );
                        // Vectorised squash across all 8 lanes for the covered
                        // types (Issue #243); scalar inline fallback otherwise.
                        let sums = [s0, s1, s2, s3, s4, s5, s6, s7];
                        let squashed = squash_x8(squash, sums).unwrap_or_else(|| {
                            let st = neuron.squash_type;
                            sums.map(|s| inline_squash(st, squash, s))
                        });
                        // Issue #245: resolve the output range once per neuron
                        // and clamp all 8 lanes through the bounds, so the range
                        // `match` runs once instead of once per record.
                        let (low, high) = apply_get_range(squash);
                        act0[actual_idx] = apply_limit_range_bounds(low, high, squashed[0]);
                        act1[actual_idx] = apply_limit_range_bounds(low, high, squashed[1]);
                        act2[actual_idx] = apply_limit_range_bounds(low, high, squashed[2]);
                        act3[actual_idx] = apply_limit_range_bounds(low, high, squashed[3]);
                        act4[actual_idx] = apply_limit_range_bounds(low, high, squashed[4]);
                        act5[actual_idx] = apply_limit_range_bounds(low, high, squashed[5]);
                        act6[actual_idx] = apply_limit_range_bounds(low, high, squashed[6]);
                        act7[actual_idx] = apply_limit_range_bounds(low, high, squashed[7]);
                    }
                }
            }

            for r in 0..8 {
                let src = match r {
                    0 => &*act0,
                    1 => &*act1,
                    2 => &*act2,
                    3 => &*act3,
                    4 => &*act4,
                    5 => &*act5,
                    6 => &*act6,
                    _ => &*act7,
                };
                let dst = (base + r) * num_outputs;
                out[dst..dst + num_outputs]
                    .copy_from_slice(&src[output_start..output_start + num_outputs]);
            }

            base += 8;
        }

        // ---- 4-record batch (0 or 1 of them) ---------------------------------
        if base + 4 <= n {
            load_record(act0, records.record(base), num_inputs);
            load_record(act1, records.record(base + 1), num_inputs);
            load_record(act2, records.record(base + 2), num_inputs);
            load_record(act3, records.record(base + 3), num_inputs);

            for (neuron_idx, neuron) in self.neurons.iter().enumerate() {
                let actual_idx = num_inputs + neuron_idx;

                if neuron.is_constant {
                    let v = apply_limit_range(SquashType::Identity, neuron.bias);
                    act0[actual_idx] = v;
                    act1[actual_idx] = v;
                    act2[actual_idx] = v;
                    act3[actual_idx] = v;
                    continue;
                }

                let squash = SquashType::from(neuron.squash_type);
                match squash {
                    SquashType::Minimum
                    | SquashType::Maximum
                    | SquashType::If
                    | SquashType::Hypotenuse
                    | SquashType::HypotenuseV2
                    | SquashType::Mean => {
                        act0[actual_idx] = neuron_activation_scalar(&self.synapses, act0, neuron);
                        act1[actual_idx] = neuron_activation_scalar(&self.synapses, act1, neuron);
                        act2[actual_idx] = neuron_activation_scalar(&self.synapses, act2, neuron);
                        act3[actual_idx] = neuron_activation_scalar(&self.synapses, act3, neuron);
                    }
                    _ => {
                        let start = neuron.start_synapse as usize;
                        let end = start + neuron.num_synapses as usize;
                        let (s0, s1, s2, s3) = weighted_sum_simd_4records(
                            &self.synapses,
                            act0,
                            act1,
                            act2,
                            act3,
                            start,
                            end,
                            neuron.bias,
                        );
                        // Vectorised squash across all 4 lanes for the covered
                        // types (Issue #243); scalar inline fallback otherwise.
                        let sums = [s0, s1, s2, s3];
                        let squashed = squash_x4(squash, sums).unwrap_or_else(|| {
                            let st = neuron.squash_type;
                            sums.map(|s| inline_squash(st, squash, s))
                        });
                        // Issue #245: resolve the output range once per neuron
                        // and clamp all 4 lanes through the bounds.
                        let (low, high) = apply_get_range(squash);
                        act0[actual_idx] = apply_limit_range_bounds(low, high, squashed[0]);
                        act1[actual_idx] = apply_limit_range_bounds(low, high, squashed[1]);
                        act2[actual_idx] = apply_limit_range_bounds(low, high, squashed[2]);
                        act3[actual_idx] = apply_limit_range_bounds(low, high, squashed[3]);
                    }
                }
            }

            for r in 0..4 {
                let src = match r {
                    0 => &*act0,
                    1 => &*act1,
                    2 => &*act2,
                    _ => &*act3,
                };
                let dst = (base + r) * num_outputs;
                out[dst..dst + num_outputs]
                    .copy_from_slice(&src[output_start..output_start + num_outputs]);
            }

            base += 4;
        }

        // ---- scalar tail (records.len() % 4) ---------------------------------
        // Exact single-record path so tail records are bit-identical to the
        // reference.
        while base < n {
            load_record(act0, records.record(base), num_inputs);
            for (neuron_idx, neuron) in self.neurons.iter().enumerate() {
                let actual_idx = num_inputs + neuron_idx;
                act0[actual_idx] = neuron_activation_scalar(&self.synapses, act0, neuron);
            }
            let dst = base * num_outputs;
            out[dst..dst + num_outputs]
                .copy_from_slice(&act0[output_start..output_start + num_outputs]);
            base += 1;
        }
    }
}
