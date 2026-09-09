//! Native SIMD fast paths for weighted sums (Issue #1202 / #1209 multi-record;
//! Issue #153 single-record, on native hosts).
//!
//! `wasm32` uses `simd128` in `simd.rs`. Here **`x86_64`** uses **AVX2** (`__m256` + FMA) for
//! 8-wide and **FMA + SSE** for 4-wide; **`aarch64`** uses **NEON** (`float32x4` pairs for 8-wide).
//!
//! The **single-record** primitives (`weighted_sum_simd`, `weighted_sum_no_bias_simd`,
//! `weighted_sum_of_squares_simd`, `weighted_sum_of_squares_v2_simd`) run on the primary
//! `activate()` forward-pass hot path. They vectorise along the synapse dimension —
//! gathering 4 indexed activations per step, FMA-accumulating, then horizontally
//! reducing — with FMA+SSE on `x86_64`, NEON on `aarch64`, and scalar elsewhere and
//! for the 0..3 synapse tail.
//!
//! The ISA-neutral scalar layer — the reference kernels, the saturating count
//! guard, and the seed-taking tail helpers — lives once in
//! [`crate::simd::scalar`] (Issue #447) and is shared with the wasm kernels.

use crate::network::SynapseData;
use crate::simd::bounds;
use crate::simd::scalar;

/// Widest record-interleaved tile the generic gather kernels support
/// (Issue #530).
///
/// The vector kernels hold their accumulators in a fixed-size array sized for
/// this maximum and use only the leading `R / 8` (AVX2) or `R / 4` (NEON)
/// entries, so the unused accumulators are eliminated at compile time. Every
/// tile width `R` must be a multiple of 8 and no wider than this — both are
/// checked at compile time by the kernels' `assert_interleaved_tile` guard.
pub const MAX_INTERLEAVED_LANES: usize = 64;

/// Compile-time guard on a record-interleaved tile width (Issue #530): `R` must
/// be a non-zero multiple of 8 and no wider than [`MAX_INTERLEAVED_LANES`].
///
/// Fails the build rather than silently mis-sizing an accumulator array
/// (fail-loud, Issue #3234).
#[inline]
pub(crate) const fn assert_interleaved_tile<const R: usize>() {
    assert!(
        R > 0 && R % 8 == 0 && R <= MAX_INTERLEAVED_LANES,
        "record-interleaved tile width must be a non-zero multiple of 8 and at most MAX_INTERLEAVED_LANES"
    );
}

/// Whether the x86_64 AVX2 record kernels may run, given the CPU features
/// actually detected at runtime (Issue #605).
///
/// `weighted_sum_simd_8records_avx2` and `weighted_sum_interleaved_avx2` issue
/// `_mm256_fmadd_ps`, so they need **`fma`** as well as **`avx2`**: AVX2 does
/// not imply FMA, and an `avx2`-only guard would execute an FMA instruction on
/// a CPU (or a hypervisor masking the feature) that does not have one. Kept as
/// a pure function of the two detected flags so the gate is testable without a
/// CPU that has one feature and not the other.
#[inline]
pub const fn avx2_fma_kernels_enabled(avx2_detected: bool, fma_detected: bool) -> bool {
    avx2_detected && fma_detected
}

