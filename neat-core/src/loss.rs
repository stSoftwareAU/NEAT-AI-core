//! Loss function implementations for neural network batch scoring.
//!
//! This module provides fused activate + loss calculation functions that process
//! batches of records in a single WASM call. These are optimised for scoring performance
//! by minimising JS/WASM boundary crossings.
//!
//! Issue #118x, #1202, #1209 - Batch scoring optimisations.

use crate::batch_scoring::{
    SCORING_LANES, inline_squash, load_record, neuron_activation_scalar, run_interleaved_forward,
};
use crate::network::CompiledNetwork;
use crate::range::{apply_get_range, apply_limit_range, apply_limit_range_bounds};
use crate::simd::{weighted_sum_simd_4records_unchecked, weighted_sum_simd_8records_unchecked};
use crate::squash::SquashType;
use crate::squash_simd::{squash_x4, squash_x8};
use crate::training_bin_stream::{for_each_read_chunk_with_mode, training_read_tuning_from_env};
use crate::training_data::find_bin_files;
use std::path::{Path, PathBuf};

#[cfg(target_family = "wasm")]
use wasm_bindgen::prelude::*;

/// Issue #1209 - Shared 8-way activation helper macro to reduce code duplication.
///
/// This macro generates the neuron activation loop for 8 records in parallel,
/// then calls a provided error calculation closure for each record.
///
/// The single home of the batched record-scan skeleton (Issue #445): records are
/// grouped 8 → 4 → 1, each group's inputs are loaded through the shared
/// [`load_record`] loader, and the per-record errors accumulate into one `f64`
/// sum. `$error_fn` carries **only** the per-record reduction
/// (`(records, target_base, act, output_start, num_outputs) -> f64`), so every
/// loss kind — MSE included — shares one iteration rule.
macro_rules! batch_8way_activation {
    ($network:expr_2021, $records:expr_2021, $values_per_record:expr_2021, $input_size:expr_2021, $num_outputs:expr_2021, $num_records:expr_2021, $error_fn:expr_2021) => {{
        let num_neurons = $network.num_neurons;
        let num_inputs = $network.num_inputs;
        let mut act0: Vec<f32> = vec![0.0; num_neurons];
        let mut act1: Vec<f32> = vec![0.0; num_neurons];
        let mut act2: Vec<f32> = vec![0.0; num_neurons];
        let mut act3: Vec<f32> = vec![0.0; num_neurons];
        let mut act4: Vec<f32> = vec![0.0; num_neurons];
        let mut act5: Vec<f32> = vec![0.0; num_neurons];
        let mut act6: Vec<f32> = vec![0.0; num_neurons];
        let mut act7: Vec<f32> = vec![0.0; num_neurons];

        let mut sum_error: f64 = 0.0;
        let output_start = num_neurons - $num_outputs;

        // Process in batches of 8
        let full_batches = $num_records / 8;
        for batch in 0..full_batches {
            let base_idx = batch * 8;

            // Load inputs for all 8 records
            for r in 0..8 {
                let record_idx = base_idx + r;
                let base = record_idx * $values_per_record;
                let inputs = &$records[base..base + $input_size];
                let act = match r {
                    0 => &mut act0,
                    1 => &mut act1,
                    2 => &mut act2,
                    3 => &mut act3,
                    4 => &mut act4,
                    5 => &mut act5,
                    6 => &mut act6,
                    _ => &mut act7,
                };
                load_record(act, inputs, num_inputs);
            }

            // Process each neuron for all 8 records
            for (neuron_idx, neuron) in $network.neurons.iter().enumerate() {
                let actual_idx = num_inputs + neuron_idx;

                if neuron.is_constant {
                    let val = apply_limit_range(SquashType::Identity, neuron.bias);
                    act0[actual_idx] = val;
                    act1[actual_idx] = val;
                    act2[actual_idx] = val;
                    act3[actual_idx] = val;
                    act4[actual_idx] = val;
                    act5[actual_idx] = val;
                    act6[actual_idx] = val;
                    act7[actual_idx] = val;
                } else {
                    let squash = SquashType::from(neuron.squash_type);
                    let start_synapse = neuron.start_synapse as usize;
                    let end_synapse = start_synapse + neuron.num_synapses as usize;

                    match squash {
                        // Aggregate squashes stay on the exact single-record path.
                        s if s.is_aggregate() => {
                            for act in [
                                &mut act0, &mut act1, &mut act2, &mut act3, &mut act4, &mut act5,
                                &mut act6, &mut act7,
                            ] {
                                let value =
                                    neuron_activation_scalar(&$network.synapses, act, neuron);
                                act[actual_idx] = value;
                            }
                        }
                        _ => {
                            let (sum0, sum1, sum2, sum3, sum4, sum5, sum6, sum7) =
                                // SAFETY: loaded `CompiledNetwork` — `new` rejected every out-of-range
                                // `from_index`, and every activation buffer is sized to `num_neurons`.
                                unsafe { weighted_sum_simd_8records_unchecked(
                                    &$network.synapses,
                                    &act0,
                                    &act1,
                                    &act2,
                                    &act3,
                                    &act4,
                                    &act5,
                                    &act6,
                                    &act7,
                                    start_synapse,
                                    end_synapse,
                                    neuron.bias,
                                ) };

                            // Hot transcendental squashes evaluate all 8 batch
                            // lanes at once via the vectorised approximation
                            // (Issue #180); other types keep the scalar inline
                            // squash so their numerics are unchanged.
                            let sums = [sum0, sum1, sum2, sum3, sum4, sum5, sum6, sum7];
                            let squashed = match squash_x8(squash, sums) {
                                Some(vec) => vec,
                                None => {
                                    sums.map(|sum| inline_squash(neuron.squash_type, squash, sum))
                                }
                            };

                            // Issue #245: resolve the output range once per
                            // neuron and clamp all 8 lanes through the bounds.
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
            }

            // Calculate error for all 8 records
            for r in 0..8 {
                let record_idx = base_idx + r;
                let target_base = record_idx * $values_per_record + $input_size;
                let act = match r {
                    0 => &act0,
                    1 => &act1,
                    2 => &act2,
                    3 => &act3,
                    4 => &act4,
                    5 => &act5,
                    6 => &act6,
                    _ => &act7,
                };
                sum_error += $error_fn($records, target_base, act, output_start, $num_outputs);
            }
        }

        // Handle remainder: use 4-way for 4-7 remaining records
        let remainder_start = full_batches * 8;
        let remaining = $num_records - remainder_start;

        if remaining >= 4 {
            let four_way_batches = remaining / 4;
            for batch in 0..four_way_batches {
                let base_idx = remainder_start + batch * 4;

                for r in 0..4 {
                    let record_idx = base_idx + r;
                    let base = record_idx * $values_per_record;
                    let inputs = &$records[base..base + $input_size];
                    let act = match r {
                        0 => &mut act0,
                        1 => &mut act1,
                        2 => &mut act2,
                        _ => &mut act3,
                    };
                    load_record(act, inputs, num_inputs);
                }

                for (neuron_idx, neuron) in $network.neurons.iter().enumerate() {
                    let actual_idx = num_inputs + neuron_idx;

                    if neuron.is_constant {
                        let val = apply_limit_range(SquashType::Identity, neuron.bias);
                        act0[actual_idx] = val;
                        act1[actual_idx] = val;
                        act2[actual_idx] = val;
                        act3[actual_idx] = val;
                    } else {
                        let squash = SquashType::from(neuron.squash_type);
                        let start_synapse = neuron.start_synapse as usize;
                        let end_synapse = start_synapse + neuron.num_synapses as usize;

                        match squash {
                            // Aggregate squashes stay on the exact single-record path.
                            s if s.is_aggregate() => {
                                for act in [&mut act0, &mut act1, &mut act2, &mut act3] {
                                    let value =
                                        neuron_activation_scalar(&$network.synapses, act, neuron);
                                    act[actual_idx] = value;
                                }
                            }
                            _ => {
                                // SAFETY: loaded `CompiledNetwork` — `new` rejected every out-of-range
                                // `from_index`, and every activation buffer is sized to `num_neurons`.
                                let (sum0, sum1, sum2, sum3) = unsafe { weighted_sum_simd_4records_unchecked(
                                    &$network.synapses,
                                    &act0,
                                    &act1,
                                    &act2,
                                    &act3,
                                    start_synapse,
                                    end_synapse,
                                    neuron.bias,
                                ) };

                                // Vectorised squash for the hot transcendental
                                // types (Issue #180); scalar fallback otherwise.
                                let sums = [sum0, sum1, sum2, sum3];
                                let squashed = match squash_x4(squash, sums) {
                                    Some(vec) => vec,
                                    None => sums
                                        .map(|sum| inline_squash(neuron.squash_type, squash, sum)),
                                };

                                // Issue #245: resolve the range once per neuron.
                                let (low, high) = apply_get_range(squash);
                                act0[actual_idx] = apply_limit_range_bounds(low, high, squashed[0]);
                                act1[actual_idx] = apply_limit_range_bounds(low, high, squashed[1]);
                                act2[actual_idx] = apply_limit_range_bounds(low, high, squashed[2]);
                                act3[actual_idx] = apply_limit_range_bounds(low, high, squashed[3]);
                            }
                        }
                    }
                }

                for r in 0..4 {
                    let record_idx = base_idx + r;
                    let target_base = record_idx * $values_per_record + $input_size;
                    let act = match r {
                        0 => &act0,
                        1 => &act1,
                        2 => &act2,
                        _ => &act3,
                    };
                    sum_error += $error_fn($records, target_base, act, output_start, $num_outputs);
                }
            }
        }

        // Handle final remainder with single-record processing
        let final_remainder_start = remainder_start + (remaining / 4) * 4;
        for record_idx in final_remainder_start..$num_records {
            let base = record_idx * $values_per_record;
            let inputs = &$records[base..base + $input_size];
            let target_base = base + $input_size;

            load_record(&mut act0, inputs, num_inputs);

            for i in num_inputs..num_neurons {
                act0[i] = 0.0;
            }

            for (neuron_idx, neuron) in $network.neurons.iter().enumerate() {
                let value = neuron_activation_scalar(&$network.synapses, &act0, neuron);
                act0[num_inputs + neuron_idx] = value;
            }

            sum_error += $error_fn($records, target_base, &act0, output_start, $num_outputs);
        }

        sum_error
    }};
}

/// How a packed `[inputs…, targets…]` buffer is carved into records.
///
/// Issue #444 — the single home of the packed-record layout rule.
struct PackedLayout {
    /// Stride between successive records: `input_size + num_outputs`.
    values_per_record: usize,
    /// Whole records the buffer holds; a trailing partial record is ignored.
    num_records: usize,
}

/// Carve a packed buffer into records, or `None` when it holds no whole record
/// (a zero-width record, or fewer values than one full record needs).
fn packed_layout(
    records_len: usize,
    input_size: usize,
    num_outputs: usize,
) -> Option<PackedLayout> {
    let values_per_record = input_size + num_outputs;
    if values_per_record == 0 {
        return None;
    }
    let num_records = records_len / values_per_record;
    if num_records == 0 {
        return None;
    }
    Some(PackedLayout {
        values_per_record,
        num_records,
    })
}

/// Drive a packed `[inputs…, targets…]` buffer record by record, returning the
/// sum of `reduce(targets, outputs)` over every whole record. Returns `0.0`
/// when the buffer holds no whole records.
///
/// `reduce` receives the record's target slice and the network's output slice,
/// both `num_outputs` long. Any per-record averaging belongs inside `reduce` —
/// MSLE and hinge deliberately do not average.
///
/// When `forward_only` is false the network's hidden state is cleared before
/// each record, preserving stateless (`feedbackLoop = false`) semantics; when
/// it is true the reset is skipped, which v4+ forward-only creatures allow.
///
/// Issue #444 — the single home of the per-record scan; SIMD dispatch stays at
/// the callers, where it genuinely differs.
fn packed_record_scan(
    network: &mut CompiledNetwork,
    records: &[f32],
    input_size: usize,
    num_outputs: usize,
    forward_only: bool,
    reduce: impl Fn(&[f32], &[f32]) -> f64,
) -> f64 {
    let Some(layout) = packed_layout(records.len(), input_size, num_outputs) else {
        return 0.0;
    };

    // Reuse a small output buffer to avoid per-record allocation.
    let mut outputs: Vec<f32> = vec![0.0; num_outputs];
    let mut sum_error: f64 = 0.0;

    for record_idx in 0..layout.num_records {
        if !forward_only {
            // Ensure stateless behaviour for networks that may read stale activations.
            network.reset_state();
        }

        let base = record_idx * layout.values_per_record;
        let input_end = base + input_size;
        let target_start = input_end;
        // Activate into the reusable output buffer.
        network.activate_into(&records[base..input_end], &mut outputs[..]);

        sum_error += reduce(
            &records[target_start..target_start + num_outputs],
            &outputs[..],
        );
    }

    sum_error
}

/// Per-record MSE: the mean over a record's outputs of `(target - output)^2`.
///
/// Accumulates in `f64` from the `f32` inputs, then scales by `1/outputs.len()`
/// — the reduction every scalar MSE path in this module applies, exported so
/// consumers holding activations already (e.g. a backpropagation trace pass
/// that cannot use a fused batch path) reduce them the same way.
///
/// Pairs are taken in order; when the slices differ in length the shorter one
/// ends the pairing, but the divisor is always `outputs.len()`.
///
/// Returns `0.0` when `outputs` is empty — there is nothing to average over.
///
/// Issue #538 — the single home of the per-record squared-error reduction.
pub fn mse_record(targets: &[f32], outputs: &[f32]) -> f64 {
    if outputs.is_empty() {
        return 0.0;
    }
    let inv_outputs = 1.0 / (outputs.len() as f64);
    let mut sq_sum: f64 = 0.0;
    for (t, o) in targets.iter().zip(outputs.iter()) {
        let diff = (*t - *o) as f64;
        sq_sum += diff * diff;
    }
    sq_sum * inv_outputs
}

/// Fused activate + MSE (Mean Squared Error) calculation for batch scoring.
///
/// This is a scoring fast-path designed to minimise JS/WASM boundary crossings:
/// - Each record is laid out as: [inputs..., targets...]
/// - `input_size` must match the number of input floats in each record.
/// - `num_outputs` must match the number of target/output floats in each record.
///
/// Returns the **sum** of per-record MSE values (not averaged over records).
///
/// When `forward_only=true`, we skip clearing `network.activations` between records
/// because v4+ forward-only creatures guarantee there are no recurrent/back edges.
/// When `forward_only=false`, we must call `reset_state()` each record to preserve
/// stateless semantics (`feedbackLoop=false`) and avoid state leakage.
///
/// Issue #118x - Fuse activate + MSE for scoring performance.
/// Issue #1202 - Use 4-record SIMD batching for forward-only networks.
#[cfg_attr(target_family = "wasm", wasm_bindgen)]
pub fn mse_sum_batch_packed(
    network: &mut CompiledNetwork,
    records: &[f32],
    input_size: usize,
    num_outputs: usize,
    forward_only: bool,
) -> f64 {
    let Some(layout) = packed_layout(records.len(), input_size, num_outputs) else {
        return 0.0;
    };

    // Issue #1209 - Use batched 8-record SIMD path for forward-only networks
    // Falls back to 4-way for remainder handling, then single-record
    if forward_only && layout.num_records >= 8 {
        return mse_sum_batch_8way(
            network,
            records,
            layout.values_per_record,
            input_size,
            num_outputs,
            layout.num_records,
        );
    }

    // Issue #1202 - Use batched 4-record SIMD path for forward-only networks.
    // The shared skeleton's remainder handling is the 4-way path (Issue #445):
    // with 4..7 records it runs one 4-record group then the scalar tail.
    if forward_only && layout.num_records >= 4 {
        return mse_sum_batch_scattered(
            network,
            records,
            layout.values_per_record,
            input_size,
            num_outputs,
            layout.num_records,
        );
    }

    // Issue #538 — the per-record reduction lives in `mse_record`.
    packed_record_scan(
        network,
        records,
        input_size,
        num_outputs,
        forward_only,
        mse_record,
    )
}

/// Issue #1209 - Batched MSE with 8-record SIMD parallelism.
///
/// Processes 8 records simultaneously using two SIMD vectors across records.
/// This is an internal helper that only works for forward-only networks
/// with standard squash functions. Falls back to 4-way for remainder < 8,
/// then single-record for remainder < 4.
///
/// Issue #384 - dispatches on [`CompiledNetwork::has_aggregate_squash`]:
/// standard-only networks (the production case) route the full 8-record groups
/// through the record-interleaved gather proven by #287
/// ([`mse_sum_batch_8way_interleaved`]), so each synapse reads one cache line
/// instead of eight scattered per-lane buffers; networks containing an
/// aggregate squash keep the exact per-lane scattered path
/// ([`mse_sum_batch_scattered`]) unchanged. Both are bit-identical to the
/// pre-#384 result.
fn mse_sum_batch_8way(
    network: &mut CompiledNetwork,
    records: &[f32],
    values_per_record: usize,
    input_size: usize,
    num_outputs: usize,
    num_records: usize,
) -> f64 {
    if network.has_aggregate_squash() {
        return mse_sum_batch_scattered(
            network,
            records,
            values_per_record,
            input_size,
            num_outputs,
            num_records,
        );
    }
    mse_sum_batch_8way_interleaved(
        network,
        records,
        values_per_record,
        input_size,
        num_outputs,
        num_records,
    )
}

/// Record tile width for the fused-MSE record-interleaved forward pass
/// (Issue #530).
///
/// The interleaved kernel re-streams the network's entire synapse array once per
/// tile, so per-record synapse traffic is `synapse_bytes / MSE_TILE_LANES`.
/// Widening the tile from the original 8 divides that traffic by
/// `MSE_TILE_LANES / 8`, at the cost of a proportionally larger `mse_inter`
/// scratch buffer: `num_neurons * MSE_TILE_LANES * 4` bytes **per network**,
/// and directory scoring holds one compiled network per worker. On the
/// production creature (~4,127 neurons) that is ~132 KB at 8 lanes and ~528 KB
/// at 32 — budget it against the scorer's worker-count RAM ceiling before
/// raising it further.
///
/// Must be a non-zero multiple of 8 and at most
/// [`crate::simd::MAX_INTERLEAVED_LANES`]; both are checked at compile time by
/// the gather kernel. Every tile width produces **bit-identical** results (see
/// `interleaved_mse_parity`), so this constant is a pure
/// memory-traffic/footprint trade-off.
pub const MSE_TILE_LANES: usize = 32;

/// Record-interleaved fused activate + MSE for standard-squash networks
/// (Issue #384; tunable tile width in Issue #530). Full `R`-record tiles run the
/// shared interleaved forward pass ([`run_interleaved_forward`]) — the #287
/// gather that reads each synapse's lanes from contiguous memory — then the MSE
/// reduction reads each lane's contiguous output slot. The `< R` remainder
/// steps down the existing ladder: whole 8-record interleaved tiles, then one
/// 4-record group, then the scalar tail, all on the exact same per-lane kernels
/// as the scattered path.
///
/// Records are reduced into `sum_error` in strict record order at every tier, and
/// each lane's weighted sum is an independent `bias + Σ w·a` in synapse order, so
/// the result is bit-identical to the pre-#384 8-way path **and** to any other
/// tile width.
///
/// Only called when the network has no aggregate-squash neuron; the caller
/// (`mse_sum_batch_8way`) routes aggregate networks to the scattered path.
///
/// Scratch reuse (NEAT-AI-scorer#531): writes into `network.mse_inter` and
/// `network.batch_activations` instead of allocating per call.
fn mse_sum_batch_8way_interleaved(
    network: &mut CompiledNetwork,
    records: &[f32],
    values_per_record: usize,
    input_size: usize,
    num_outputs: usize,
    num_records: usize,
) -> f64 {
    mse_sum_batch_interleaved::<MSE_TILE_LANES>(
        network,
        records,
        values_per_record,
        input_size,
        num_outputs,
        num_records,
    )
}

/// Transpose `R` records' inputs into the interleaved buffer and reduce their
/// squared error after the forward pass — the body shared by every tile tier.
///
/// Walks **input-major**: for each input neuron the `R` lanes are written to
/// consecutive `inter` slots, so the scratch buffer is filled by a single linear
/// sweep whatever `R` is. (Lane-major would revisit the whole `num_inputs * R`
/// region once per lane, which stops fitting in L1 as the tile widens.)
///
/// Takes the caller's **running** `sum_error` and returns it with this tile's
/// per-record MSE added in record order — a seed-taking helper, like the SIMD
/// tail helpers in [`crate::simd::scalar`]. Returning a per-tile partial sum
/// instead would re-associate the `f64` reduction and break bit-parity with the
/// scattered path.
#[inline]
#[allow(clippy::too_many_arguments)]
fn interleaved_tile_mse<const R: usize>(
    network: &mut CompiledNetwork,
    records: &[f32],
    values_per_record: usize,
    input_size: usize,
    num_outputs: usize,
    base_idx: usize,
    inv_outputs: f64,
    mut sum_error: f64,
) -> f64 {
    let num_neurons = network.num_neurons;
    let num_inputs = network.num_inputs;
    let output_start = num_neurons - num_outputs;
    let input_lanes = input_size.min(num_inputs);
    let inter = &mut network.mse_inter[..num_neurons * R];

    for i in 0..input_lanes {
        let row = &mut inter[i * R..i * R + R];
        for (l, slot) in row.iter_mut().enumerate() {
            *slot = records[(base_idx + l) * values_per_record + i];
        }
    }
    // Zero every input slot the records do not cover (stateless scoring).
    for slot in inter[input_lanes * R..num_inputs * R].iter_mut() {
        *slot = 0.0;
    }

    run_interleaved_forward::<R>(
        &network.neurons,
        &network.hot_weights,
        &network.hot_from,
        num_inputs,
        inter,
    );

    for l in 0..R {
        let target_base = (base_idx + l) * values_per_record + input_size;
        let mut sq_sum: f64 = 0.0;
        for j in 0..num_outputs {
            let out_val = inter[(output_start + j) * R + l];
            let diff = (records[target_base + j] - out_val) as f64;
            sq_sum += diff * diff;
        }
        sum_error += sq_sum * inv_outputs;
    }
    sum_error
}

fn mse_sum_batch_interleaved<const R: usize>(
    network: &mut CompiledNetwork,
    records: &[f32],
    values_per_record: usize,
    input_size: usize,
    num_outputs: usize,
    num_records: usize,
) -> f64 {
    // Issue #533 - fail loud in debug if the SoA hot view has drifted.
    network.debug_assert_hot_soa();
    let inv_outputs: f64 = if num_outputs > 0 {
        1.0 / (num_outputs as f64)
    } else {
        return 0.0;
    };

    let num_neurons = network.num_neurons;
    let num_inputs = network.num_inputs;
    let output_start = num_neurons - num_outputs;
    // Sized for the widest tier used below, so the `< R` 8-lane remainder tiles
    // can take a prefix of the same buffer.
    let needed = num_neurons * R.max(SCORING_LANES);
    if network.mse_inter.len() != needed {
        network.mse_inter.resize(needed, 0.0);
    }
    for act in &mut network.batch_activations {
        if act.len() != num_neurons {
            act.resize(num_neurons, 0.0);
        }
    }

    let mut sum_error: f64 = 0.0;

    // ---- full `R`-record tiles through the interleaved gather ---------------
    let full_tiles = num_records / R;
    for tile in 0..full_tiles {
        sum_error = interleaved_tile_mse::<R>(
            network,
            records,
            values_per_record,
            input_size,
            num_outputs,
            tile * R,
            inv_outputs,
            sum_error,
        );
    }

    // ---- `< R` remainder: whole 8-record interleaved tiles ------------------
    let mut remainder_start = full_tiles * R;
    while num_records - remainder_start >= SCORING_LANES {
        sum_error = interleaved_tile_mse::<SCORING_LANES>(
            network,
            records,
            values_per_record,
            input_size,
            num_outputs,
            remainder_start,
            inv_outputs,
            sum_error,
        );
        remainder_start += SCORING_LANES;
    }

    // ---- `< 8` remainder: 4-record group then scalar tail -------------------
    // Kept on the exact per-lane kernels of the scattered path so the numerics
    // match bit-for-bit. This branch never sees an aggregate neuron.
    // Reuses `batch_activations` (Issue #155 / NEAT-AI-scorer#531).
    let remaining = num_records - remainder_start;

    if remaining >= 4 {
        let base_idx = remainder_start;
        for r in 0..4 {
            let base = (base_idx + r) * values_per_record;
            load_record(
                &mut network.batch_activations[r],
                &records[base..base + input_size],
                num_inputs,
            );
        }

        for (neuron_idx, neuron) in network.neurons.iter().enumerate() {
            let actual_idx = num_inputs + neuron_idx;
            if neuron.is_constant {
                let val = apply_limit_range(SquashType::Identity, neuron.bias);
                for act in &mut network.batch_activations {
                    act[actual_idx] = val;
                }
                continue;
            }
            let squash = SquashType::from(neuron.squash_type);
            let start_synapse = neuron.start_synapse as usize;
            let end_synapse = start_synapse + neuron.num_synapses as usize;
            // SAFETY: loaded `CompiledNetwork` — `new` rejected every out-of-range
            // `from_index`, and every activation buffer is sized to `num_neurons`.
            let (sum0, sum1, sum2, sum3) = unsafe {
                weighted_sum_simd_4records_unchecked(
                    &network.synapses,
                    &network.batch_activations[0],
                    &network.batch_activations[1],
                    &network.batch_activations[2],
                    &network.batch_activations[3],
                    start_synapse,
                    end_synapse,
                    neuron.bias,
                )
            };
            let sums = [sum0, sum1, sum2, sum3];
            let squashed = match squash_x4(squash, sums) {
                Some(vec) => vec,
                None => sums.map(|sum| inline_squash(neuron.squash_type, squash, sum)),
            };
            let (low, high) = apply_get_range(squash);
            network.batch_activations[0][actual_idx] =
                apply_limit_range_bounds(low, high, squashed[0]);
            network.batch_activations[1][actual_idx] =
                apply_limit_range_bounds(low, high, squashed[1]);
            network.batch_activations[2][actual_idx] =
                apply_limit_range_bounds(low, high, squashed[2]);
            network.batch_activations[3][actual_idx] =
                apply_limit_range_bounds(low, high, squashed[3]);
        }

        for r in 0..4 {
            let target_base = (base_idx + r) * values_per_record + input_size;
            let act = &network.batch_activations[r];
            let mut sq_sum: f64 = 0.0;
            for j in 0..num_outputs {
                let diff = (records[target_base + j] - act[output_start + j]) as f64;
                sq_sum += diff * diff;
            }
            sum_error += sq_sum * inv_outputs;
        }
    }

    // Scalar single-record tail (`remaining % 4`), bit-identical to `activate`.
    let final_remainder_start = remainder_start + (remaining / 4) * 4;
    for record_idx in final_remainder_start..num_records {
        let base = record_idx * values_per_record;
        let target_base = base + input_size;
        load_record(
            &mut network.batch_activations[0],
            &records[base..base + input_size],
            num_inputs,
        );
        for activation in network.batch_activations[0]
            .iter_mut()
            .take(num_neurons)
            .skip(num_inputs)
        {
            *activation = 0.0;
        }
        for (neuron_idx, neuron) in network.neurons.iter().enumerate() {
            let value =
                neuron_activation_scalar(&network.synapses, &network.batch_activations[0], neuron);
            network.batch_activations[0][num_inputs + neuron_idx] = value;
        }
        let mut sq_sum: f64 = 0.0;
        for j in 0..num_outputs {
            let diff =
                (records[target_base + j] - network.batch_activations[0][output_start + j]) as f64;
            sq_sum += diff * diff;
        }
        sum_error += sq_sum * inv_outputs;
    }

    sum_error
}

/// Scattered per-lane fused activate + MSE (Issue #1209), used for networks
/// containing an aggregate squash (Issue #384) and for the 4..7-record
/// remainder of any forward-only batch.
///
/// Issue #445 — MSE runs the shared [`batch_8way_activation`] skeleton like
/// every other loss kind: 8-record groups, then a 4-record group, then a scalar
/// tail, with only the per-record squared-error reduction supplied here. The
/// numerics are unchanged — the same kernels in the same order as the
/// hand-inlined 8-way and 4-way copies this replaces.
fn mse_sum_batch_scattered(
    network: &CompiledNetwork,
    records: &[f32],
    values_per_record: usize,
    input_size: usize,
    num_outputs: usize,
    num_records: usize,
) -> f64 {
    let inv_outputs: f64 = if num_outputs > 0 {
        1.0 / (num_outputs as f64)
    } else {
        return 0.0;
    };

    // MSE error calculation: mean((target - output)^2)
    let mse_error = |records: &[f32],
                     target_base: usize,
                     act: &[f32],
                     output_start: usize,
                     num_outputs: usize|
     -> f64 {
        let mut sq_sum: f64 = 0.0;
        for j in 0..num_outputs {
            let diff = (records[target_base + j] - act[output_start + j]) as f64;
            sq_sum += diff * diff;
        }
        sq_sum * inv_outputs
    };

    batch_8way_activation!(
        network,
        records,
        values_per_record,
        input_size,
        num_outputs,
        num_records,
        mse_error
    )
}

/// Issue #1209 - Batched MAE with 8-record SIMD parallelism.
fn mae_sum_batch_8way(
    network: &CompiledNetwork,
    records: &[f32],
    values_per_record: usize,
    input_size: usize,
    num_outputs: usize,
    num_records: usize,
) -> f64 {
    let inv_outputs: f64 = if num_outputs > 0 {
        1.0 / (num_outputs as f64)
    } else {
        return 0.0;
    };

    // MAE error calculation: mean(|target - output|)
    let mae_error = |records: &[f32],
                     target_base: usize,
                     act: &[f32],
                     output_start: usize,
                     num_outputs: usize|
     -> f64 {
        let mut abs_sum: f64 = 0.0;
        for j in 0..num_outputs {
            let diff = (records[target_base + j] - act[output_start + j]) as f64;
            abs_sum += diff.abs();
        }
        abs_sum * inv_outputs
    };

    batch_8way_activation!(
        network,
        records,
        values_per_record,
        input_size,
        num_outputs,
        num_records,
        mae_error
    )
}

/// Issue #1209 - Batched Cross-Entropy with 8-record SIMD parallelism.
fn cross_entropy_sum_batch_8way(
    network: &CompiledNetwork,
    records: &[f32],
    values_per_record: usize,
    input_size: usize,
    num_outputs: usize,
    num_records: usize,
) -> f64 {
    let inv_outputs: f64 = if num_outputs > 0 {
        1.0 / (num_outputs as f64)
    } else {
        return 0.0;
    };

    const EPSILON: f64 = 1e-15;

    // Cross-Entropy error calculation: -(1/n) * Σ(t * log(o) + (1-t) * log(1-o))
    let ce_error = |records: &[f32],
                    target_base: usize,
                    act: &[f32],
                    output_start: usize,
                    num_outputs: usize|
     -> f64 {
        let mut ce_sum: f64 = 0.0;
        for j in 0..num_outputs {
            let t = records[target_base + j] as f64;
            let o_raw = act[output_start + j] as f64;
            let o = o_raw.clamp(EPSILON, 1.0 - EPSILON);
            ce_sum -= t * o.ln() + (1.0 - t) * (1.0 - o).ln();
        }
        ce_sum * inv_outputs
    };

    batch_8way_activation!(
        network,
        records,
        values_per_record,
        input_size,
        num_outputs,
        num_records,
        ce_error
    )
}

/// Issue #1209 - Batched MAPE with 8-record SIMD parallelism.
fn mape_sum_batch_8way(
    network: &CompiledNetwork,
    records: &[f32],
    values_per_record: usize,
    input_size: usize,
    num_outputs: usize,
    num_records: usize,
) -> f64 {
    let inv_outputs: f64 = if num_outputs > 0 {
        1.0 / (num_outputs as f64)
    } else {
        return 0.0;
    };

    const EPSILON: f64 = 1e-15;

    // MAPE error calculation: (1/n) * Σ|(output - target) / max(target, ε)|
    let mape_error = |records: &[f32],
                      target_base: usize,
                      act: &[f32],
                      output_start: usize,
                      num_outputs: usize|
     -> f64 {
        let mut mape_sum: f64 = 0.0;
        for j in 0..num_outputs {
            let t = (records[target_base + j] as f64).max(EPSILON);
            let o = act[output_start + j] as f64;
            mape_sum += ((o - t) / t).abs();
        }
        mape_sum * inv_outputs
    };

    batch_8way_activation!(
        network,
        records,
        values_per_record,
        input_size,
        num_outputs,
        num_records,
        mape_error
    )
}

/// Issue #1209 - Batched MSLE with 8-record SIMD parallelism.
fn msle_sum_batch_8way(
    network: &CompiledNetwork,
    records: &[f32],
    values_per_record: usize,
    input_size: usize,
    num_outputs: usize,
    num_records: usize,
) -> f64 {
    if num_outputs == 0 {
        return 0.0;
    }

    const EPSILON: f64 = 1e-15;

    // MSLE error calculation: Σ(log(max(target, ε)) - log(max(output, ε)))
    // Note: No averaging per record to match JS implementation
    let msle_error = |records: &[f32],
                      target_base: usize,
                      act: &[f32],
                      output_start: usize,
                      num_outputs: usize|
     -> f64 {
        let mut msle_sum: f64 = 0.0;
        for j in 0..num_outputs {
            let t = (records[target_base + j] as f64).max(EPSILON);
            let o = (act[output_start + j] as f64).max(EPSILON);
            msle_sum += t.ln() - o.ln();
        }
        msle_sum
    };

    batch_8way_activation!(
        network,
        records,
        values_per_record,
        input_size,
        num_outputs,
        num_records,
        msle_error
    )
}

/// Issue #1209 - Batched Hinge with 8-record SIMD parallelism.
fn hinge_sum_batch_8way(
    network: &CompiledNetwork,
    records: &[f32],
    values_per_record: usize,
    input_size: usize,
    num_outputs: usize,
    num_records: usize,
) -> f64 {
    if num_outputs == 0 {
        return 0.0;
    }

    // Hinge error calculation: Σmax(0, 1 - target * output)
    // Note: No averaging per record to match JS implementation
    let hinge_error = |records: &[f32],
                       target_base: usize,
                       act: &[f32],
                       output_start: usize,
                       num_outputs: usize|
     -> f64 {
        let mut hinge_sum: f64 = 0.0;
        for j in 0..num_outputs {
            let t = records[target_base + j] as f64;
            let o = act[output_start + j] as f64;
            hinge_sum += (1.0 - t * o).max(0.0);
        }
        hinge_sum
    };

    batch_8way_activation!(
        network,
        records,
        values_per_record,
        input_size,
        num_outputs,
        num_records,
        hinge_error
    )
}

/// Fused activate + MAE (Mean Absolute Error) calculation for batch scoring.
///
/// Like `mse_sum_batch_packed`, this processes a batch of `[inputs..., targets...]` records
/// in a single WASM call, returning the sum of per-record MAE errors.
///
/// MAE formula per record: (1/n) * Σ|target - output|
///
/// # Arguments
/// * `network` - The compiled network to activate
/// * `records` - Packed array of `[inputs..., targets...]` records
/// * `input_size` - Number of inputs per record
/// * `num_outputs` - Number of outputs per record
/// * `forward_only` - If true, skip reset_state() (for forward-only networks)
///
/// # Returns
/// Sum of per-record MAE errors (divide by record count for mean)
#[cfg_attr(target_family = "wasm", wasm_bindgen)]
pub fn mae_sum_batch_packed(
    network: &mut CompiledNetwork,
    records: &[f32],
    input_size: usize,
    num_outputs: usize,
    forward_only: bool,
) -> f64 {
    let Some(layout) = packed_layout(records.len(), input_size, num_outputs) else {
        return 0.0;
    };

    // Issue #1209 - Use batched 8-record SIMD path for forward-only networks
    if forward_only && layout.num_records >= 8 {
        return mae_sum_batch_8way(
            network,
            records,
            layout.values_per_record,
            input_size,
            num_outputs,
            layout.num_records,
        );
    }

    let inv_outputs: f64 = if num_outputs > 0 {
        1.0 / (num_outputs as f64)
    } else {
        0.0
    };

    packed_record_scan(
        network,
        records,
        input_size,
        num_outputs,
        forward_only,
        |targets, outputs| {
            // Per-record MAE = mean(|target - output|)
            let mut abs_sum: f64 = 0.0;
            for (t, o) in targets.iter().zip(outputs.iter()) {
                abs_sum += ((*t - *o) as f64).abs();
            }
            abs_sum * inv_outputs
        },
    )
}

/// Fused activate + Cross Entropy calculation for batch scoring.
///
/// Cross Entropy formula per record: -(1/n) * Σ(t * log(o) + (1-t) * log(1-o))
/// Output values are clamped to [1e-15, 1-1e-15] to prevent log(0).
///
/// # Arguments
/// * `network` - The compiled network to activate
/// * `records` - Packed array of `[inputs..., targets...]` records
/// * `input_size` - Number of inputs per record
/// * `num_outputs` - Number of outputs per record
/// * `forward_only` - If true, skip reset_state() (for forward-only networks)
///
/// # Returns
/// Sum of per-record Cross Entropy errors (divide by record count for mean)
#[cfg_attr(target_family = "wasm", wasm_bindgen)]
pub fn cross_entropy_sum_batch_packed(
    network: &mut CompiledNetwork,
    records: &[f32],
    input_size: usize,
    num_outputs: usize,
    forward_only: bool,
) -> f64 {
    let Some(layout) = packed_layout(records.len(), input_size, num_outputs) else {
        return 0.0;
    };

    // Issue #1209 - Use batched 8-record SIMD path for forward-only networks
    if forward_only && layout.num_records >= 8 {
        return cross_entropy_sum_batch_8way(
            network,
            records,
            layout.values_per_record,
            input_size,
            num_outputs,
            layout.num_records,
        );
    }

    let inv_outputs: f64 = if num_outputs > 0 {
        1.0 / (num_outputs as f64)
    } else {
        0.0
    };

    const EPSILON: f64 = 1e-15;

    packed_record_scan(
        network,
        records,
        input_size,
        num_outputs,
        forward_only,
        |targets, outputs| {
            // Per-record Cross Entropy = -(1/n) * Σ(t * log(o) + (1-t) * log(1-o))
            let mut ce_sum: f64 = 0.0;
            for (t, o_raw) in targets.iter().zip(outputs.iter()) {
                let t = *t as f64;
                // Clamp to [epsilon, 1-epsilon] to prevent log(0)
                let o = (*o_raw as f64).clamp(EPSILON, 1.0 - EPSILON);
                ce_sum -= t * o.ln() + (1.0 - t) * (1.0 - o).ln();
            }
            ce_sum * inv_outputs
        },
    )
}

/// Fused activate + MAPE (Mean Absolute Percentage Error) calculation for batch scoring.
///
/// MAPE formula per record: (1/n) * Σ|(output - target) / max(target, ε)|
///
/// # Arguments
/// * `network` - The compiled network to activate
/// * `records` - Packed array of `[inputs..., targets...]` records
/// * `input_size` - Number of inputs per record
/// * `num_outputs` - Number of outputs per record
/// * `forward_only` - If true, skip reset_state() (for forward-only networks)
///
/// # Returns
/// Sum of per-record MAPE errors (divide by record count for mean)
#[cfg_attr(target_family = "wasm", wasm_bindgen)]
pub fn mape_sum_batch_packed(
    network: &mut CompiledNetwork,
    records: &[f32],
    input_size: usize,
    num_outputs: usize,
    forward_only: bool,
) -> f64 {
    let Some(layout) = packed_layout(records.len(), input_size, num_outputs) else {
        return 0.0;
    };

    // Issue #1209 - Use batched 8-record SIMD path for forward-only networks
    if forward_only && layout.num_records >= 8 {
        return mape_sum_batch_8way(
            network,
            records,
            layout.values_per_record,
            input_size,
            num_outputs,
            layout.num_records,
        );
    }

    let inv_outputs: f64 = if num_outputs > 0 {
        1.0 / (num_outputs as f64)
    } else {
        0.0
    };

    const EPSILON: f64 = 1e-15;

    packed_record_scan(
        network,
        records,
        input_size,
        num_outputs,
        forward_only,
        |targets, outputs| {
            // Per-record MAPE = (1/n) * Σ|(output - target) / max(target, ε)|
            let mut mape_sum: f64 = 0.0;
            for (t, o) in targets.iter().zip(outputs.iter()) {
                let t = (*t as f64).max(EPSILON);
                mape_sum += ((*o as f64 - t) / t).abs();
            }
            mape_sum * inv_outputs
        },
    )
}

/// Fused activate + MSLE (Mean Squared Logarithmic Error) calculation for batch scoring.
///
/// MSLE formula per record: Σ(log(max(target, ε)) - log(max(output, ε)))
/// Note: Unlike MSE/MAE, MSLE does NOT divide by number of outputs per record.
///
/// # Arguments
/// * `network` - The compiled network to activate
/// * `records` - Packed array of `[inputs..., targets...]` records
/// * `input_size` - Number of inputs per record
/// * `num_outputs` - Number of outputs per record
/// * `forward_only` - If true, skip reset_state() (for forward-only networks)
///
/// # Returns
/// Sum of per-record MSLE errors (divide by record count for mean)
#[cfg_attr(target_family = "wasm", wasm_bindgen)]
pub fn msle_sum_batch_packed(
    network: &mut CompiledNetwork,
    records: &[f32],
    input_size: usize,
    num_outputs: usize,
    forward_only: bool,
) -> f64 {
    let Some(layout) = packed_layout(records.len(), input_size, num_outputs) else {
        return 0.0;
    };

    // Issue #1209 - Use batched 8-record SIMD path for forward-only networks
    if forward_only && layout.num_records >= 8 {
        return msle_sum_batch_8way(
            network,
            records,
            layout.values_per_record,
            input_size,
            num_outputs,
            layout.num_records,
        );
    }

    const EPSILON: f64 = 1e-15;

    packed_record_scan(
        network,
        records,
        input_size,
        num_outputs,
        forward_only,
        |targets, outputs| {
            // Per-record MSLE = Σ(log(max(target, ε)) - log(max(output, ε)))
            // Note: No averaging per record to match JS implementation
            let mut msle_sum: f64 = 0.0;
            for (t, o) in targets.iter().zip(outputs.iter()) {
                let t = (*t as f64).max(EPSILON);
                let o = (*o as f64).max(EPSILON);
                msle_sum += t.ln() - o.ln();
            }
            msle_sum
        },
    )
}

/// Fused activate + Hinge Loss calculation for batch scoring.
///
/// Hinge formula per record: Σmax(0, 1 - target * output)
/// Note: Unlike MSE/MAE, Hinge does NOT divide by number of outputs per record.
///
/// # Arguments
/// * `network` - The compiled network to activate
/// * `records` - Packed array of `[inputs..., targets...]` records
/// * `input_size` - Number of inputs per record
/// * `num_outputs` - Number of outputs per record
/// * `forward_only` - If true, skip reset_state() (for forward-only networks)
///
/// # Returns
/// Sum of per-record Hinge errors (divide by record count for mean)
#[cfg_attr(target_family = "wasm", wasm_bindgen)]
pub fn hinge_sum_batch_packed(
    network: &mut CompiledNetwork,
    records: &[f32],
    input_size: usize,
    num_outputs: usize,
    forward_only: bool,
) -> f64 {
    let Some(layout) = packed_layout(records.len(), input_size, num_outputs) else {
        return 0.0;
    };

    // Issue #1209 - Use batched 8-record SIMD path for forward-only networks
    if forward_only && layout.num_records >= 8 {
        return hinge_sum_batch_8way(
            network,
            records,
            layout.values_per_record,
            input_size,
            num_outputs,
            layout.num_records,
        );
    }

    packed_record_scan(
        network,
        records,
        input_size,
        num_outputs,
        forward_only,
        |targets, outputs| {
            // Per-record Hinge = Σmax(0, 1 - target * output)
            // Note: No averaging per record to match JS implementation
            let mut hinge_sum: f64 = 0.0;
            for (t, o) in targets.iter().zip(outputs.iter()) {
                hinge_sum += (1.0 - (*t as f64) * (*o as f64)).max(0.0);
            }
            hinge_sum
        },
    )
}

/// Fused activate + Categorical Error (argmax misclassification) for batch scoring.
///
/// Reference TypeScript: `NEAT-AI/src/costs/CategoricalError.ts`. For each
/// record, this compares the index of the largest target value (the true
/// class) with the index of the largest output value (the predicted class).
/// Each record contributes `0` for a correct prediction or `1` for an
/// incorrect one. The returned sum is therefore the **count of
/// misclassified records**; divide by `record_count` to obtain the mean
/// error rate (`1 - accuracy`).
///
/// Ties resolve to the first index (standard argmax convention) on both the
/// target and output sides, matching the TS reference.
///
/// This metric is intentionally **non-differentiable** — it is intended as a
/// scoring / early-stop signal, not a gradient source.
///
/// # Arguments
/// * `network` - The compiled network to activate
/// * `records` - Packed array of `[inputs..., targets...]` records
/// * `input_size` - Number of inputs per record
/// * `num_outputs` - Number of outputs per record
/// * `forward_only` - If true, skip reset_state() (for forward-only networks)
///
/// # Returns
/// Count of misclassified records (divide by record count for mean error
/// rate). Returns `0.0` when the record set is empty or `input_size +
/// num_outputs == 0`.
///
/// Issue stSoftwareAU/NEAT-AI-core#88 — extend native scorer with the last
/// remaining built-in NEAT-AI cost function.
#[cfg_attr(target_family = "wasm", wasm_bindgen)]
pub fn categorical_error_sum_batch_packed(
    network: &mut CompiledNetwork,
    records: &[f32],
    input_size: usize,
    num_outputs: usize,
    forward_only: bool,
) -> f64 {
    // A record with no outputs has no class to predict — guard before the scan,
    // whose closure indexes the first target and output.
    if num_outputs == 0 {
        return 0.0;
    }

    /// Argmax with first-index tie-breaking — matches the TS reference
    /// (`a > b` so equal values keep the earlier index).
    fn argmax(values: &[f32]) -> usize {
        let mut best_idx: usize = 0;
        let mut best: f32 = values[0];
        for (idx, v) in values.iter().enumerate().skip(1) {
            if *v > best {
                best = *v;
                best_idx = idx;
            }
        }
        best_idx
    }

    packed_record_scan(
        network,
        records,
        input_size,
        num_outputs,
        forward_only,
        |targets, outputs| {
            if argmax(targets) != argmax(outputs) {
                1.0
            } else {
                0.0
            }
        },
    )
}

/// Non-fused recurrent-path MSE for `forwardOnly: false` networks.
///
/// Activates the network **once per record**, resetting hidden activations
/// between records to preserve stateless semantics (`feedbackLoop = false`),
/// and returns the **mean per-record MSE** — matching the TypeScript
/// `MSE.calculate()` semantics used in NEAT-AI.
///
/// Per record:
/// - Squared error is computed for each output: `(target - output)^2`.
/// - The per-record MSE is `mean(diff^2)` across the record's outputs.
///
/// Across records the per-record MSEs are averaged, giving:
///
/// `(1 / num_records) * Σ_records (1 / num_outputs) * Σ_outputs (t - o)^2`
///
/// This is algebraically `mse_sum_batch_packed(..) / num_records` on any
/// forward-only fixture where the two paths see the same activations.
///
/// # Edge cases
/// Returns `0.0` when either `records` is empty or `input_size + num_outputs`
/// is zero. An empty input set has no error to report, matching the
/// convention used by the existing `*_sum_batch_packed` helpers.
///
/// Issue stSoftwareAU/NEAT-AI-core#15 — unblocks `rust_scorer` compile.
pub fn mse_mean_record(
    network: &mut CompiledNetwork,
    records: &[f32],
    input_size: usize,
    num_outputs: usize,
) -> f64 {
    let Some(layout) = packed_layout(records.len(), input_size, num_outputs) else {
        return 0.0;
    };

    // Non-fused recurrent path: `forward_only = false` clears hidden state
    // between records so the previous record's activations cannot leak in.
    // Issue #538 — the per-record reduction lives in `mse_record`.
    let sum_error =
        packed_record_scan(network, records, input_size, num_outputs, false, mse_record);

    sum_error / (layout.num_records as f64)
}

/// Append the little-endian `f32` values in `bytes` to `out`.
///
/// A trailing 1..3 bytes cannot form a value and are ignored; callers hand
/// this helper whole records only.
fn append_le_f32(bytes: &[u8], out: &mut Vec<f32>) {
    out.extend(
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .copied()
            .map(f32::from_le_bytes),
    );
}

/// Drop every `.bin` file past the one that covers `max_records`, so a capped
/// scan never opens a file it cannot use.
///
/// Fails loud when a listed file cannot be stat-ed — a corpus we cannot size
/// is not a corpus we can cap.
fn truncate_bin_files_to_cap(
    bin_files: &mut Vec<PathBuf>,
    record_bytes: usize,
    max_records: u64,
) -> Result<(), String> {
    let budget_bytes = (max_records as u128) * (record_bytes as u128);
    let mut covered: u128 = 0;
    for (idx, path) in bin_files.iter().enumerate() {
        let len = std::fs::metadata(path)
            .map_err(|e| format!("failed to stat training file '{}': {e}", path.display()))?
            .len();
        covered += len as u128;
        if covered >= budget_bytes {
            bin_files.truncate(idx + 1);
            return Ok(());
        }
    }
    Ok(())
}

/// Streaming mean per-record MSE over a `.bin` training directory.
///
/// Semantics match [`mse_mean_record`] and NEAT-AI's `Costs.MSE`:
///
/// `(1 / N) * Σ_records mse_record(targets, outputs)`
///
/// — the per-record mean over outputs, then averaged over records.
///
/// Records are buffered into packed `[inputs…, targets…]` chunks and scored
/// through [`mse_sum_batch_packed`], so a `forward_only` corpus still takes the
/// fused SIMD path; a record straddling a read-chunk (or file) boundary is
/// carried in a residual buffer and scored with the next chunk. `forward_only
/// = false` keeps stateless semantics — `mse_sum_batch_packed` resets the
/// network before every record on that route.
///
/// `max_records` caps the scan: files past the cap are never opened and the
/// final chunk is truncated, so the cap costs no extra I/O. `Some(0)` reads
/// nothing.
///
/// Returns `(mean_mse, record_count)`, or `(0.0, 0)` when the directory yields
/// no whole records — the caller decides whether an empty corpus is an error.
///
/// # Errors
/// - the path is not an existing directory;
/// - a `.bin` file cannot be listed, stat-ed, opened, or read;
/// - the corpus ends mid-record (trailing bytes that form no whole record).
///
/// This is a native-host convenience and is deliberately **not** part of the
/// `wasm_bindgen` export surface.
///
/// Issue #538 — one streaming directory MSE loop for every consumer.
pub fn mse_mean_streaming(
    network: &mut CompiledNetwork,
    training_data: &Path,
    input_size: usize,
    num_outputs: usize,
    forward_only: bool,
    max_records: Option<u64>,
) -> Result<(f64, u64), String> {
    let values_per_record = input_size + num_outputs;
    if values_per_record == 0 || max_records == Some(0) {
        return Ok((0.0, 0));
    }
    if !training_data.is_dir() {
        return Err(format!(
            "training data path '{}' is not an existing directory",
            training_data.display()
        ));
    }

    let record_bytes = values_per_record * std::mem::size_of::<f32>();
    let mut bin_files = find_bin_files(training_data).map_err(|e| {
        format!(
            "failed to list .bin files in '{}': {e}",
            training_data.display()
        )
    })?;
    if bin_files.is_empty() {
        return Ok((0.0, 0));
    }
    if let Some(max) = max_records {
        truncate_bin_files_to_cap(&mut bin_files, record_bytes, max)?;
    }

    let (mode, read_buf_len) = training_read_tuning_from_env(record_bytes);

    let mut sum_error: f64 = 0.0;
    let mut records_done: u64 = 0;
    // Bytes of a record split across a chunk (or file) boundary.
    let mut pending: Vec<u8> = Vec::with_capacity(record_bytes);
    let mut packed: Vec<f32> = Vec::new();
    let mut capped = false;

    for_each_read_chunk_with_mode(&bin_files, read_buf_len, mode, |chunk| {
        if capped {
            return Ok(());
        }

        let mut bytes = chunk;
        packed.clear();

        // Finish the record left straddling the previous chunk first, so the
        // whole chunk still scores through one batched call.
        if !pending.is_empty() {
            let take = (record_bytes - pending.len()).min(bytes.len());
            pending.extend_from_slice(&bytes[..take]);
            bytes = &bytes[take..];
            if pending.len() < record_bytes {
                return Ok(());
            }
            append_le_f32(&pending, &mut packed);
            pending.clear();
        }

        let whole_bytes = (bytes.len() / record_bytes) * record_bytes;
        append_le_f32(&bytes[..whole_bytes], &mut packed);
        pending.extend_from_slice(&bytes[whole_bytes..]);

        let mut chunk_records = packed.len() / values_per_record;
        if let Some(max) = max_records {
            let remaining = max - records_done;
            if chunk_records as u64 >= remaining {
                chunk_records = remaining as usize;
                packed.truncate(chunk_records * values_per_record);
                capped = true;
            }
        }
        if chunk_records == 0 {
            return Ok(());
        }

        sum_error += mse_sum_batch_packed(network, &packed, input_size, num_outputs, forward_only);
        records_done += chunk_records as u64;
        Ok(())
    })?;

    // A corpus that ends mid-record is malformed — say so rather than
    // silently dropping the tail. A capped scan stops early by design, so its
    // residual is expected.
    if !pending.is_empty() && !capped {
        return Err(format!(
            "training data in '{}' ends with {} trailing bytes that do not form a whole {record_bytes}-byte record",
            training_data.display(),
            pending.len()
        ));
    }

    if records_done == 0 {
        return Ok((0.0, 0));
    }
    Ok((sum_error / records_done as f64, records_done))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{compile_creature, parse_creature_json};

    /// Simple 2-input, 1-output identity creature:
    ///   output = 0.5 * in0 + (-0.3) * in1 + 0.1
    /// Forward-only — the `forward_only=true` batch path is numerically
    /// identical to the recurrent reset-per-record path.
    fn linear_creature_json() -> &'static str {
        r#"{
            "input": 2,
            "output": 1,
            "neurons": [
                {"type": "output", "uuid": "output-0", "bias": 0.1, "squash": "IDENTITY"}
            ],
            "synapses": [
                {"fromUUID": "input-0", "toUUID": "output-0", "weight": 0.5},
                {"fromUUID": "input-1", "toUUID": "output-0", "weight": -0.3}
            ],
            "forwardOnly": true
        }"#
    }

    #[test]
    fn mse_mean_record_matches_hand_rolled_reference() {
        let creature = parse_creature_json(linear_creature_json()).expect("parse");
        let mut network = compile_creature(&creature).expect("compile");

        // Three records: [in0, in1, target]
        let records: Vec<f32> = vec![
            1.0, 0.5, 0.4, // output = 0.45, diff = -0.05
            0.0, 0.0, 0.0, // output = 0.1,  diff = -0.1
            -1.0, 2.0, -1.0, // output = 0.5*(-1) + (-0.3)*2 + 0.1 = -1.0, diff = 0.0
        ];

        let actual = mse_mean_record(&mut network, &records, 2, 1);

        // num_outputs = 1 so per-record MSE == diff^2.
        let per_record_mse = [
            0.05f64 * 0.05,
            0.1f64 * 0.1,
            0.0f64, // exact: 0.5*(-1) + (-0.3)*2 + 0.1 = -1.0
        ];
        let expected: f64 = per_record_mse.iter().sum::<f64>() / per_record_mse.len() as f64;

        assert!(
            (actual - expected).abs() < 1e-6,
            "mse_mean_record = {actual}, expected {expected}"
        );
    }

    #[test]
    fn mse_mean_record_agrees_with_sum_divided_by_records_on_forward_only() {
        let creature = parse_creature_json(linear_creature_json()).expect("parse");
        let mut net_mean = compile_creature(&creature).expect("compile");
        let mut net_sum = compile_creature(&creature).expect("compile");

        // Use enough records to exercise both scalar and SIMD paths in
        // `mse_sum_batch_packed` (>= 8 triggers the 8-way batch).
        let records: Vec<f32> = vec![
            1.0, 0.5, 0.4, //
            0.0, 0.0, 0.0, //
            -1.0, 2.0, -1.0, //
            0.25, -0.75, 0.2, //
            2.0, 1.0, 0.8, //
            -0.5, -0.5, 0.0, //
            1.5, 0.5, 0.5, //
            0.1, 0.2, 0.15, //
            0.9, -0.1, 0.6, //
        ];
        let input_size = 2;
        let num_outputs = 1;
        let num_records = records.len() / (input_size + num_outputs);

        let mean = mse_mean_record(&mut net_mean, &records, input_size, num_outputs);
        let sum = mse_sum_batch_packed(&mut net_sum, &records, input_size, num_outputs, true);
        let expected = sum / num_records as f64;

        assert!(
            (mean - expected).abs() < 1e-6,
            "mse_mean_record = {mean}, mse_sum_batch_packed / {num_records} = {expected}"
        );
    }

    #[test]
    fn mse_mean_record_empty_input_returns_zero() {
        let creature = parse_creature_json(linear_creature_json()).expect("parse");
        let mut network = compile_creature(&creature).expect("compile");

        // Zero records — no error to report.
        let records: Vec<f32> = Vec::new();
        assert_eq!(mse_mean_record(&mut network, &records, 2, 1), 0.0);

        // Zero input_size + zero num_outputs — degenerate, also 0.0.
        assert_eq!(mse_mean_record(&mut network, &records, 0, 0), 0.0);
    }

    // ----- categorical_error_sum_batch_packed (issue #88) -----
    //
    // 2-input / 3-output linear creature used to test argmax classification.
    // All outputs use IDENTITY so we can hand-roll exact activations:
    //   output-0 = 1.0 * in0 + 0.0 * in1
    //   output-1 = 0.0 * in0 + 1.0 * in1
    //   output-2 = 0.5 * in0 + 0.5 * in1
    //
    // Sample activations:
    //   (1, 0) -> [1.0, 0.0, 0.5]   argmax = 0
    //   (0, 1) -> [0.0, 1.0, 0.5]   argmax = 1
    //   (2, 1) -> [2.0, 1.0, 1.5]   argmax = 0
    //   (1, 1) -> [1.0, 1.0, 1.0]   argmax = 0 (3-way tie, first wins)
    fn three_class_creature_json() -> &'static str {
        r#"{
            "input": 2,
            "output": 3,
            "neurons": [
                {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"},
                {"type": "output", "uuid": "output-1", "bias": 0.0, "squash": "IDENTITY"},
                {"type": "output", "uuid": "output-2", "bias": 0.0, "squash": "IDENTITY"}
            ],
            "synapses": [
                {"fromUUID": "input-0", "toUUID": "output-0", "weight": 1.0},
                {"fromUUID": "input-1", "toUUID": "output-0", "weight": 0.0},
                {"fromUUID": "input-0", "toUUID": "output-1", "weight": 0.0},
                {"fromUUID": "input-1", "toUUID": "output-1", "weight": 1.0},
                {"fromUUID": "input-0", "toUUID": "output-2", "weight": 0.5},
                {"fromUUID": "input-1", "toUUID": "output-2", "weight": 0.5}
            ],
            "forwardOnly": true
        }"#
    }

    #[test]
    fn categorical_error_perfect_prediction_returns_zero() {
        let creature = parse_creature_json(three_class_creature_json()).expect("parse");
        let mut network = compile_creature(&creature).expect("compile");

        // Each record's target argmax matches the output argmax above.
        #[rustfmt::skip]
        let records: Vec<f32> = vec![
            1.0, 0.0, /* target */ 1.0, 0.0, 0.0, // out argmax 0, tgt argmax 0
            0.0, 1.0, /* target */ 0.0, 1.0, 0.0, // out argmax 1, tgt argmax 1
            2.0, 1.0, /* target */ 1.0, 0.0, 0.0, // out argmax 0, tgt argmax 0
        ];

        let err = categorical_error_sum_batch_packed(&mut network, &records, 2, 3, true);
        assert_eq!(err, 0.0);
    }

    #[test]
    fn categorical_error_all_wrong_returns_record_count() {
        let creature = parse_creature_json(three_class_creature_json()).expect("parse");
        let mut network = compile_creature(&creature).expect("compile");

        // Every record's target argmax disagrees with the output argmax.
        #[rustfmt::skip]
        let records: Vec<f32> = vec![
            1.0, 0.0, /* target */ 0.0, 1.0, 0.0, // out argmax 0, tgt argmax 1
            0.0, 1.0, /* target */ 1.0, 0.0, 0.0, // out argmax 1, tgt argmax 0
            2.0, 1.0, /* target */ 0.0, 0.0, 1.0, // out argmax 0, tgt argmax 2
        ];

        let err = categorical_error_sum_batch_packed(&mut network, &records, 2, 3, true);
        assert_eq!(err, 3.0);
    }

    #[test]
    fn categorical_error_empty_input_returns_zero() {
        let creature = parse_creature_json(three_class_creature_json()).expect("parse");
        let mut network = compile_creature(&creature).expect("compile");

        // Empty record set.
        let empty: Vec<f32> = Vec::new();
        assert_eq!(
            categorical_error_sum_batch_packed(&mut network, &empty, 2, 3, true),
            0.0
        );

        // Degenerate input_size + num_outputs == 0.
        assert_eq!(
            categorical_error_sum_batch_packed(&mut network, &empty, 0, 0, true),
            0.0
        );

        // Degenerate num_outputs == 0 with non-empty input_size.
        assert_eq!(
            categorical_error_sum_batch_packed(&mut network, &empty, 2, 0, true),
            0.0
        );
    }

    #[test]
    fn categorical_error_ties_resolve_to_first_index() {
        let creature = parse_creature_json(three_class_creature_json()).expect("parse");
        let mut network = compile_creature(&creature).expect("compile");

        // Input (1, 1) -> outputs [1.0, 1.0, 1.0]: 3-way tie, argmax = 0.
        // Record 1: target [0.5, 0.5, 0.4] -> tgt argmax = 0 (first wins) -> correct.
        // Record 2: target [0.4, 0.5, 0.5] -> tgt argmax = 1 (first of tied
        //           1 and 2) -> mismatch with output argmax 0 -> 1 error.
        // Record 3: target [0.5, 0.4, 0.5] -> tgt argmax = 0 (first of tied
        //           0 and 2) -> correct.
        #[rustfmt::skip]
        let records: Vec<f32> = vec![
            1.0, 1.0, /* target */ 0.5, 0.5, 0.4,
            1.0, 1.0, /* target */ 0.4, 0.5, 0.5,
            1.0, 1.0, /* target */ 0.5, 0.4, 0.5,
        ];

        let err = categorical_error_sum_batch_packed(&mut network, &records, 2, 3, true);
        assert_eq!(err, 1.0);
    }

    #[test]
    fn categorical_error_mixed_batch_matches_reference() {
        let creature = parse_creature_json(three_class_creature_json()).expect("parse");
        let mut network = compile_creature(&creature).expect("compile");

        // Hand-crafted mixed batch — annotate each row with the expected
        // output argmax (from the linear creature above) and target argmax,
        // then the per-record contribution (0 correct, 1 wrong).
        #[rustfmt::skip]
        let records: Vec<f32> = vec![
            // in0, in1, t0, t1, t2 | out argmax | tgt argmax | err
            1.0, 0.0,  1.0, 0.0, 0.0, //     0          0         0
            0.0, 1.0,  1.0, 0.0, 0.0, //     1          0         1
            2.0, 1.0,  0.0, 0.0, 1.0, //     0          2         1
            0.0, 2.0,  0.0, 1.0, 0.0, //     1          1         0
            1.0, 1.0,  0.5, 0.4, 0.4, //     0 (tie)    0 (tie)   0
            1.0, 0.0,  0.0, 1.0, 0.0, //     0          1         1
        ];
        // Total errors expected = 3.

        let err = categorical_error_sum_batch_packed(&mut network, &records, 2, 3, true);
        assert_eq!(err, 3.0);

        // The forward_only=false path takes the same record loop with an
        // additional reset_state() — for this stateless feed-forward
        // creature it must produce the same count.
        let mut net_recurrent = compile_creature(&creature).expect("compile");
        let err_recurrent =
            categorical_error_sum_batch_packed(&mut net_recurrent, &records, 2, 3, false);
        assert_eq!(err_recurrent, 3.0);
    }
}

