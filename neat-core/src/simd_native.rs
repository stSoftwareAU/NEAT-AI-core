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

use crate::network::SynapseData;

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

#[cfg(target_arch = "x86_64")]
mod x86 {
    use super::SynapseData;
    use core::arch::x86_64::*;

    /// # Safety
    /// Caller must ensure AVX2 is enabled (`is_x86_feature_detected!("avx2")`).
    #[target_feature(enable = "avx2")]
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
            // `_mm256_fmadd_ps` needs `fma`, which this `avx2` fn does not enable.
            acc = unsafe { _mm256_fmadd_ps(ws, acts, acc) };
        }
        let mut out = [0.0_f32; 8];
        unsafe { _mm256_storeu_ps(out.as_mut_ptr(), acc) };
        (
            out[0], out[1], out[2], out[3], out[4], out[5], out[6], out[7],
        )
    }

    /// # Safety
    /// Caller must ensure FMA is enabled (`is_x86_feature_detected!("fma")`).
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
        let chunk_end = start + ((end - start) / 4) * 4;
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
        let mut sum = bias + out[0] + out[1] + out[2] + out[3];
        while i < end {
            let s = unsafe { synapses.get_unchecked(i) };
            sum += unsafe { *activations.get_unchecked(s.from_index as usize) } * s.weight;
            i += 1;
        }
        sum
    }

    /// # Safety
    /// Caller must ensure FMA is enabled (`is_x86_feature_detected!("fma")`).
    #[target_feature(enable = "fma")]
    #[inline]
    pub unsafe fn weighted_sum_of_squares_fma(
        synapses: &[SynapseData],
        activations: &[f32],
        start: usize,
        end: usize,
    ) -> f32 {
        let mut acc = _mm_setzero_ps();
        let chunk_end = start + ((end - start) / 4) * 4;
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
        let mut sum = out[0] + out[1] + out[2] + out[3];
        while i < end {
            let s = unsafe { synapses.get_unchecked(i) };
            let val = unsafe { *activations.get_unchecked(s.from_index as usize) } * s.weight;
            sum += val * val;
            i += 1;
        }
        sum
    }

    /// # Safety
    /// Caller must ensure FMA is enabled (`is_x86_feature_detected!("fma")`).
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
        let chunk_end = start + ((end - start) / 4) * 4;
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
        let mut sum = out[0] + out[1] + out[2] + out[3];
        while i < end {
            let s = unsafe { synapses.get_unchecked(i) };
            let val =
                bias + unsafe { *activations.get_unchecked(s.from_index as usize) } * s.weight;
            sum += val * val;
            i += 1;
        }
        sum
    }
}

#[cfg(target_arch = "aarch64")]
mod aarch64 {
    use super::SynapseData;
    use core::arch::aarch64::*;

    /// # Safety
    /// Caller must ensure NEON is available (typical on aarch64-apple-darwin / linux-aarch64).
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

    /// # Safety
    /// Caller must ensure NEON is available.
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
        let chunk_end = start + ((end - start) / 4) * 4;
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
        let mut sum = bias + vaddvq_f32(acc);
        while i < end {
            let s = unsafe { synapses.get_unchecked(i) };
            sum += unsafe { *activations.get_unchecked(s.from_index as usize) } * s.weight;
            i += 1;
        }
        sum
    }

    /// # Safety
    /// Caller must ensure NEON is available.
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
        let chunk_end = start + ((end - start) / 4) * 4;
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
        let mut sum = vaddvq_f32(acc);
        while i < end {
            let s = unsafe { synapses.get_unchecked(i) };
            let val = unsafe { *activations.get_unchecked(s.from_index as usize) } * s.weight;
            sum += val * val;
            i += 1;
        }
        sum
    }

    /// # Safety
    /// Caller must ensure NEON is available.
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
        let chunk_end = start + ((end - start) / 4) * 4;
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
        let mut sum = vaddvq_f32(acc);
        while i < end {
            let s = unsafe { synapses.get_unchecked(i) };
            let val =
                bias + unsafe { *activations.get_unchecked(s.from_index as usize) } * s.weight;
            sum += val * val;
            i += 1;
        }
        sum
    }
}