#[inline]
fn weighted_sum_simd_8records_scalar(
    synapses: &[SynapseData],
    act0: &[f32],
    act1: &[f32],
    act2: &[f32],
    act3: &[f32],
    act4: &[f32],
    act5: &[f32],
    act6: &[f32],
    act7: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> (f32, f32, f32, f32, f32, f32, f32, f32) {
    let mut sum0 = bias;
    let mut sum1 = bias;
    let mut sum2 = bias;
    let mut sum3 = bias;
    let mut sum4 = bias;
    let mut sum5 = bias;
    let mut sum6 = bias;
    let mut sum7 = bias;
    for synapse in synapses.iter().take(end).skip(start) {
        let from = synapse.from_index as usize;
        let w = synapse.weight;
        sum0 += act0[from] * w;
        sum1 += act1[from] * w;
        sum2 += act2[from] * w;
        sum3 += act3[from] * w;
        sum4 += act4[from] * w;
        sum5 += act5[from] * w;
        sum6 += act6[from] * w;
        sum7 += act7[from] * w;
    }
    (sum0, sum1, sum2, sum3, sum4, sum5, sum6, sum7)
}

#[inline]
fn weighted_sum_simd_4records_scalar(
    synapses: &[SynapseData],
    act0: &[f32],
    act1: &[f32],
    act2: &[f32],
    act3: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> (f32, f32, f32, f32) {
    let mut sum0 = bias;
    let mut sum1 = bias;
    let mut sum2 = bias;
    let mut sum3 = bias;
    for synapse in synapses.iter().take(end).skip(start) {
        let from = synapse.from_index as usize;
        let w = synapse.weight;
        sum0 += act0[from] * w;
        sum1 += act1[from] * w;
        sum2 += act2[from] * w;
        sum3 += act3[from] * w;
    }
    (sum0, sum1, sum2, sum3)
}

/// Scalar fallback for the record-interleaved `R`-lane weighted sum
/// (Issue #287; widened to a tunable tile in Issue #530).
///
/// `inter` is the transposed batch activation buffer: lane `l` of neuron `n`
/// lives at `inter[n * R + l]`, so all `R` records for a source neuron are
/// contiguous. This mirrors [`weighted_sum_simd_8records_scalar`] numerically —
/// same per-synapse FMA order, bias seeded into every lane, each lane summed
/// independently — so the result is bit-identical whichever gather layout **and
/// whichever tile width** is used.
#[inline]
fn weighted_sum_interleaved_scalar<const R: usize>(
    hot_weights: &[f32],
    hot_from: &[u16],
    inter: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> [f32; R] {
    let mut acc = [bias; R];
    for (w, from) in hot_weights[start..end].iter().zip(&hot_from[start..end]) {
        let base = *from as usize * R;
        let chunk = &inter[base..base + R];
        for (a, c) in acc.iter_mut().zip(chunk) {
            *a += *c * *w;
        }
    }
    acc
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use super::{SynapseData, scalar};
    use core::arch::x86_64::*;

    /// # Safety
    /// Caller must ensure **both** AVX2 and FMA are enabled
    /// (`is_x86_feature_detected!("avx2")` **and**
    /// `is_x86_feature_detected!("fma")`) — the kernel issues
    /// `_mm256_fmadd_ps`, and AVX2 does not imply FMA (Issue #605).
    /// Issue #207 - caller must also ensure every `synapse.from_index` in
    /// `start..end` is a valid index into each activation buffer; the kernel reads
    /// activations with `get_unchecked`. `CompiledNetwork::new` validates this at
    /// load time.
    #[target_feature(enable = "avx2", enable = "fma")]
    #[inline]
    pub unsafe fn weighted_sum_simd_8records_avx2(
        synapses: &[SynapseData],
        act0: &[f32],
        act1: &[f32],
        act2: &[f32],
        act3: &[f32],
        act4: &[f32],
        act5: &[f32],
        act6: &[f32],
        act7: &[f32],
        start: usize,
        end: usize,
        bias: f32,
    ) -> (f32, f32, f32, f32, f32, f32, f32, f32) {
        let mut acc = _mm256_set1_ps(bias);
        for i in start..end {
            let synapse = unsafe { synapses.get_unchecked(i) };
            let from = synapse.from_index as usize;
            let w = synapse.weight;
            let acts = unsafe {
                _mm256_set_ps(
                    *act7.get_unchecked(from),
                    *act6.get_unchecked(from),
                    *act5.get_unchecked(from),
                    *act4.get_unchecked(from),
                    *act3.get_unchecked(from),
                    *act2.get_unchecked(from),
                    *act1.get_unchecked(from),
                    *act0.get_unchecked(from),
                )
            };
            let ws = _mm256_set1_ps(w);
            // `_mm256_fmadd_ps` needs `fma`, enabled on this fn alongside `avx2`.
            acc = _mm256_fmadd_ps(ws, acts, acc);
        }
        let mut out = [0.0_f32; 8];
        unsafe { _mm256_storeu_ps(out.as_mut_ptr(), acc) };
        (
            out[0], out[1], out[2], out[3], out[4], out[5], out[6], out[7],
        )
    }

    /// Record-interleaved `R`-lane weighted sum (Issue #287; widened to a
    /// tunable tile in Issue #530).
    ///
    /// `inter` holds the transposed batch: the `R` records for source neuron
    /// `n` are contiguous at `inter[n * R .. n * R + R]`, so each synapse gather
    /// is `R / 8` adjacent `_mm256_loadu_ps` reads instead of `R` scattered
    /// scalar loads. Widening `R` amortises the synapse stream: the whole
    /// synapse array is re-read once per **tile**, not once per eight records.
    ///
    /// # Safety
    /// Caller must ensure **both** AVX2 and FMA are enabled
    /// (`is_x86_feature_detected!("avx2")` **and**
    /// `is_x86_feature_detected!("fma")`) — the kernel issues
    /// `_mm256_fmadd_ps`, and AVX2 does not imply FMA (Issue #605).
    /// Issue #287 - `inter.len()` must be `num_neurons * R` and every
    /// `hot_from` entry in `start..end` must be `< num_neurons`, so
    /// `from_index * R + R <= inter.len()`; `CompiledNetwork::new` validates the
    /// synapse index range at load time. The `R`-wide read is then in bounds.
    /// Issue #533 - `start..end` must also be in bounds for **both** hot arrays
    /// (`end <= hot_weights.len() == hot_from.len()`), which holds because both
    /// are built from `synapses` by `hot_synapse_soa` and carry its length.
    #[target_feature(enable = "avx2", enable = "fma")]
    #[inline]
    pub unsafe fn weighted_sum_interleaved_avx2<const R: usize>(
        hot_weights: &[f32],
        hot_from: &[u16],
        inter: &[f32],
        start: usize,
        end: usize,
        bias: f32,
    ) -> [f32; R] {
        const { super::assert_interleaved_tile::<R>() };
        let octs = R / 8;
        let mut acc = [_mm256_set1_ps(bias); super::MAX_INTERLEAVED_LANES / 8];
        let ptr = inter.as_ptr();
        for i in start..end {
            // SAFETY: `i < end <= hot_from.len() == hot_weights.len()`, checked
            // by the caller's `synapse_count` prologue against the same span
            // that indexes `synapses`.
            let base = unsafe { *hot_from.get_unchecked(i) } as usize * R;
            let ws = _mm256_set1_ps(unsafe { *hot_weights.get_unchecked(i) });
            for (o, a) in acc.iter_mut().take(octs).enumerate() {
                // SAFETY: base + R <= inter.len() by the load-time index
                // validation documented above, and `o < R / 8`, so this
                // unaligned 8-wide load is in bounds.
                let acts = unsafe { _mm256_loadu_ps(ptr.add(base + o * 8)) };
                // `_mm256_fmadd_ps` needs `fma`, enabled on this fn alongside `avx2`
                // (mirrors the 8records kernel above).
                *a = _mm256_fmadd_ps(ws, acts, *a);
            }
        }
        let mut out = [0.0_f32; R];
        for (o, a) in acc.iter().take(octs).enumerate() {
            // SAFETY: `o < R / 8`, so `out[o * 8 .. o * 8 + 8]` is in bounds.
            unsafe { _mm256_storeu_ps(out.as_mut_ptr().add(o * 8), *a) };
        }
        out
    }

    /// # Safety
    /// Caller must ensure FMA is enabled (`is_x86_feature_detected!("fma")`).
    /// Issue #207 - caller must also ensure every `synapse.from_index` in
    /// `start..end` is a valid index into `activations` (`< activations.len()`);
    /// the kernel reads activations with `get_unchecked`. `CompiledNetwork::new`
    /// validates this at load time.
    #[target_feature(enable = "fma")]
    #[inline]
    pub unsafe fn weighted_sum_simd_4records_fma(
        synapses: &[SynapseData],
        act0: &[f32],
        act1: &[f32],
        act2: &[f32],
        act3: &[f32],
        start: usize,
        end: usize,
        bias: f32,
    ) -> (f32, f32, f32, f32) {
        let mut acc = _mm_set1_ps(bias);
        for i in start..end {
            let synapse = unsafe { synapses.get_unchecked(i) };
            let from = synapse.from_index as usize;
            let w = synapse.weight;
            let acts = unsafe {
                _mm_set_ps(
                    *act3.get_unchecked(from),
                    *act2.get_unchecked(from),
                    *act1.get_unchecked(from),
                    *act0.get_unchecked(from),
                )
            };
            let ws = _mm_set1_ps(w);
            acc = _mm_fmadd_ps(ws, acts, acc);
        }
        let mut out = [0.0_f32; 4];
        unsafe { _mm_storeu_ps(out.as_mut_ptr(), acc) };
        (out[0], out[1], out[2], out[3])
    }

    // ---- Single-record primitives (Issue #153) ----------------------------------
    // Vectorise along the synapse dimension: gather 4 indexed activations, multiply
    // by 4 weights, FMA-accumulate, then horizontally reduce. The 0..3 tail is
    // handled scalar-side. `_mm_set_ps`/`_mm_mul_ps`/`_mm_add_ps`/`_mm_storeu_ps`
    // are SSE (baseline on x86_64); the FMA target feature enables `_mm_fmadd_ps`.

    /// # Safety
    /// Caller must ensure FMA is enabled (`is_x86_feature_detected!("fma")`).
    /// Issue #207 - caller must also ensure every `synapse.from_index` in
    /// `start..end` is a valid index into `activations` (`< activations.len()`);
    /// the kernel reads activations with `get_unchecked`. `CompiledNetwork::new`
    /// validates this at load time.
    #[target_feature(enable = "fma")]
    #[inline]
    pub unsafe fn weighted_sum_fma(
        synapses: &[SynapseData],
        activations: &[f32],
        start: usize,
        end: usize,
        bias: f32,
    ) -> f32 {
        let mut acc = _mm_setzero_ps();
        let chunk_end = start + (scalar::synapse_count(start, end) / 4) * 4;
        let mut i = start;
        while i < chunk_end {
            let s0 = unsafe { synapses.get_unchecked(i) };
            let s1 = unsafe { synapses.get_unchecked(i + 1) };
            let s2 = unsafe { synapses.get_unchecked(i + 2) };
            let s3 = unsafe { synapses.get_unchecked(i + 3) };
            let weights = _mm_set_ps(s3.weight, s2.weight, s1.weight, s0.weight);
            let acts = unsafe {
                _mm_set_ps(
                    *activations.get_unchecked(s3.from_index as usize),
                    *activations.get_unchecked(s2.from_index as usize),
                    *activations.get_unchecked(s1.from_index as usize),
                    *activations.get_unchecked(s0.from_index as usize),
                )
            };
            acc = _mm_fmadd_ps(weights, acts, acc);
            i += 4;
        }
        let mut out = [0.0_f32; 4];
        unsafe { _mm_storeu_ps(out.as_mut_ptr(), acc) };
        let sum = bias + out[0] + out[1] + out[2] + out[3];
        // SAFETY: this fn's own index precondition (documented above) is exactly
        // `tail_sum`'s contract — load-time validation in `CompiledNetwork::new`.
        unsafe { scalar::tail_sum(synapses, activations, i, end, sum) }
    }

    /// # Safety
    /// Caller must ensure FMA is enabled (`is_x86_feature_detected!("fma")`).
    /// Issue #207 - caller must also ensure every `synapse.from_index` in
    /// `start..end` is a valid index into `activations` (`< activations.len()`);
    /// the kernel reads activations with `get_unchecked`. `CompiledNetwork::new`
    /// validates this at load time.
    #[target_feature(enable = "fma")]
    #[inline]
    pub unsafe fn weighted_sum_of_squares_fma(
        synapses: &[SynapseData],
        activations: &[f32],
        start: usize,
        end: usize,
    ) -> f32 {
        let mut acc = _mm_setzero_ps();
        let chunk_end = start + (scalar::synapse_count(start, end) / 4) * 4;
        let mut i = start;
        while i < chunk_end {
            let s0 = unsafe { synapses.get_unchecked(i) };
            let s1 = unsafe { synapses.get_unchecked(i + 1) };
            let s2 = unsafe { synapses.get_unchecked(i + 2) };
            let s3 = unsafe { synapses.get_unchecked(i + 3) };
            let weights = _mm_set_ps(s3.weight, s2.weight, s1.weight, s0.weight);
            let acts = unsafe {
                _mm_set_ps(
                    *activations.get_unchecked(s3.from_index as usize),
                    *activations.get_unchecked(s2.from_index as usize),
                    *activations.get_unchecked(s1.from_index as usize),
                    *activations.get_unchecked(s0.from_index as usize),
                )
            };
            let products = _mm_mul_ps(weights, acts);
            acc = _mm_fmadd_ps(products, products, acc);
            i += 4;
        }
        let mut out = [0.0_f32; 4];
        unsafe { _mm_storeu_ps(out.as_mut_ptr(), acc) };
        let sum = out[0] + out[1] + out[2] + out[3];
        // SAFETY: this fn's own index precondition (documented above) is exactly
        // `tail_sum_of_squares`'s contract — load-time validation.
        unsafe { scalar::tail_sum_of_squares(synapses, activations, i, end, sum) }
    }

    /// # Safety
    /// Caller must ensure FMA is enabled (`is_x86_feature_detected!("fma")`).
    /// Issue #207 - caller must also ensure every `synapse.from_index` in
    /// `start..end` is a valid index into `activations` (`< activations.len()`);
    /// the kernel reads activations with `get_unchecked`. `CompiledNetwork::new`
    /// validates this at load time.
    #[target_feature(enable = "fma")]
    #[inline]
    pub unsafe fn weighted_sum_of_squares_v2_fma(
        synapses: &[SynapseData],
        activations: &[f32],
        start: usize,
        end: usize,
        bias: f32,
    ) -> f32 {
        let bias_vec = _mm_set1_ps(bias);
        let mut acc = _mm_setzero_ps();
        let chunk_end = start + (scalar::synapse_count(start, end) / 4) * 4;
        let mut i = start;
        while i < chunk_end {
            let s0 = unsafe { synapses.get_unchecked(i) };
            let s1 = unsafe { synapses.get_unchecked(i + 1) };
            let s2 = unsafe { synapses.get_unchecked(i + 2) };
            let s3 = unsafe { synapses.get_unchecked(i + 3) };
            let weights = _mm_set_ps(s3.weight, s2.weight, s1.weight, s0.weight);
            let acts = unsafe {
                _mm_set_ps(
                    *activations.get_unchecked(s3.from_index as usize),
                    *activations.get_unchecked(s2.from_index as usize),
                    *activations.get_unchecked(s1.from_index as usize),
                    *activations.get_unchecked(s0.from_index as usize),
                )
            };
            let vals = _mm_add_ps(bias_vec, _mm_mul_ps(weights, acts));
            acc = _mm_fmadd_ps(vals, vals, acc);
            i += 4;
        }
        let mut out = [0.0_f32; 4];
        unsafe { _mm_storeu_ps(out.as_mut_ptr(), acc) };
        let sum = out[0] + out[1] + out[2] + out[3];
        // SAFETY: this fn's own index precondition (documented above) is exactly
        // `tail_sum_of_squares_v2`'s contract — load-time validation.
        unsafe { scalar::tail_sum_of_squares_v2(synapses, activations, i, end, sum, bias) }
    }
}

#[cfg(target_arch = "aarch64")]
mod aarch64 {
    use super::{SynapseData, scalar};
    use core::arch::aarch64::*;

    /// # Safety
    /// Caller must ensure NEON is available (typical on aarch64-apple-darwin / linux-aarch64).
    /// Issue #207 - caller must also ensure every `synapse.from_index` in
    /// `start..end` is a valid index into each activation buffer; the kernel reads
    /// activations with `get_unchecked`. `CompiledNetwork::new` validates this at
    /// load time.
    #[target_feature(enable = "neon")]
    #[inline]
    pub unsafe fn weighted_sum_simd_8records_neon(
        synapses: &[SynapseData],
        act0: &[f32],
        act1: &[f32],
        act2: &[f32],
        act3: &[f32],
        act4: &[f32],
        act5: &[f32],
        act6: &[f32],
        act7: &[f32],
        start: usize,
        end: usize,
        bias: f32,
    ) -> (f32, f32, f32, f32, f32, f32, f32, f32) {
        let mut acc03 = vdupq_n_f32(bias);
        let mut acc47 = vdupq_n_f32(bias);
        let mut lane03 = [0.0_f32; 4];
        let mut lane47 = [0.0_f32; 4];
        for i in start..end {
            let synapse = unsafe { synapses.get_unchecked(i) };
            let from = synapse.from_index as usize;
            let w = synapse.weight;
            unsafe {
                lane03[0] = *act0.get_unchecked(from);
                lane03[1] = *act1.get_unchecked(from);
                lane03[2] = *act2.get_unchecked(from);
                lane03[3] = *act3.get_unchecked(from);
                lane47[0] = *act4.get_unchecked(from);
                lane47[1] = *act5.get_unchecked(from);
                lane47[2] = *act6.get_unchecked(from);
                lane47[3] = *act7.get_unchecked(from);
            }
            let acts03 = unsafe { vld1q_f32(lane03.as_ptr()) };
            let acts47 = unsafe { vld1q_f32(lane47.as_ptr()) };
            let vw = vdupq_n_f32(w);
            acc03 = vfmaq_f32(acc03, vw, acts03);
            acc47 = vfmaq_f32(acc47, vw, acts47);
        }
        let mut o03 = [0.0_f32; 4];
        let mut o47 = [0.0_f32; 4];
        unsafe { vst1q_f32(o03.as_mut_ptr(), acc03) };
        unsafe { vst1q_f32(o47.as_mut_ptr(), acc47) };
        (
            o03[0], o03[1], o03[2], o03[3], o47[0], o47[1], o47[2], o47[3],
        )
    }

    /// Record-interleaved `R`-lane weighted sum (Issue #287; widened to a
    /// tunable tile in Issue #530).
    ///
    /// `inter` holds the transposed batch: the `R` records for source neuron
    /// `n` are contiguous at `inter[n * R .. n * R + R]`, so each synapse gather
    /// is `R / 4` adjacent `vld1q_f32` loads instead of `R` scattered scalar
    /// loads staged through a stack array. Widening `R` amortises the synapse
    /// stream: the whole synapse array is re-read once per **tile**, not once
    /// per eight records.
    ///
    /// # Safety
    /// Caller must ensure NEON is available (typical on aarch64-apple-darwin /
    /// linux-aarch64). Issue #287 - `inter.len()` must be `num_neurons * R` and
    /// every `hot_from` entry in `start..end` must be `< num_neurons`, so
    /// `from_index * R + R <= inter.len()`; `CompiledNetwork::new` validates the
    /// synapse index range at load time, making the `R / 4` 4-wide reads in
    /// bounds.
    /// Issue #533 - `start..end` must also be in bounds for **both** hot arrays
    /// (`end <= hot_weights.len() == hot_from.len()`), which holds because both
    /// are built from `synapses` by `hot_synapse_soa` and carry its length.
    #[target_feature(enable = "neon")]
    #[inline]
    pub unsafe fn weighted_sum_interleaved_neon<const R: usize>(
        hot_weights: &[f32],
        hot_from: &[u16],
        inter: &[f32],
        start: usize,
        end: usize,
        bias: f32,
    ) -> [f32; R] {
        const { super::assert_interleaved_tile::<R>() };
        let quads = R / 4;
        let mut acc = [vdupq_n_f32(bias); super::MAX_INTERLEAVED_LANES / 4];
        let ptr = inter.as_ptr();
        for i in start..end {
            // SAFETY: `i < end <= hot_from.len() == hot_weights.len()`, checked
            // by the caller's `synapse_count` prologue against the same span
            // that indexes `synapses`.
            let base = unsafe { *hot_from.get_unchecked(i) } as usize * R;
            let vw = vdupq_n_f32(unsafe { *hot_weights.get_unchecked(i) });
            for (q, a) in acc.iter_mut().take(quads).enumerate() {
                // SAFETY: base + R <= inter.len() by the load-time index
                // validation documented above, and `q < R / 4`, so this 4-wide
                // read is in bounds.
                let acts = unsafe { vld1q_f32(ptr.add(base + q * 4)) };
                *a = vfmaq_f32(*a, vw, acts);
            }
        }
        let mut out = [0.0_f32; R];
        for (q, a) in acc.iter().take(quads).enumerate() {
            // SAFETY: `q < R / 4`, so `out[q * 4 .. q * 4 + 4]` is in bounds.
            unsafe { vst1q_f32(out.as_mut_ptr().add(q * 4), *a) };
        }
        out
    }

    /// # Safety
    /// Caller must ensure NEON is available.
    /// Issue #207 - caller must also ensure every `synapse.from_index` in
    /// `start..end` is a valid index into `activations` (`< activations.len()`);
    /// the kernel reads activations with `get_unchecked`. `CompiledNetwork::new`
    /// validates this at load time.
    #[target_feature(enable = "neon")]
    #[inline]
    pub unsafe fn weighted_sum_simd_4records_neon(
        synapses: &[SynapseData],
        act0: &[f32],
        act1: &[f32],
        act2: &[f32],
        act3: &[f32],
        start: usize,
        end: usize,
        bias: f32,
    ) -> (f32, f32, f32, f32) {
        let mut acc = vdupq_n_f32(bias);
        let mut lane = [0.0_f32; 4];
        for i in start..end {
            let synapse = unsafe { synapses.get_unchecked(i) };
            let from = synapse.from_index as usize;
            let w = synapse.weight;
            unsafe {
                lane[0] = *act0.get_unchecked(from);
                lane[1] = *act1.get_unchecked(from);
                lane[2] = *act2.get_unchecked(from);
                lane[3] = *act3.get_unchecked(from);
            }
            let acts = unsafe { vld1q_f32(lane.as_ptr()) };
            let vw = vdupq_n_f32(w);
            acc = vfmaq_f32(acc, vw, acts);
        }
        let mut out = [0.0_f32; 4];
        unsafe { vst1q_f32(out.as_mut_ptr(), acc) };
        (out[0], out[1], out[2], out[3])
    }

    // ---- Single-record primitives (Issue #153) ----------------------------------
    // Vectorise along the synapse dimension: gather 4 indexed activations into a
    // lane buffer, multiply by 4 weights, FMA-accumulate, then horizontally reduce
    // (`vaddvq_f32`). The 0..3 tail is handled scalar-side.

    /// # Safety
    /// Caller must ensure NEON is available.
    /// Issue #207 - caller must also ensure every `synapse.from_index` in
    /// `start..end` is a valid index into `activations` (`< activations.len()`);
    /// the kernel reads activations with `get_unchecked`. `CompiledNetwork::new`
    /// validates this at load time.
    #[target_feature(enable = "neon")]
    #[inline]
    pub unsafe fn weighted_sum_neon(
        synapses: &[SynapseData],
        activations: &[f32],
        start: usize,
        end: usize,
        bias: f32,
    ) -> f32 {
        let mut acc = vdupq_n_f32(0.0);
        let mut wl = [0.0_f32; 4];
        let mut al = [0.0_f32; 4];
        let chunk_end = start + (scalar::synapse_count(start, end) / 4) * 4;
        let mut i = start;
        while i < chunk_end {
            let s0 = unsafe { synapses.get_unchecked(i) };
            let s1 = unsafe { synapses.get_unchecked(i + 1) };
            let s2 = unsafe { synapses.get_unchecked(i + 2) };
            let s3 = unsafe { synapses.get_unchecked(i + 3) };
            wl[0] = s0.weight;
            wl[1] = s1.weight;
            wl[2] = s2.weight;
            wl[3] = s3.weight;
            unsafe {
                al[0] = *activations.get_unchecked(s0.from_index as usize);
                al[1] = *activations.get_unchecked(s1.from_index as usize);
                al[2] = *activations.get_unchecked(s2.from_index as usize);
                al[3] = *activations.get_unchecked(s3.from_index as usize);
            }
            let weights = unsafe { vld1q_f32(wl.as_ptr()) };
            let acts = unsafe { vld1q_f32(al.as_ptr()) };
            acc = vfmaq_f32(acc, weights, acts);
            i += 4;
        }
        let sum = bias + vaddvq_f32(acc);
        // SAFETY: this fn's own index precondition (documented above) is exactly
        // `tail_sum`'s contract — load-time validation in `CompiledNetwork::new`.
        unsafe { scalar::tail_sum(synapses, activations, i, end, sum) }
    }

    /// # Safety
    /// Caller must ensure NEON is available.
    /// Issue #207 - caller must also ensure every `synapse.from_index` in
    /// `start..end` is a valid index into `activations` (`< activations.len()`);
    /// the kernel reads activations with `get_unchecked`. `CompiledNetwork::new`
    /// validates this at load time.
    #[target_feature(enable = "neon")]
    #[inline]
    pub unsafe fn weighted_sum_of_squares_neon(
        synapses: &[SynapseData],
        activations: &[f32],
        start: usize,
        end: usize,
    ) -> f32 {
        let mut acc = vdupq_n_f32(0.0);
        let mut wl = [0.0_f32; 4];
        let mut al = [0.0_f32; 4];
        let chunk_end = start + (scalar::synapse_count(start, end) / 4) * 4;
        let mut i = start;
        while i < chunk_end {
            let s0 = unsafe { synapses.get_unchecked(i) };
            let s1 = unsafe { synapses.get_unchecked(i + 1) };
            let s2 = unsafe { synapses.get_unchecked(i + 2) };
            let s3 = unsafe { synapses.get_unchecked(i + 3) };
            wl[0] = s0.weight;
            wl[1] = s1.weight;
            wl[2] = s2.weight;
            wl[3] = s3.weight;
            unsafe {
                al[0] = *activations.get_unchecked(s0.from_index as usize);
                al[1] = *activations.get_unchecked(s1.from_index as usize);
                al[2] = *activations.get_unchecked(s2.from_index as usize);
                al[3] = *activations.get_unchecked(s3.from_index as usize);
            }
            let weights = unsafe { vld1q_f32(wl.as_ptr()) };
            let acts = unsafe { vld1q_f32(al.as_ptr()) };
            let products = vmulq_f32(weights, acts);
            acc = vfmaq_f32(acc, products, products);
            i += 4;
        }
        let sum = vaddvq_f32(acc);
        // SAFETY: this fn's own index precondition (documented above) is exactly
        // `tail_sum_of_squares`'s contract — load-time validation.
        unsafe { scalar::tail_sum_of_squares(synapses, activations, i, end, sum) }
    }

    /// # Safety
    /// Caller must ensure NEON is available.
    /// Issue #207 - caller must also ensure every `synapse.from_index` in
    /// `start..end` is a valid index into `activations` (`< activations.len()`);
    /// the kernel reads activations with `get_unchecked`. `CompiledNetwork::new`
    /// validates this at load time.
    #[target_feature(enable = "neon")]
    #[inline]
    pub unsafe fn weighted_sum_of_squares_v2_neon(
        synapses: &[SynapseData],
        activations: &[f32],
        start: usize,
        end: usize,
        bias: f32,
    ) -> f32 {
        let bias_vec = vdupq_n_f32(bias);
        let mut acc = vdupq_n_f32(0.0);
        let mut wl = [0.0_f32; 4];
        let mut al = [0.0_f32; 4];
        let chunk_end = start + (scalar::synapse_count(start, end) / 4) * 4;
        let mut i = start;
        while i < chunk_end {
            let s0 = unsafe { synapses.get_unchecked(i) };
            let s1 = unsafe { synapses.get_unchecked(i + 1) };
            let s2 = unsafe { synapses.get_unchecked(i + 2) };
            let s3 = unsafe { synapses.get_unchecked(i + 3) };
            wl[0] = s0.weight;
            wl[1] = s1.weight;
            wl[2] = s2.weight;
            wl[3] = s3.weight;
            unsafe {
                al[0] = *activations.get_unchecked(s0.from_index as usize);
                al[1] = *activations.get_unchecked(s1.from_index as usize);
                al[2] = *activations.get_unchecked(s2.from_index as usize);
                al[3] = *activations.get_unchecked(s3.from_index as usize);
            }
            let weights = unsafe { vld1q_f32(wl.as_ptr()) };
            let acts = unsafe { vld1q_f32(al.as_ptr()) };
            let vals = vaddq_f32(bias_vec, vmulq_f32(weights, acts));
            acc = vfmaq_f32(acc, vals, vals);
            i += 4;
        }
        let sum = vaddvq_f32(acc);
        // SAFETY: this fn's own index precondition (documented above) is exactly
        // `tail_sum_of_squares_v2`'s contract — load-time validation.
        unsafe { scalar::tail_sum_of_squares_v2(synapses, activations, i, end, sum, bias) }
    }
}

/// 8-record weighted sum: AVX2+FMA on x86_64, NEON on aarch64, else scalar.
///
/// Bounds-validating entry point (Issue #613): a span whose `from_index` values
/// do not all index every activation buffer is **refused with a panic** rather
/// than read unchecked, so safe caller code can never reach an out-of-bounds
/// read. Validated callers — anything holding a `CompiledNetwork` — should call
/// [`weighted_sum_simd_8records_unchecked`] and skip the `O(end - start)`
/// predicate.
///
/// # Panics
/// If `end > synapses.len()`, or any `from_index` in `start..end` does not index
/// every one of the eight activation buffers.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn weighted_sum_simd_8records(
    synapses: &[SynapseData],
    act0: &[f32],
    act1: &[f32],
    act2: &[f32],
    act3: &[f32],
    act4: &[f32],
    act5: &[f32],
    act6: &[f32],
    act7: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> (f32, f32, f32, f32, f32, f32, f32, f32) {
    let lens = [
        act0.len(),
        act1.len(),
        act2.len(),
        act3.len(),
        act4.len(),
        act5.len(),
        act6.len(),
        act7.len(),
    ];
    if !bounds::span_in_bounds_multi(synapses, start, end, &lens) {
        bounds::reject_span("weighted_sum_simd_8records");
    }
    // SAFETY: `span_in_bounds_multi` has just established the kernel's index
    // precondition for this span against the shortest activation buffer.
    unsafe {
        weighted_sum_simd_8records_unchecked(
            synapses, act0, act1, act2, act3, act4, act5, act6, act7, start, end, bias,
        )
    }
}

/// 8-record weighted sum without the [`bounds`] pre-pass — the forward-pass
/// hot-path form.
///
/// # Safety
/// Every `synapse.from_index` in `start..end` must be a valid index into each
/// of the eight activation buffers, and `end <= synapses.len()`; the kernel
/// reads both with `get_unchecked`. `CompiledNetwork::new` enforces exactly
/// that at load time (`NetworkError::InvalidSynapseIndex`), so a caller
/// holding a loaded network already satisfies it. Safe callers must go through
/// [`weighted_sum_simd_8records`].
#[inline]
#[allow(clippy::too_many_arguments)]
pub unsafe fn weighted_sum_simd_8records_unchecked(
    synapses: &[SynapseData],
    act0: &[f32],
    act1: &[f32],
    act2: &[f32],
    act3: &[f32],
    act4: &[f32],
    act5: &[f32],
    act6: &[f32],
    act7: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> (f32, f32, f32, f32, f32, f32, f32, f32) {
    if scalar::synapse_count(start, end) == 0 {
        return (bias, bias, bias, bias, bias, bias, bias, bias);
    }

    #[cfg(target_arch = "x86_64")]
    {
        if avx2_fma_kernels_enabled(
            std::arch::is_x86_feature_detected!("avx2"),
            std::arch::is_x86_feature_detected!("fma"),
        ) {
            // SAFETY: the `is_x86_feature_detected!("avx2")` and
            // `is_x86_feature_detected!("fma")` guards above prove both features
            // are available, satisfying the
            // `#[target_feature(enable = "avx2", enable = "fma")]` precondition
            // documented on `weighted_sum_simd_8records_avx2`.
            return unsafe {
                x86::weighted_sum_simd_8records_avx2(
                    synapses, act0, act1, act2, act3, act4, act5, act6, act7, start, end, bias,
                )
            };
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("neon") {
            // SAFETY: the `is_aarch64_feature_detected!("neon")` guard above proves
            // NEON is available, satisfying the `#[target_feature(enable = "neon")]`
            // precondition documented on `weighted_sum_simd_8records_neon`.
            return unsafe {
                aarch64::weighted_sum_simd_8records_neon(
                    synapses, act0, act1, act2, act3, act4, act5, act6, act7, start, end, bias,
                )
            };
        }
    }

    weighted_sum_simd_8records_scalar(
        synapses, act0, act1, act2, act3, act4, act5, act6, act7, start, end, bias,
    )
}

/// Record-interleaved `R`-lane weighted sum (Issue #287; widened to a tunable
/// tile in Issue #530): AVX2+FMA on x86_64, NEON on aarch64, else scalar.
/// `inter` is the transposed batch buffer (`inter[n * R + l]` = lane `l` of
/// neuron `n`), so each synapse gather reads `R` contiguous floats instead of
/// `R` scattered per-lane buffers.
///
/// `R` must be a non-zero multiple of 8 and at most
/// [`MAX_INTERLEAVED_LANES`] — enforced at compile time.
///
/// Every lane accumulates `bias + Σ w·a` in synapse order, independently of the
/// other lanes, so widening `R` leaves each record's sum **bit-identical**.
///
/// Bounds-validating entry point (Issue #613): a tile that does not address
/// `inter` throughout is **refused with a panic** rather than read unchecked, so
/// safe caller code can never reach an out-of-bounds read. Validated callers
/// should use [`weighted_sum_interleaved_unchecked`].
///
/// # Panics
/// If `end` exceeds either hot array, or any `hot_from` entry in `start..end`
/// does not satisfy `from * R + R <= inter.len()`.
#[inline]
pub fn weighted_sum_interleaved<const R: usize>(
    hot_weights: &[f32],
    hot_from: &[u16],
    inter: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> [f32; R] {
    const { assert_interleaved_tile::<R>() };
    if !bounds::interleaved_span_in_bounds(hot_weights, hot_from, inter.len(), R, start, end) {
        bounds::reject_interleaved_span("weighted_sum_interleaved");
    }
    // SAFETY: `interleaved_span_in_bounds` has just established that every
    // `hot_from` entry in `start..end` addresses a whole `R`-wide tile inside
    // `inter`, and that the span indexes both hot arrays.
    unsafe {
        weighted_sum_interleaved_unchecked::<R>(hot_weights, hot_from, inter, start, end, bias)
    }
}

/// [`weighted_sum_interleaved`] without the [`bounds`] pre-pass — the batched
/// scoring hot-path form.
///
/// # Safety
/// `end` must be `<= hot_weights.len()` and `<= hot_from.len()`, and every
/// `hot_from` entry in `start..end` must satisfy `from * R + R <= inter.len()`
/// — i.e. `inter` is the `num_neurons * R` transposed buffer and every source
/// neuron is in range. `CompiledNetwork::new` enforces the index range at load
/// time. Safe callers must go through [`weighted_sum_interleaved`].
#[inline]
pub unsafe fn weighted_sum_interleaved_unchecked<const R: usize>(
    hot_weights: &[f32],
    hot_from: &[u16],
    inter: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> [f32; R] {
    const { assert_interleaved_tile::<R>() };
    debug_assert_eq!(
        hot_weights.len(),
        hot_from.len(),
        "Issue #533 - the hot synapse arrays must be the same length"
    );

    if scalar::synapse_count(start, end) == 0 {
        return [bias; R];
    }

    #[cfg(target_arch = "x86_64")]
    {
        if avx2_fma_kernels_enabled(
            std::arch::is_x86_feature_detected!("avx2"),
            std::arch::is_x86_feature_detected!("fma"),
        ) {
            // SAFETY: the `is_x86_feature_detected!("avx2")` and
            // `is_x86_feature_detected!("fma")` guards above prove both features
            // are available, satisfying the
            // `#[target_feature(enable = "avx2", enable = "fma")]` precondition
            // on `weighted_sum_interleaved_avx2`.
            return unsafe {
                x86::weighted_sum_interleaved_avx2::<R>(
                    hot_weights,
                    hot_from,
                    inter,
                    start,
                    end,
                    bias,
                )
            };
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("neon") {
            // SAFETY: the `is_aarch64_feature_detected!("neon")` guard proves NEON
            // is available, satisfying the `#[target_feature(enable = "neon")]`
            // precondition on `weighted_sum_interleaved_neon`.
            return unsafe {
                aarch64::weighted_sum_interleaved_neon::<R>(
                    hot_weights,
                    hot_from,
                    inter,
                    start,
                    end,
                    bias,
                )
            };
        }
    }

    weighted_sum_interleaved_scalar::<R>(hot_weights, hot_from, inter, start, end, bias)
}

/// The 8-lane tile of [`weighted_sum_interleaved`], kept as the name the
/// batched **scoring** path (`BatchScratch::inter`) and its tests use.
///
/// # Panics
/// On the same conditions as [`weighted_sum_interleaved`].
#[inline]
pub fn weighted_sum_interleaved_8(
    hot_weights: &[f32],
    hot_from: &[u16],
    inter: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> [f32; 8] {
    weighted_sum_interleaved::<8>(hot_weights, hot_from, inter, start, end, bias)
}

/// The 8-lane tile of [`weighted_sum_interleaved_unchecked`].
///
/// # Safety
/// Same contract as [`weighted_sum_interleaved_unchecked`] with `R == 8`.
#[inline]
pub unsafe fn weighted_sum_interleaved_8_unchecked(
    hot_weights: &[f32],
    hot_from: &[u16],
    inter: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> [f32; 8] {
    // SAFETY: forwarded verbatim to the generic kernel; the caller's obligation
    // is the same contract at `R == 8`.
    unsafe {
        weighted_sum_interleaved_unchecked::<8>(hot_weights, hot_from, inter, start, end, bias)
    }
}

/// 4-record weighted sum: FMA+SSE on x86_64, NEON on aarch64, else scalar.
///
/// Bounds-validating entry point (Issue #613) — see
/// [`weighted_sum_simd_8records`]: an out-of-bounds span is refused with a
/// panic, never read unchecked. Validated callers should use
/// [`weighted_sum_simd_4records_unchecked`].
///
/// # Panics
/// On the same conditions as [`weighted_sum_simd_8records`], over the four
/// activation buffers.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn weighted_sum_simd_4records(
    synapses: &[SynapseData],
    act0: &[f32],
    act1: &[f32],
    act2: &[f32],
    act3: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> (f32, f32, f32, f32) {
    let lens = [act0.len(), act1.len(), act2.len(), act3.len()];
    if !bounds::span_in_bounds_multi(synapses, start, end, &lens) {
        bounds::reject_span("weighted_sum_simd_4records");
    }
    // SAFETY: `span_in_bounds_multi` has just established the kernel's index
    // precondition for this span against the shortest activation buffer.
    unsafe {
        weighted_sum_simd_4records_unchecked(synapses, act0, act1, act2, act3, start, end, bias)
    }
}

/// 4-record weighted sum without the [`bounds`] pre-pass — the hot-path form.
///
/// # Safety
/// Same contract as [`weighted_sum_simd_8records_unchecked`], over the four
/// activation buffers.
#[inline]
#[allow(clippy::too_many_arguments)]
pub unsafe fn weighted_sum_simd_4records_unchecked(
    synapses: &[SynapseData],
    act0: &[f32],
    act1: &[f32],
    act2: &[f32],
    act3: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> (f32, f32, f32, f32) {
    if scalar::synapse_count(start, end) == 0 {
        return (bias, bias, bias, bias);
    }

    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("fma") {
            // SAFETY: the `is_x86_feature_detected!("fma")` guard above proves
            // FMA is available, satisfying the `#[target_feature(enable = "fma")]`
            // precondition documented on `weighted_sum_simd_4records_fma`.
            return unsafe {
                x86::weighted_sum_simd_4records_fma(
                    synapses, act0, act1, act2, act3, start, end, bias,
                )
            };
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("neon") {
            // SAFETY: the `is_aarch64_feature_detected!("neon")` guard above proves
            // NEON is available, satisfying the `#[target_feature(enable = "neon")]`
            // precondition documented on `weighted_sum_simd_4records_neon`.
            return unsafe {
                aarch64::weighted_sum_simd_4records_neon(
                    synapses, act0, act1, act2, act3, start, end, bias,
                )
            };
        }
    }

    weighted_sum_simd_4records_scalar(synapses, act0, act1, act2, act3, start, end, bias)
}

// ============================================================================
// Single-record primitives (Issue #153)
//
// These run on the primary single-record `activate()` forward-pass hot path
// (`CompiledNetwork::activate`, `activate_into`, `activate_and_trace`), called
// once per neuron. They dispatch to AVX2/FMA on x86_64, NEON on aarch64, and
// fall back to the scalar loop elsewhere and for the 0..3 synapse tail.
// ============================================================================

// The scalar fallbacks and the `SINGLE_RECORD_SIMD_MIN` threshold live in
// `crate::simd::scalar` (Issue #447), shared with the wasm kernels.

/// Single-record weighted sum: `bias + sum(activation[from] * weight)`.
///
/// Production forward-pass hot path. AVX2/FMA on x86_64, NEON on aarch64, scalar
/// elsewhere and for counts below one SIMD lane.
///
/// Bounds-validating entry point (Issue #613): a span that does not index
/// `activations` throughout is **refused with a panic** rather than read
/// unchecked, so safe caller code can never reach an out-of-bounds read.
/// Validated callers — anything holding a `CompiledNetwork` — should call
/// [`weighted_sum_simd_unchecked`] and skip the `O(end - start)` predicate.
///
/// # Panics
/// If `end > synapses.len()`, or any `from_index` in `start..end` is not a valid
/// index into `activations`. Both are caller bugs the load-time
/// `NetworkError::InvalidSynapseIndex` validation rules out for a loaded
/// network, so failing loud beats answering from a truncated span.
#[inline]
pub fn weighted_sum_simd(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> f32 {
    if !bounds::span_in_bounds(synapses, start, end, activations.len()) {
        bounds::reject_span("weighted_sum_simd");
    }
    // SAFETY: `span_in_bounds` has just established the kernel's index
    // precondition for this span.
    unsafe { weighted_sum_simd_unchecked(synapses, activations, start, end, bias) }
}

/// [`weighted_sum_simd`] without the [`bounds`] pre-pass — the forward-pass
/// hot-path form.
///
/// Issue #207 - on the SIMD paths this delegates to `get_unchecked` kernels.
///
/// # Safety
/// `end` must be `<= synapses.len()` and every `synapse.from_index` in
/// `start..end` must be a valid index into `activations`.
/// `CompiledNetwork::new` enforces this at load time
/// (`NetworkError::InvalidSynapseIndex`), so callers holding a loaded network
/// already satisfy the precondition. Safe callers must go through
/// [`weighted_sum_simd`].
#[inline]
pub unsafe fn weighted_sum_simd_unchecked(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> f32 {
    if scalar::synapse_count(start, end) >= scalar::SINGLE_RECORD_SIMD_MIN {
        #[cfg(target_arch = "x86_64")]
        {
            if std::arch::is_x86_feature_detected!("fma") {
                // SAFETY: the `is_x86_feature_detected!("fma")` guard proves FMA is
                // available, satisfying the `#[target_feature(enable = "fma")]`
                // precondition documented on `weighted_sum_fma`.
                return unsafe { x86::weighted_sum_fma(synapses, activations, start, end, bias) };
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            if std::arch::is_aarch64_feature_detected!("neon") {
                // SAFETY: the `is_aarch64_feature_detected!("neon")` guard proves NEON
                // is available, satisfying the `#[target_feature(enable = "neon")]`
                // precondition documented on `weighted_sum_neon`.
                return unsafe {
                    aarch64::weighted_sum_neon(synapses, activations, start, end, bias)
                };
            }
        }
    }
    scalar::weighted_sum(synapses, activations, start, end, bias)
}

/// Single-record sum of squared weighted activations (Hypotenuse): `sum((a*w)^2)`.
///
/// Bounds-validating entry point (Issue #613) — see [`weighted_sum_simd`]: an
/// out-of-bounds span is refused with a panic, never read unchecked. Validated
/// callers should use [`weighted_sum_of_squares_simd_unchecked`].
///
/// # Panics
/// On the same conditions as [`weighted_sum_simd`].
#[inline]
pub fn weighted_sum_of_squares_simd(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
) -> f32 {
    if !bounds::span_in_bounds(synapses, start, end, activations.len()) {
        bounds::reject_span("weighted_sum_of_squares_simd");
    }
    // SAFETY: `span_in_bounds` has just established the kernel's index
    // precondition for this span.
    unsafe { weighted_sum_of_squares_simd_unchecked(synapses, activations, start, end) }
}

/// [`weighted_sum_of_squares_simd`] without the [`bounds`] pre-pass — the
/// forward-pass hot-path form. AVX2/FMA on x86_64, NEON on aarch64, scalar
/// elsewhere and for counts below one SIMD lane.
///
/// # Safety
/// Same contract as [`weighted_sum_simd_unchecked`].
#[inline]
pub unsafe fn weighted_sum_of_squares_simd_unchecked(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
) -> f32 {
    if scalar::synapse_count(start, end) >= scalar::SINGLE_RECORD_SIMD_MIN {
        #[cfg(target_arch = "x86_64")]
        {
            if std::arch::is_x86_feature_detected!("fma") {
                // SAFETY: the `is_x86_feature_detected!("fma")` guard above proves
                // the `#[target_feature(enable = "fma")]` precondition on
                // `weighted_sum_of_squares_fma` holds.
                return unsafe {
                    x86::weighted_sum_of_squares_fma(synapses, activations, start, end)
                };
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            if std::arch::is_aarch64_feature_detected!("neon") {
                // SAFETY: the `is_aarch64_feature_detected!("neon")` guard above
                // proves the `#[target_feature(enable = "neon")]` precondition on
                // `weighted_sum_of_squares_neon` holds.
                return unsafe {
                    aarch64::weighted_sum_of_squares_neon(synapses, activations, start, end)
                };
            }
        }
    }
    scalar::weighted_sum_of_squares(synapses, activations, start, end)
}

/// Single-record weighted sum without bias (Mean): `sum(activation[from] * weight)`.
///
/// Bounds-validating entry point (Issue #613) — see [`weighted_sum_simd`]: an
/// out-of-bounds span is refused with a panic, never read unchecked. Validated
/// callers should use [`weighted_sum_no_bias_simd_unchecked`].
///
/// # Panics
/// On the same conditions as [`weighted_sum_simd`].
#[inline]
pub fn weighted_sum_no_bias_simd(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
) -> f32 {
    if !bounds::span_in_bounds(synapses, start, end, activations.len()) {
        bounds::reject_span("weighted_sum_no_bias_simd");
    }
    // SAFETY: `span_in_bounds` has just established the kernel's index
    // precondition for this span.
    unsafe { weighted_sum_no_bias_simd_unchecked(synapses, activations, start, end) }
}

/// [`weighted_sum_no_bias_simd`] without the [`bounds`] pre-pass — the
/// forward-pass hot-path form. Reuses the bias-carrying kernel with `bias = 0`
/// so the SIMD path is shared. AVX2/FMA on x86_64, NEON on aarch64, scalar elsewhere.
///
/// # Safety
/// Same contract as [`weighted_sum_simd_unchecked`].
#[inline]
pub unsafe fn weighted_sum_no_bias_simd_unchecked(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
) -> f32 {
    if scalar::synapse_count(start, end) >= scalar::SINGLE_RECORD_SIMD_MIN {
        #[cfg(target_arch = "x86_64")]
        {
            if std::arch::is_x86_feature_detected!("fma") {
                // SAFETY: the `is_x86_feature_detected!("fma")` guard above proves
                // the `#[target_feature(enable = "fma")]` precondition on
                // `weighted_sum_fma` holds.
                return unsafe { x86::weighted_sum_fma(synapses, activations, start, end, 0.0) };
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            if std::arch::is_aarch64_feature_detected!("neon") {
                // SAFETY: the `is_aarch64_feature_detected!("neon")` guard above
                // proves the `#[target_feature(enable = "neon")]` precondition on
                // `weighted_sum_neon` holds.
                return unsafe {
                    aarch64::weighted_sum_neon(synapses, activations, start, end, 0.0)
                };
            }
        }
    }
    scalar::weighted_sum_no_bias(synapses, activations, start, end)
}

/// Single-record sum of squared (bias + weighted activation) (HypotenuseV2):
/// `sum((bias + a*w)^2)`.
///
/// Bounds-validating entry point (Issue #613) — see [`weighted_sum_simd`]: an
/// out-of-bounds span is refused with a panic, never read unchecked. Validated
/// callers should use [`weighted_sum_of_squares_v2_simd_unchecked`].
///
/// # Panics
/// On the same conditions as [`weighted_sum_simd`].
#[inline]
pub fn weighted_sum_of_squares_v2_simd(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> f32 {
    if !bounds::span_in_bounds(synapses, start, end, activations.len()) {
        bounds::reject_span("weighted_sum_of_squares_v2_simd");
    }
    // SAFETY: `span_in_bounds` has just established the kernel's index
    // precondition for this span.
    unsafe { weighted_sum_of_squares_v2_simd_unchecked(synapses, activations, start, end, bias) }
}

/// [`weighted_sum_of_squares_v2_simd`] without the [`bounds`] pre-pass — the
/// forward-pass hot-path form. AVX2/FMA on x86_64, NEON on aarch64, scalar
/// elsewhere and for counts below one SIMD lane.
///
/// # Safety
/// Same contract as [`weighted_sum_simd_unchecked`].
#[inline]
pub unsafe fn weighted_sum_of_squares_v2_simd_unchecked(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> f32 {
    if scalar::synapse_count(start, end) >= scalar::SINGLE_RECORD_SIMD_MIN {
        #[cfg(target_arch = "x86_64")]
        {
            if std::arch::is_x86_feature_detected!("fma") {
                // SAFETY: the `is_x86_feature_detected!("fma")` guard above proves
                // the `#[target_feature(enable = "fma")]` precondition on
                // `weighted_sum_of_squares_v2_fma` holds.
                return unsafe {
                    x86::weighted_sum_of_squares_v2_fma(synapses, activations, start, end, bias)
                };
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            if std::arch::is_aarch64_feature_detected!("neon") {
                // SAFETY: the `is_aarch64_feature_detected!("neon")` guard above
                // proves the `#[target_feature(enable = "neon")]` precondition on
                // `weighted_sum_of_squares_v2_neon` holds.
                return unsafe {
                    aarch64::weighted_sum_of_squares_v2_neon(
                        synapses,
                        activations,
                        start,
                        end,
                        bias,
                    )
                };
            }
        }
    }
    scalar::weighted_sum_of_squares_v2(synapses, activations, start, end, bias)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::SynapseData;

    fn synapse(from: u16, weight: f32) -> SynapseData {
        SynapseData {
            weight,
            from_index: from,
            synapse_type: 0,
        }
    }

    #[test]
    fn native_8_matches_scalar_small() {
        let syn = [synapse(0, 1.0), synapse(1, 2.0)];
        let act0 = vec![1.0_f32, 2.0];
        let act1 = vec![0.5_f32, 1.0];
        let act2 = vec![2.0_f32, 0.0];
        let act3 = vec![1.0_f32, 1.0];
        let act4 = vec![0.0_f32, 3.0];
        let act5 = vec![1.0_f32, 0.5];
        let act6 = vec![1.5_f32, 1.5];
        let act7 = vec![-1.0_f32, 2.0];
        let s = weighted_sum_simd_8records_scalar(
            &syn, &act0, &act1, &act2, &act3, &act4, &act5, &act6, &act7, 0, 2, 0.25,
        );
        let n = weighted_sum_simd_8records(
            &syn, &act0, &act1, &act2, &act3, &act4, &act5, &act6, &act7, 0, 2, 0.25,
        );
        assert_eq!(n.0, s.0);
        assert_eq!(n.1, s.1);
        assert_eq!(n.2, s.2);
        assert_eq!(n.3, s.3);
        assert_eq!(n.4, s.4);
        assert_eq!(n.5, s.5);
        assert_eq!(n.6, s.6);
        assert_eq!(n.7, s.7);
    }

    #[test]
    fn native_4_matches_scalar_small() {
        let syn = [synapse(0, 0.5), synapse(2, 1.5)];
        let a0 = vec![1.0_f32, 0.0, 3.0];
        let a1 = vec![2.0_f32, 1.0, 0.0];
        let a2 = vec![0.0_f32, 1.0, 4.0];
        let a3 = vec![1.0_f32, 2.0, 0.5];
        let s = weighted_sum_simd_4records_scalar(&syn, &a0, &a1, &a2, &a3, 0, 2, -1.0);
        let n = weighted_sum_simd_4records(&syn, &a0, &a1, &a2, &a3, 0, 2, -1.0);
        assert_eq!(n.0, s.0);
        assert_eq!(n.1, s.1);
        assert_eq!(n.2, s.2);
        assert_eq!(n.3, s.3);
    }

    // FMA/NEON reorder rounding can differ slightly from the scalar reference, so
    // single-record results are compared within a small relative tolerance.
    fn close(a: f32, b: f32) {
        let tol = 1e-4_f32 * (1.0 + a.abs().max(b.abs()));
        assert!((a - b).abs() <= tol, "expected {a} ≈ {b}");
    }

    // Six synapses exercise one full 4-wide SIMD chunk plus a 2-element scalar tail,
    // so the wrapped `unsafe { … }` ops in the SIMD primitives are actually run.
    fn six_synapses() -> ([SynapseData; 6], Vec<f32>) {
        let syn = [
            synapse(0, 0.5),
            synapse(2, -1.5),
            synapse(1, 2.0),
            synapse(3, 0.25),
            synapse(4, -0.75),
            synapse(2, 1.25),
        ];
        let acts = vec![1.0_f32, 2.0, -3.0, 0.5, 4.0];
        (syn, acts)
    }

    #[test]
    fn weighted_sum_simd_matches_scalar() {
        let (syn, acts) = six_synapses();
        let s = scalar::weighted_sum(&syn, &acts, 0, 6, 0.25);
        let n = weighted_sum_simd(&syn, &acts, 0, 6, 0.25);
        close(n, s);
    }

    #[test]
    fn weighted_sum_no_bias_simd_matches_scalar() {
        let (syn, acts) = six_synapses();
        let s = scalar::weighted_sum_no_bias(&syn, &acts, 0, 6);
        let n = weighted_sum_no_bias_simd(&syn, &acts, 0, 6);
        close(n, s);
    }

    #[test]
    fn weighted_sum_of_squares_simd_matches_scalar() {
        let (syn, acts) = six_synapses();
        let s = scalar::weighted_sum_of_squares(&syn, &acts, 0, 6);
        let n = weighted_sum_of_squares_simd(&syn, &acts, 0, 6);
        close(n, s);
    }

    #[test]
    fn weighted_sum_of_squares_v2_simd_matches_scalar() {
        let (syn, acts) = six_synapses();
        let s = scalar::weighted_sum_of_squares_v2(&syn, &acts, 0, 6, -0.5);
        let n = weighted_sum_of_squares_v2_simd(&syn, &acts, 0, 6, -0.5);
        close(n, s);
    }
}