#[cfg(test)]
mod interleaved_mse_parity {
    //! Issue #384 - bit-identity guard for rerouting the fused MSE batch loss
    //! lane through the #287 record-interleaved gather.
    //!
    //! `mse_sum_batch_8way` now dispatches standard-squash networks to
    //! [`mse_sum_batch_8way_interleaved`] and aggregate networks to the
    //! unchanged [`mse_sum_batch_scattered`]. These "what" tests assert the
    //! interleaved result is **bit-identical** (`f64::to_bits`) to the scattered
    //! path it replaces across the record counts that straddle the 8-record
    //! group boundary, so a lane-transpose bug, a changed `f64` accumulation
    //! order, or a wrong remainder split would break the assertion.

    use super::*;
    use crate::network::{NeuronData, SynapseData};

    /// Build a forward-only network: `num_inputs` inputs fully connected into
    /// two hidden neurons and one output neuron, every non-input neuron using
    /// `squash`. Mirrors the topology of the existing MSE/interleaved parity
    /// tests so the fixtures stay comparable.
    fn build_network(num_inputs: usize, squash: SquashType) -> CompiledNetwork {
        let mut synapses = Vec::new();
        let mut neurons = Vec::new();

        for h in 0..2 {
            let start = synapses.len() as u32;
            for i in 0..num_inputs {
                synapses.push(SynapseData {
                    weight: 0.31 - 0.13 * (i as f32) + 0.09 * (h as f32),
                    from_index: i as u16,
                    synapse_type: 0,
                });
            }
            neurons.push(NeuronData {
                bias: 0.04 * (h as f32) - 0.03,
                start_synapse: start,
                num_synapses: num_inputs as u16,
                squash_type: squash as u8,
                is_constant: false,
            });
        }

        let start = synapses.len() as u32;
        let hidden0 = num_inputs as u16;
        let hidden1 = num_inputs as u16 + 1;
        synapses.push(SynapseData {
            weight: 0.55,
            from_index: hidden0,
            synapse_type: 0,
        });
        synapses.push(SynapseData {
            weight: -0.42,
            from_index: hidden1,
            synapse_type: 0,
        });
        neurons.push(NeuronData {
            bias: 0.02,
            start_synapse: start,
            num_synapses: 2,
            squash_type: squash as u8,
            is_constant: false,
        });

        CompiledNetwork::from_parts(num_inputs, neurons, synapses)
            .expect("fixture must satisfy the load-time index invariant")
    }

