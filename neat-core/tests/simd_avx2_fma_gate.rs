//! Runtime feature gate for the x86_64 AVX2 record kernels (Issue #605).
//!
//! `weighted_sum_simd_8records_avx2` and `weighted_sum_interleaved_avx2` issue
//! `_mm256_fmadd_ps`. That intrinsic needs the **`fma`** target feature, and
//! AVX2 does **not** imply FMA — an `avx2`-only dispatch guard would run an FMA
//! instruction on a CPU (or under a hypervisor) that masks the feature, which is
//! undefined behaviour and an illegal instruction in practice.
//!
//! [`avx2_fma_kernels_enabled`] is the pure decision the dispatchers ask, so the
//! gate is testable on any host — including one that cannot present AVX2
//! without FMA.

use neat_core::simd::avx2_fma_kernels_enabled;

#[test]
fn avx2_without_fma_does_not_enable_the_avx2_kernels() {
    assert!(
        !avx2_fma_kernels_enabled(true, false),
        "an AVX2-only CPU must fall back to the scalar path: the AVX2 kernels \
         issue `_mm256_fmadd_ps`, which needs FMA"
    );
}

#[test]
fn fma_without_avx2_does_not_enable_the_avx2_kernels() {
    assert!(
        !avx2_fma_kernels_enabled(false, true),
        "FMA alone does not satisfy `#[target_feature(enable = \"avx2\")]`"
    );
}

#[test]
fn neither_feature_does_not_enable_the_avx2_kernels() {
    assert!(!avx2_fma_kernels_enabled(false, false));
}

#[test]
fn both_features_enable_the_avx2_kernels() {
    assert!(
        avx2_fma_kernels_enabled(true, true),
        "a CPU with both features must still take the vectorised path — the gate \
         must not disable the kernels outright"
    );
}

/// The gate is the *only* thing standing between this CPU and the FMA
/// instruction, so on x86_64 a `true` verdict must imply FMA was detected.
#[cfg(target_arch = "x86_64")]
#[test]
fn the_gate_never_admits_this_cpu_without_detected_fma() {
    let avx2 = std::arch::is_x86_feature_detected!("avx2");
    let fma = std::arch::is_x86_feature_detected!("fma");
    if avx2_fma_kernels_enabled(avx2, fma) {
        assert!(
            fma,
            "the AVX2 kernels were admitted on a CPU with no detected FMA"
        );
    }
}

/// Whatever the gate decides on this host, the dispatched result must equal the
/// scalar reference — the gate may not silently change the numbers.
#[test]
fn the_dispatched_eight_record_sum_matches_the_scalar_reference() {
    use neat_core::network::SynapseData;
    use neat_core::simd::weighted_sum_simd_8records;

    let synapses = [
        SynapseData {
            weight: 0.5,
            from_index: 0,
            synapse_type: 0,
        },
        SynapseData {
            weight: -1.25,
            from_index: 2,
            synapse_type: 0,
        },
        SynapseData {
            weight: 2.0,
            from_index: 1,
            synapse_type: 0,
        },
    ];
    // Record r gives neuron n the activation `n + r`, so every lane differs.
    let acts: Vec<Vec<f32>> = (0..8)
        .map(|r| (0..3).map(|n| n as f32 + r as f32).collect())
        .collect();
    let bias = 0.75_f32;

    let got = weighted_sum_simd_8records(
        &synapses,
        &acts[0],
        &acts[1],
        &acts[2],
        &acts[3],
        &acts[4],
        &acts[5],
        &acts[6],
        &acts[7],
        0,
        synapses.len(),
        bias,
    );
    let got = [got.0, got.1, got.2, got.3, got.4, got.5, got.6, got.7];

    for (lane, actual) in got.iter().enumerate() {
        // Independent oracle: the documented formula, summed in synapse order.
        let mut expected = bias;
        for s in &synapses {
            expected += acts[lane][s.from_index as usize] * s.weight;
        }
        assert!(
            (actual - expected).abs() < 1e-6,
            "lane {lane}: dispatched {actual} != reference {expected}"
        );
    }
}
