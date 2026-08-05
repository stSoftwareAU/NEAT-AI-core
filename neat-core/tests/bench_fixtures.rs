//! Behavioural tests for the deterministic fixtures behind the `hot_paths`
//! Criterion harness (Issue #176). The builders live in `benches/common/mod.rs`
//! and are reused here verbatim via `#[path]`, so the production-scale shape is
//! validated by a real `cargo test` run rather than only compiled in the bench.

#[path = "../benches/common/mod.rs"]
#[allow(dead_code)]
mod common;

use common::{
    FanIn, NETWORKS, NetSpec, PRODUCTION_SCORING_RECORDS, aggregate_count, build_backprop_data,
    build_inputs, build_network, build_records, with_aggregates,
};
use neat_core::squash::SquashType;

fn spec(label: &str) -> &'static NetSpec {
    NETWORKS
        .iter()
        .find(|s| s.label == label)
        .unwrap_or_else(|| panic!("no NetSpec labelled {label}"))
}

#[test]
fn production_shapes_are_registered_with_expected_dimensions() {
    let prod = spec("production");
    assert_eq!(prod.num_inputs, 2461);
    assert_eq!(prod.num_outputs, 1);
    assert_eq!(prod.num_non_inputs(), 1673);
    assert_eq!(prod.num_neurons, 4134);

    let prod2x = spec("production_2x");
    // "or larger creatures": ~2x neurons.
    assert!(
        prod2x.num_non_inputs() >= 2 * prod.num_non_inputs(),
        "production_2x should have at least double the non-input neurons"
    );
    assert!(prod2x.num_inputs >= prod.num_inputs);
}

#[test]
fn production_exact_matches_committed_grq_topology() {
    // Issue #286: the `production_exact` fixture must reproduce the committed
    // production creature topology to the synapse — 1,666 non-input
    // neurons, 21,513 synapses, 2,461 inputs — so the Criterion baseline is
    // anchored to the real production model rather than a ~13-average estimate.
    let spec = spec("production_exact");
    assert_eq!(spec.num_inputs, 2461, "production inputs");
    assert_eq!(spec.num_non_inputs(), 1666, "production non-input neurons");
    assert_eq!(spec.num_neurons, 4127, "2461 inputs + 1666 neurons");
    assert_eq!(spec.num_outputs, 1);

    let net = build_network(spec, 0x5152_5354);
    assert_eq!(
        net.neurons.len(),
        1666,
        "one NeuronData per non-input neuron"
    );
    assert_eq!(net.num_inputs, 2461);
    assert_eq!(net.num_neurons, 4127);
    assert_eq!(
        net.synapses.len(),
        21_513,
        "production_exact must build exactly 21,513 synapses"
    );

    // Per-neuron `num_synapses` must sum to the flat synapse buffer length.
    let summed: usize = net.neurons.iter().map(|n| n.num_synapses as usize).sum();
    assert_eq!(summed, 21_513);
}

#[test]
fn production_exact_fan_in_is_evenly_spread_and_varies() {
    // ExactTotal spreads 21,513 synapses across 1,666 neurons as evenly as
    // possible, so the fan-in takes exactly two values (base and base+1) with a
    // mean near the production ~13.
    let net = build_network(spec("production_exact"), 0x5152_5354);
    let distinct: std::collections::BTreeSet<u16> =
        net.neurons.iter().map(|n| n.num_synapses).collect();
    assert!(
        distinct.len() > 1,
        "exact fan-in should still vary, saw only {distinct:?}"
    );
    assert!(
        distinct.len() <= 2,
        "even distribution should use at most two fan-in values, saw {distinct:?}"
    );
    let avg_fan_in = net.synapses.len() as f64 / net.neurons.len() as f64;
    assert!(
        (12.0..=14.0).contains(&avg_fan_in),
        "average fan-in {avg_fan_in} should sit near the production ~13"
    );
}