    /// Packed `[inputs..., target]` records with distinct per-record values so a
    /// lane mix-up in the transpose or MSE reduction cannot hide.
    fn build_records(num_records: usize, input_size: usize) -> Vec<f32> {
        let values_per_record = input_size + 1;
        let mut records = vec![0.0f32; num_records * values_per_record];
        for r in 0..num_records {
            let base = r * values_per_record;
            for i in 0..input_size {
                records[base + i] = -1.3 + 0.29 * (r as f32) - 0.17 * (i as f32);
            }
            records[base + input_size] = 0.2 + 0.031 * (r as f32);
        }
        records
    }

    /// Assert the tile-`R` interleaved kernel is bit-identical to the scattered
    /// oracle. The oracle reaches the same value by a genuinely independent
    /// route — the per-lane scattered kernels driven by the shared 8 → 4 → 1
    /// skeleton — so a lane-transpose slip, a wrong remainder split, or a
    /// changed `f64` reduction order in the tiled path moves only one side.
    fn assert_bit_identical<const R: usize>(squash: SquashType, num_records: usize) {
        let input_size = 6;
        let mut net = build_network(input_size, squash);
        assert!(
            !net.has_aggregate_squash(),
            "{squash:?} must route through the interleaved path"
        );
        let records = build_records(num_records, input_size);
        let values_per_record = input_size + 1;

        let interleaved = mse_sum_batch_interleaved::<R>(
            &mut net,
            &records,
            values_per_record,
            input_size,
            1,
            num_records,
        );
        let scattered = mse_sum_batch_scattered(
            &net,
            &records,
            values_per_record,
            input_size,
            1,
            num_records,
        );
        assert_eq!(
            interleaved.to_bits(),
            scattered.to_bits(),
            "{squash:?} R={R} n={num_records}: interleaved MSE {interleaved} not bit-identical to scattered {scattered}"
        );
    }