/// 8-record weighted sum: AVX2+FMA on x86_64, NEON on aarch64, else scalar.
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
    let count = end.saturating_sub(start);
    if count == 0 {
        return (bias, bias, bias, bias, bias, bias, bias, bias);
    }

    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the `is_x86_feature_detected!("avx2")` guard above proves
            // AVX2 is available, satisfying the `#[target_feature(enable = "avx2")]`
            // precondition documented on `weighted_sum_simd_8records_avx2`.
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

/// 4-record weighted sum: FMA+SSE on x86_64, NEON on aarch64, else scalar.
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
    let count = end.saturating_sub(start);
    if count == 0 {
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

/// Scalar fallback: bias + sum(activation[from] * weight).
#[inline]
fn weighted_sum_scalar(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> f32 {
    let mut sum = bias;
    for synapse in synapses.iter().take(end).skip(start) {
        sum += activations[synapse.from_index as usize] * synapse.weight;
    }
    sum
}

/// Scalar fallback: sum((activation[from] * weight)^2).
#[inline]
fn weighted_sum_of_squares_scalar(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
) -> f32 {
    let mut sum_sq = 0.0f32;
    for synapse in synapses.iter().take(end).skip(start) {
        let val = activations[synapse.from_index as usize] * synapse.weight;
        sum_sq += val * val;
    }
    sum_sq
}

/// Scalar fallback: sum(activation[from] * weight), no bias.
#[inline]
fn weighted_sum_no_bias_scalar(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
) -> f32 {
    let mut sum = 0.0f32;
    for synapse in synapses.iter().take(end).skip(start) {
        sum += activations[synapse.from_index as usize] * synapse.weight;
    }
    sum
}

/// Scalar fallback: sum((bias + activation[from] * weight)^2).
#[inline]
fn weighted_sum_of_squares_v2_scalar(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> f32 {
    let mut sum_sq = 0.0f32;
    for synapse in synapses.iter().take(end).skip(start) {
        let val = bias + activations[synapse.from_index as usize] * synapse.weight;
        sum_sq += val * val;
    }
    sum_sq
}

// SIMD setup (lane gather, horizontal reduce) only pays off once there is at
// least one full 4-wide chunk; below that the scalar loop wins.
const SINGLE_RECORD_SIMD_MIN: usize = 4;

/// Single-record weighted sum: `bias + sum(activation[from] * weight)`.
///
/// Production forward-pass hot path. AVX2/FMA on x86_64, NEON on aarch64, scalar
/// elsewhere and for counts below one SIMD lane.
#[inline]
pub fn weighted_sum_simd(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> f32 {
    if end.saturating_sub(start) >= SINGLE_RECORD_SIMD_MIN {
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
    weighted_sum_scalar(synapses, activations, start, end, bias)
}

/// Single-record sum of squared weighted activations (Hypotenuse): `sum((a*w)^2)`.
///
/// Production forward-pass hot path. AVX2/FMA on x86_64, NEON on aarch64, scalar
/// elsewhere and for counts below one SIMD lane.
#[inline]
pub fn weighted_sum_of_squares_simd(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
) -> f32 {
    if end.saturating_sub(start) >= SINGLE_RECORD_SIMD_MIN {
        #[cfg(target_arch = "x86_64")]
        {
            if std::arch::is_x86_feature_detected!("fma") {
                // SAFETY: the FMA guard proves the `#[target_feature(enable = "fma")]`
                // precondition on `weighted_sum_of_squares_fma` holds.
                return unsafe {
                    x86::weighted_sum_of_squares_fma(synapses, activations, start, end)
                };
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            if std::arch::is_aarch64_feature_detected!("neon") {
                // SAFETY: the NEON guard proves the `#[target_feature(enable = "neon")]`
                // precondition on `weighted_sum_of_squares_neon` holds.
                return unsafe {
                    aarch64::weighted_sum_of_squares_neon(synapses, activations, start, end)
                };
            }
        }
    }
    weighted_sum_of_squares_scalar(synapses, activations, start, end)
}

/// Single-record weighted sum without bias (Mean): `sum(activation[from] * weight)`.
///
/// Production forward-pass hot path. Reuses the bias-carrying kernel with `bias = 0`
/// so the SIMD path is shared. AVX2/FMA on x86_64, NEON on aarch64, scalar elsewhere.
#[inline]
pub fn weighted_sum_no_bias_simd(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
) -> f32 {
    if end.saturating_sub(start) >= SINGLE_RECORD_SIMD_MIN {
        #[cfg(target_arch = "x86_64")]
        {
            if std::arch::is_x86_feature_detected!("fma") {
                // SAFETY: the FMA guard proves the `#[target_feature(enable = "fma")]`
                // precondition on `weighted_sum_fma` holds.
                return unsafe { x86::weighted_sum_fma(synapses, activations, start, end, 0.0) };
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            if std::arch::is_aarch64_feature_detected!("neon") {
                // SAFETY: the NEON guard proves the `#[target_feature(enable = "neon")]`
                // precondition on `weighted_sum_neon` holds.
                return unsafe {
                    aarch64::weighted_sum_neon(synapses, activations, start, end, 0.0)
                };
            }
        }
    }
    weighted_sum_no_bias_scalar(synapses, activations, start, end)
}

/// Single-record sum of squared (bias + weighted activation) (HypotenuseV2):
/// `sum((bias + a*w)^2)`.
///
/// Production forward-pass hot path. AVX2/FMA on x86_64, NEON on aarch64, scalar
/// elsewhere and for counts below one SIMD lane.
#[inline]
pub fn weighted_sum_of_squares_v2_simd(
    synapses: &[SynapseData],
    activations: &[f32],
    start: usize,
    end: usize,
    bias: f32,
) -> f32 {
    if end.saturating_sub(start) >= SINGLE_RECORD_SIMD_MIN {
        #[cfg(target_arch = "x86_64")]
        {
            if std::arch::is_x86_feature_detected!("fma") {
                // SAFETY: the FMA guard proves the `#[target_feature(enable = "fma")]`
                // precondition on `weighted_sum_of_squares_v2_fma` holds.
                return unsafe {
                    x86::weighted_sum_of_squares_v2_fma(synapses, activations, start, end, bias)
                };
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            if std::arch::is_aarch64_feature_detected!("neon") {
                // SAFETY: the NEON guard proves the `#[target_feature(enable = "neon")]`
                // precondition on `weighted_sum_of_squares_v2_neon` holds.
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
    weighted_sum_of_squares_v2_scalar(synapses, activations, start, end, bias)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::SynapseData;

    fn synapse(from: u32, weight: f32) -> SynapseData {
        SynapseData {
            weight,
            from_index: from,
            synapse_type: 0,
            _padding: [0; 3],
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
        let s = weighted_sum_scalar(&syn, &acts, 0, 6, 0.25);
        let n = weighted_sum_simd(&syn, &acts, 0, 6, 0.25);
        close(n, s);
    }

    #[test]
    fn weighted_sum_no_bias_simd_matches_scalar() {
        let (syn, acts) = six_synapses();
        let s = weighted_sum_no_bias_scalar(&syn, &acts, 0, 6);
        let n = weighted_sum_no_bias_simd(&syn, &acts, 0, 6);
        close(n, s);
    }

    #[test]
    fn weighted_sum_of_squares_simd_matches_scalar() {
        let (syn, acts) = six_synapses();
        let s = weighted_sum_of_squares_scalar(&syn, &acts, 0, 6);
        let n = weighted_sum_of_squares_simd(&syn, &acts, 0, 6);
        close(n, s);
    }

    #[test]
    fn weighted_sum_of_squares_v2_simd_matches_scalar() {
        let (syn, acts) = six_synapses();
        let s = weighted_sum_of_squares_v2_scalar(&syn, &acts, 0, 6, -0.5);
        let n = weighted_sum_of_squares_v2_simd(&syn, &acts, 0, 6, -0.5);
        close(n, s);
    }
}