#[test]
fn production_exact_build_is_deterministic() {
    // Seeded synthesis must reproduce byte-for-byte so the baseline numbers are
    // reproducible (Issue #286 acceptance).
    let a = build_network(spec("production_exact"), 0x5152_5354);
    let b = build_network(spec("production_exact"), 0x5152_5354);
    assert_eq!(a.synapses.len(), b.synapses.len());
    assert!(
        a.synapses
            .iter()
            .zip(&b.synapses)
            .all(|(x, y)| x.weight == y.weight && x.from_index == y.from_index),
        "same seed must reproduce identical synapses"
    );
    assert!(
        a.neurons
            .iter()
            .zip(&b.neurons)
            .all(|(x, y)| x.bias == y.bias && x.num_synapses == y.num_synapses),
        "same seed must reproduce identical neurons"
    );
}

#[test]
fn production_exact_backprop_data_has_exact_synapse_count() {
    // Regression: `build_backprop_data` must honour the ExactTotal schedule too,
    // otherwise the exact shape falls through to `draw` (which returns 0 for
    // ExactTotal) and the backprop bench runs on a synapse-free network.
    let spec = spec("production_exact");
    let data = build_backprop_data(spec, 0x1357_9BDF);
    assert_eq!(data.neurons.len(), spec.num_neurons);
    assert_eq!(data.reverse_topo_order.len(), spec.num_non_inputs());
    assert_eq!(
        data.synapses.len(),
        21_513,
        "backprop fixture must build the same 21,513 synapses as the forward fixture"
    );
    let summed: usize = data.inward_counts.iter().map(|&c| c as usize).sum();
    assert_eq!(summed, 21_513);
    assert_eq!(summed, data.inward_indices.len());
}

#[test]
fn production_exact_network_activates_to_finite_outputs() {
    let spec = spec("production_exact");
    let mut net = build_network(spec, 0x5152_5354);
    let inputs = build_inputs(spec.num_inputs, 0xA1B2_C3D4);
    let out = net.activate(&inputs, spec.num_outputs);
    assert_eq!(out.len(), spec.num_outputs);
    assert!(
        out.iter().all(|v| v.is_finite()),
        "production_exact forward pass must produce finite outputs"
    );
}

#[test]
fn build_network_matches_production_neuron_and_synapse_counts() {
    let prod = spec("production");
    let net = build_network(prod, 0x5152_5354);

    // One NeuronData per non-input neuron; total count includes the input layer.
    assert_eq!(net.neurons.len(), prod.num_non_inputs());
    assert_eq!(net.num_neurons, prod.num_neurons);
    assert_eq!(net.num_inputs, prod.num_inputs);

    // Sparse ~13 average fan-in ⇒ roughly 21.7k synapses for the real creature.
    let total_synapses = net.synapses.len();
    let avg_fan_in = total_synapses as f64 / prod.num_non_inputs() as f64;
    assert!(
        (12.0..=14.0).contains(&avg_fan_in),
        "average fan-in {avg_fan_in} should sit near the production ~13"
    );

    // num_synapses on each neuron must sum to the flat synapse buffer length.
    let summed: usize = net.neurons.iter().map(|n| n.num_synapses as usize).sum();
    assert_eq!(summed, total_synapses);
}

#[test]
fn varied_fan_in_actually_varies_unlike_fixed_shapes() {
    // The wide/shallow production shape must draw a non-constant fan-in.
    let prod = spec("production");
    let net = build_network(prod, 0x5152_5354);
    let distinct: std::collections::BTreeSet<u16> =
        net.neurons.iter().map(|n| n.num_synapses).collect();
    assert!(
        distinct.len() > 1,
        "production fan-in should vary, saw only {distinct:?}"
    );

    // A fixed-shape network keeps a single fan-in (away from the early cap).
    let small = spec("small_50");
    if let FanIn::Fixed(f) = small.fan_in {
        let net = build_network(small, 0x5152_5354);
        let tail = &net.neurons[small.num_inputs..];
        assert!(
            tail.iter().all(|n| n.num_synapses as usize == f),
            "fixed shapes should keep a constant fan-in past the early ramp"
        );
    }
}