    /// Record counts straddling every tier boundary of the 8-lane ladder:
    /// 8 = one full group; 9 = group + scalar tail; 12 = group + 4-way
    /// remainder; 13/15 = group + 4-way + scalar tail; 16 = two full groups;
    /// 4096 = the production steady state (all full groups). At the wider tiles
    /// these also cover "no full tile at all" (n < R) and "full tile + 8-record
    /// remainder tiles", which is the ladder Issue #530 adds.
    const BOUNDARY_COUNTS: [usize; 15] =
        [8, 9, 12, 13, 15, 16, 17, 24, 31, 32, 33, 40, 64, 71, 4096];

    const PARITY_SQUASHES: [SquashType; 7] = [
        SquashType::Tanh,
        SquashType::Logistic,
        SquashType::Gelu,
        SquashType::Mish,
        SquashType::Relu,
        SquashType::Identity,
        SquashType::Sine,
    ];

    #[test]
    fn interleaved_mse_bit_identical_to_scattered_across_boundaries() {
        for squash in PARITY_SQUASHES {
            for &n in &BOUNDARY_COUNTS {
                assert_bit_identical::<{ SCORING_LANES }>(squash, n);
            }
        }
    }

    /// Issue #530 — widening the record tile must not move a single bit. Each
    /// lane accumulates its own `bias + Σ w·a` in synapse order and every tier
    /// reduces in record order, so tiles of 16/32/64 must agree with the
    /// scattered oracle exactly as the 8-lane tile does. This is the guard that
    /// makes `MSE_TILE_LANES` a free memory-traffic knob.
    #[test]
    fn every_tile_width_is_bit_identical_to_scattered() {
        for squash in PARITY_SQUASHES {
            for &n in &BOUNDARY_COUNTS {
                assert_bit_identical::<16>(squash, n);
                assert_bit_identical::<32>(squash, n);
                assert_bit_identical::<64>(squash, n);
            }
        }
    }

