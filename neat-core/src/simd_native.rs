//! Native SIMD fast paths for multi-record weighted sums (Issue #1202 / #1209 on native hosts).
//!
//! `wasm32` uses `simd128` in `simd.rs`. Here **`x86_64`** uses **AVX2** (`__m256` + FMA) for
//! 8-wide and **FMA + SSE** for 4-wide; **`aarch64`** uses **NEON** (`float32x4` pairs for 8-wide).

#![allow(unsafe_op_in_unsafe_fn)]

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
            let synapse = synapses.get_unchecked(i);
            let from = synapse.from_index as usize;
            let w = synapse.weight;
            let acts = _mm256_set_ps(
                *act7.get_unchecked(from),
                *act6.get_unchecked(from),
                *act5.get_unchecked(from),
                *act4.get_unchecked(from),
                *act3.get_unchecked(from),
                *act2.get_unchecked(from),
                *act1.get_unchecked(from),
                *act0.get_unchecked(from),
            );
            let ws = _mm256_set1_ps(w);
            acc = _mm256_fmadd_ps(ws, acts, acc);
        }
        let mut out = [0.0_f32; 8];
        _mm256_storeu_ps(out.as_mut_ptr(), acc);
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
            let synapse = synapses.get_unchecked(i);
            let from = synapse.from_index as usize;
            let w = synapse.weight;
            let acts = _mm_set_ps(
                *act3.get_unchecked(from),
                *act2.get_unchecked(from),
                *act1.get_unchecked(from),
                *act0.get_unchecked(from),
            );
            let ws = _mm_set1_ps(w);
            acc = _mm_fmadd_ps(ws, acts, acc);
        }
        let mut out = [0.0_f32; 4];
        _mm_storeu_ps(out.as_mut_ptr(), acc);
        (out[0], out[1], out[2], out[3])
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
            let synapse = synapses.get_unchecked(i);
            let from = synapse.from_index as usize;
            let w = synapse.weight;
            lane03[0] = *act0.get_unchecked(from);
            lane03[1] = *act1.get_unchecked(from);
            lane03[2] = *act2.get_unchecked(from);
            lane03[3] = *act3.get_unchecked(from);
            lane47[0] = *act4.get_unchecked(from);
            lane47[1] = *act5.get_unchecked(from);
            lane47[2] = *act6.get_unchecked(from);
            lane47[3] = *act7.get_unchecked(from);
            let acts03 = vld1q_f32(lane03.as_ptr());
            let acts47 = vld1q_f32(lane47.as_ptr());
            let vw = vdupq_n_f32(w);
            acc03 = vfmaq_f32(acc03, vw, acts03);
            acc47 = vfmaq_f32(acc47, vw, acts47);
        }
        let mut o03 = [0.0_f32; 4];
        let mut o47 = [0.0_f32; 4];
        vst1q_f32(o03.as_mut_ptr(), acc03);
        vst1q_f32(o47.as_mut_ptr(), acc47);
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
            let synapse = synapses.get_unchecked(i);
            let from = synapse.from_index as usize;
            let w = synapse.weight;
            lane[0] = *act0.get_unchecked(from);
            lane[1] = *act1.get_unchecked(from);
            lane[2] = *act2.get_unchecked(from);
            lane[3] = *act3.get_unchecked(from);
            let acts = vld1q_f32(lane.as_ptr());
            let vw = vdupq_n_f32(w);
            acc = vfmaq_f32(acc, vw, acts);
        }
        let mut out = [0.0_f32; 4];
        vst1q_f32(out.as_mut_ptr(), acc);
        (out[0], out[1], out[2], out[3])
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
            return unsafe {
                x86::weighted_sum_simd_4records_fma(synapses, act0, act1, act2, act3, start, end, bias)
            };
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("neon") {
            return unsafe {
                aarch64::weighted_sum_simd_4records_neon(synapses, act0, act1, act2, act3, start, end, bias)
            };
        }
    }

    weighted_sum_simd_4records_scalar(synapses, act0, act1, act2, act3, start, end, bias)
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
}