#[test]
fn production_fixture_squash_is_homogeneous_tanh() {
    // The BASELINE.md / README.md "Fixture caveat" (Issue #261) rests on the
    // production/production_2x fixtures being uniformly `Tanh`. That homogeneity
    // makes squash-vectorisation deltas a lower bound (real production creatures also
    // run scalar-`libm` Gelu/Mish) and makes branch-prediction levers
    // unmeasurable on the fixture (the predictor already nails a one-arm match).
    // If a future change diversifies the fixture squash, this test fails so the
    // documented caveat is revisited rather than silently invalidated.
    for label in ["production", "production_2x", "production_exact"] {
        let net = build_network(spec(label), 0x5152_5354);
        assert!(
            net.neurons
                .iter()
                .all(|n| n.squash_type == SquashType::Tanh as u8),
            "{label} fixture must be homogeneous Tanh — the bench-doc caveat depends on it"
        );
    }
}

#[test]
fn build_network_is_deterministic_for_a_fixed_seed() {
    let prod = spec("production");
    let a = build_network(prod, 0x5152_5354);
    let b = build_network(prod, 0x5152_5354);

    assert_eq!(a.synapses.len(), b.synapses.len());
    assert!(
        a.synapses
            .iter()
            .zip(&b.synapses)
            .all(|(x, y)| x.weight == y.weight && x.from_index == y.from_index),
        "same seed must reproduce identical synapses"
    );
    assert!(
        a.neurons
            .iter()
            .zip(&b.neurons)
            .all(|(x, y)| x.bias == y.bias && x.num_synapses == y.num_synapses),
        "same seed must reproduce identical neurons"
    );
}

#[test]
fn production_network_activates_to_finite_outputs() {
    let prod = spec("production");
    let mut net = build_network(prod, 0x5152_5354);
    let inputs = build_inputs(prod.num_inputs, 0xA1B2_C3D4);

    let out = net.activate(&inputs, prod.num_outputs);
    assert_eq!(out.len(), prod.num_outputs);
    assert!(
        out.iter().all(|v| v.is_finite()),
        "production forward pass must produce finite outputs"
    );
}

#[test]
fn backprop_data_has_consistent_inward_adjacency_at_production_scale() {
    let prod = spec("production");
    let data = build_backprop_data(prod, 0x1357_9BDF);

    assert_eq!(data.neurons.len(), prod.num_neurons);
    assert_eq!(data.input_count as usize, prod.num_inputs);
    assert_eq!(data.output_count as usize, prod.num_outputs);
    assert_eq!(data.reverse_topo_order.len(), prod.num_non_inputs());

    // Inward counts must sum to the flat synapse buffer and index list length.
    let summed: usize = data.inward_counts.iter().map(|&c| c as usize).sum();
    assert_eq!(summed, data.synapses.len());
    assert_eq!(summed, data.inward_indices.len());
}

#[test]
fn build_records_produces_deterministic_distinct_batch_sized_to_inputs() {
    let prod = spec("production");
    let a = build_records(prod.num_inputs, 64);
    let b = build_records(prod.num_inputs, 64);

    // One record per requested count, each a full input vector.
    assert_eq!(a.len(), 64);
    assert!(
        a.iter().all(|r| r.len() == prod.num_inputs),
        "each record must be a full production-width input vector"
    );

    // Fixed-seed synthesis must reproduce byte-for-byte (non-determinism guard).
    assert_eq!(a, b, "seeded record synthesis must be reproducible");

    // Per-record seed ⇒ distinct rows, so throughput is not measured on a
    // degenerate all-identical batch.
    assert_ne!(a[0], a[1], "records should differ across the batch");
}

#[test]
fn production_scoring_record_count_is_production_representative() {
    // Build the actual batch the scoring benches time — calibrated to one
    // production training shard (~corpus/520 ≈ 4.3k records).
    let widest = spec("production_2x");
    let batch = build_records(widest.num_inputs, PRODUCTION_SCORING_RECORDS);

    // Well above the prior 2048 token batch, so records/sec reflects
    // steady-state scoring rather than warm-up.
    assert_eq!(batch.len(), PRODUCTION_SCORING_RECORDS);
    assert!(
        batch.len() > 2048,
        "scoring batch must be production-sized, not a token batch"
    );

    // Memory-feasible on the Apple Silicon host class: even the widest shape's
    // batch stays well under 1 GiB so the harness can materialise it.
    let bytes: usize = batch
        .iter()
        .map(|r| r.len() * std::mem::size_of::<f32>())
        .sum();
    assert!(
        bytes < (1usize << 30),
        "production batch ({bytes} bytes) must fit comfortably in RAM"
    );
}