    /// The shipped tile width must itself be one of the widths proven above and
    /// stay inside the kernel's compile-time bounds — a stray value would fail
    /// the build, but this states the contract where a reader looks for it.
    #[test]
    fn shipped_tile_width_is_a_supported_multiple_of_eight() {
        assert!(
            MSE_TILE_LANES.is_multiple_of(SCORING_LANES)
                && MSE_TILE_LANES >= SCORING_LANES
                && MSE_TILE_LANES <= crate::simd::MAX_INTERLEAVED_LANES,
            "MSE_TILE_LANES = {MSE_TILE_LANES} is not a supported tile width"
        );
        for squash in PARITY_SQUASHES {
            for &n in &BOUNDARY_COUNTS {
                assert_bit_identical::<MSE_TILE_LANES>(squash, n);
            }
        }
    }

    /// A network with an aggregate squash must dispatch to the unchanged
    /// scattered kernel — never the interleaved gather (whose standard-only
    /// `squash_x8` fallback would mis-evaluate an aggregate neuron). Asserting
    /// the public `mse_sum_batch_8way` dispatcher is bit-identical to
    /// `mse_sum_batch_scattered` proves the routing, and keeps aggregate
    /// networks bit-identical to their pre-#384 result.
    #[test]
    fn aggregate_dispatch_stays_on_scattered_path() {
        let input_size = 6;
        let values_per_record = input_size + 1;
        for squash in [
            SquashType::Minimum,
            SquashType::Maximum,
            SquashType::If,
            SquashType::Hypotenuse,
            SquashType::HypotenuseV2,
            SquashType::Mean,
        ] {
            let mut net = build_network(input_size, squash);
            assert!(
                net.has_aggregate_squash(),
                "{squash:?} must be detected as an aggregate squash"
            );
            // Multiples of 8 only: the dispatch guarantee (aggregate → scattered)
            // is proven by the full-group path, which handles every aggregate
            // type. The scattered path's `< 8` aggregate remainder handling is a
            // pre-existing concern untouched by #384, so it is out of scope here.
            for &n in &[8usize, 16, 24, 4096] {
                let records = build_records(n, input_size);
                let dispatched =
                    mse_sum_batch_8way(&mut net, &records, values_per_record, input_size, 1, n);
                let scattered =
                    mse_sum_batch_scattered(&net, &records, values_per_record, input_size, 1, n);
                assert_eq!(
                    dispatched.to_bits(),
                    scattered.to_bits(),
                    "{squash:?} n={n}: aggregate dispatch diverged from scattered path"
                );
            }
        }
    }

    /// Fail-loud guard (debug builds): a hot view that has drifted from
    /// `synapses` must panic rather than score silently-wrong numbers.
    ///
    /// Issue #625 moved this test in-crate. It used to live in
    /// `tests/hot_synapse_soa.rs` and drift the view by writing the then-public
    /// `synapses` field from outside the crate — the write that issue closed.
    /// Drift is now only expressible inside `neat-core`, which is exactly where
    /// `debug_assert_hot_soa` still has to catch it.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Issue #533")]
    fn a_drifted_hot_view_fails_loud_instead_of_scoring_wrong_numbers() {
        let input_size = 6;
        let mut net = build_network(input_size, SquashType::Tanh);

        // The redundancy hazard: a weight is changed and the hot view is not
        // rebuilt from it.
        net.synapses[0].weight = 12.5;

        // Same entry point the test drove before Issue #625 moved it in-crate, so
        // `mse_sum_batch_packed`'s own guard call stays pinned rather than only
        // the interleaved kernel's.
        let records = build_records(8, input_size);
        let _ = mse_sum_batch_packed(&mut net, &records, input_size, 1, true);
    }
}