#[test]
fn score_records_on_production_batch_yields_finite_ordered_outputs() {
    let prod = spec("production");
    let net = build_network(prod, 0x5EED);
    // A small production-shaped batch keeps the debug test fast while still
    // exercising the exact fixture path the scoring benches time.
    let records = build_records(net.num_inputs(), 96);

    // `score_records_flat` takes the batch as one contiguous buffer (Issue #386)
    // and returns a flat `[record * num_outputs]` buffer (Issue #229), so assert
    // on the flat length and finiteness rather than per-record rows.
    let stride = net.num_inputs();
    let inputs: Vec<f32> = records.iter().flat_map(|r| r.iter().copied()).collect();
    let out = net.score_records_flat(&inputs, stride, prod.num_outputs);
    assert_eq!(out.len(), records.len() * prod.num_outputs);
    assert!(
        out.iter().all(|v| v.is_finite()),
        "production scoring must produce finite outputs"
    );
}

#[test]
fn aggregate_rewrite_hits_the_requested_frequency() {
    // Issue #510 - the aggregate_frequency sweep is only meaningful if the
    // requested percentage is what the fixture actually carries.
    let spec = spec("production_exact");
    for (percent, expected) in [(0usize, 0usize), (10, 167), (50, 833), (100, 1666)] {
        let net = with_aggregates(build_network(spec, 0x5152_5354), percent);
        assert_eq!(
            aggregate_count(&net),
            expected,
            "{percent}% aggregate rewrite produced the wrong neuron count"
        );
    }
}

#[test]
fn aggregate_rewrite_leaves_topology_and_weights_untouched() {
    // Only squash types (and If synapse types) may change: otherwise the sweep
    // would be measuring a different creature at each frequency.
    let spec = spec("production_exact");
    let base = build_network(spec, 0x5152_5354);
    let rewritten = with_aggregates(build_network(spec, 0x5152_5354), 50);

    assert_eq!(rewritten.num_neurons, base.num_neurons);
    assert_eq!(rewritten.synapses.len(), base.synapses.len());
    for (i, (got, want)) in rewritten
        .synapses
        .iter()
        .zip(base.synapses.iter())
        .enumerate()
    {
        assert_eq!(got.from_index, want.from_index, "synapse {i} source moved");
        assert_eq!(
            got.weight.to_bits(),
            want.weight.to_bits(),
            "synapse {i} weight changed"
        );
    }
    for (i, (got, want)) in rewritten
        .neurons
        .iter()
        .zip(base.neurons.iter())
        .enumerate()
    {
        assert_eq!(
            got.start_synapse, want.start_synapse,
            "neuron {i} span moved"
        );
        assert_eq!(
            got.num_synapses, want.num_synapses,
            "neuron {i} fan-in changed"
        );
        assert_eq!(
            got.bias.to_bits(),
            want.bias.to_bits(),
            "neuron {i} bias changed"
        );
    }
}

#[test]
fn rewritten_if_neurons_carry_a_condition_synapse() {
    // An If neuron with no condition synapse would always take the negative
    // branch, so the If arm would not be exercised at all.
    let spec = spec("production_exact");
    let net = with_aggregates(build_network(spec, 0x5152_5354), 100);
    let mut checked = 0;
    for neuron in net
        .neurons
        .iter()
        .filter(|n| SquashType::from(n.squash_type) == SquashType::If && n.num_synapses > 0)
    {
        let start = neuron.start_synapse as usize;
        let end = start + neuron.num_synapses as usize;
        assert!(
            net.synapses[start..end].iter().any(|s| s.synapse_type == 1),
            "If neuron at {start} has no condition synapse"
        );
        checked += 1;
    }
    assert!(checked > 0, "fixture produced no If neurons to check");
}
