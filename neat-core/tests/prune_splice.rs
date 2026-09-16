//! Splicing a hidden `IDENTITY` neuron out of a cleaned creature (Issue #688).
//!
//! An `IDENTITY` hidden neuron forwards `bias + Σ w·a` and nothing else, so its
//! sources can be wired straight into its targets and the neuron itself thrown
//! away without moving a single output. The `IF` rewrite
//! ([`neat_core::IfRepair::Rewrite`]) leaves exactly that shape behind, and the
//! score charges a hidden neuron ten times what it charges a synapse, so the
//! splice is worth making wherever it is exact.
//!
//! The oracle here is the function itself: the creature before the splice and
//! the creature after it are compiled and activated on the same probe records,
//! and every splice this suite accepts must agree on all of them. A refusal is
//! asserted the other way round — the neuron is still there, named.

use neat_core::{
    CleanupOptions, CleanupOutcome, CreatureExport, IfRepair, MAX_NET_NEW_SYNAPSES_PER_SPLICE,
    PruneResult, SynapseKey, SynapseType, ValidateOptions, cleanup_creature, cleanup_creature_with,
    compile_creature, creature_validate, parse_creature_json, parse_synapse_type, prune_neuron,
    prune_synapse, validate_creature_topology,
};

/// `f32` activation slack: the creatures carry `f64` weights and the compiled
/// network computes in `f32`, so an exact comparison would fail on rounding.
const ACTIVATION_TOL: f32 = 1e-6;

// --- helpers ----------------------------------------------------------------

fn creature(json: &str) -> CreatureExport {
    parse_creature_json(json).expect("test fixture parses")
}

/// What both pruning entry points ask cleanup for, once this issue lands.
fn splice_options() -> CleanupOptions {
    CleanupOptions {
        if_repair: IfRepair::Rewrite,
        splice_identity: true,
    }
}

fn spliced(creature: &CreatureExport) -> CleanupOutcome {
    cleanup_creature_with(creature, splice_options()).expect("cleanup succeeds")
}

fn probe_inputs(width: usize) -> Vec<Vec<f32>> {
    let seeds: [f32; 5] = [-1.5, -0.25, 0.0, 0.75, 2.0];
    seeds
        .iter()
        .map(|s| (0..width).map(|i| s + i as f32 * 0.125).collect())
        .collect()
}

fn outputs(creature: &CreatureExport, inputs: &[f32]) -> Vec<f32> {
    let mut net = compile_creature(creature).expect("creature compiles");
    net.activate(inputs, creature.output)
}

/// Assert two creatures are the same function of the inputs.
fn assert_same_function(name: &str, left: &CreatureExport, right: &CreatureExport) {
    assert_eq!(left.input, right.input, "{name}: observation width moved");
    assert_eq!(left.output, right.output, "{name}: target width moved");
    for probe in probe_inputs(left.input) {
        let a = outputs(left, &probe);
        let b = outputs(right, &probe);
        for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
            assert!(
                (x - y).abs() <= ACTIVATION_TOL * (1.0 + x.abs()),
                "{name}: output {i} moved on {probe:?}: {x} vs {y}"
            );
        }
    }
}

fn assert_valid(name: &str, creature: &CreatureExport) {
    let options = ValidateOptions {
        neurons: None,
        connections: None,
        feedback_loop: None,
        forward_only: creature.forward_only,
    };
    creature_validate(creature, &options).unwrap_or_else(|e| panic!("{name}: invalid: {e:?}"));
    validate_creature_topology(creature)
        .unwrap_or_else(|e| panic!("{name}: failed the topology gate: {e}"));
}

fn has_neuron(creature: &CreatureExport, uuid: &str) -> bool {
    creature.neurons.iter().any(|n| n.uuid == uuid)
}

fn neuron<'a>(creature: &'a CreatureExport, uuid: &str) -> &'a neat_core::NeuronExport {
    creature
        .neurons
        .iter()
        .find(|n| n.uuid == uuid)
        .unwrap_or_else(|| panic!("no neuron {uuid}"))
}

fn weight(creature: &CreatureExport, from_uuid: &str, to_uuid: &str) -> f64 {
    creature
        .synapses
        .iter()
        .find(|s| s.from_uuid == from_uuid && s.to_uuid == to_uuid)
        .unwrap_or_else(|| panic!("no synapse {from_uuid} -> {to_uuid}"))
        .weight
}

fn role(creature: &CreatureExport, from_uuid: &str, to_uuid: &str) -> SynapseType {
    creature
        .synapses
        .iter()
        .find(|s| s.from_uuid == from_uuid && s.to_uuid == to_uuid)
        .map(|s| parse_synapse_type(s.synapse_type.as_deref()))
        .unwrap_or_else(|| panic!("no synapse {from_uuid} -> {to_uuid}"))
}

fn assert_close(name: &str, actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-9 * (1.0 + expected.abs()),
        "{name}: {actual} != {expected}"
    );
}

// --- fixtures ---------------------------------------------------------------

/// `input-0 → h-1 → h-2 → output-0`, both hiddens a pure `IDENTITY` relay.
const IDENTITY_CHAIN: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-1","bias":0.0,"squash":"IDENTITY"},
    {"type":"hidden","uuid":"h-2","bias":0.0,"squash":"IDENTITY"},
    {"type":"output","uuid":"output-0","bias":0.25,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":2.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":3.0,"fromUUID":"h-1","toUUID":"h-2"},
    {"weight":4.0,"fromUUID":"h-2","toUUID":"output-0"}
  ]
}"#;

/// `h-id` relays a constant into `if-1`'s condition, so the condition is only
/// decidable once `h-id` has gone.
const SPLICE_MAKES_IF_STATIC: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"constant","uuid":"c-1","bias":1.0},
    {"type":"hidden","uuid":"h-id","bias":0.0,"squash":"IDENTITY"},
    {"type":"hidden","uuid":"h-a","bias":0.1,"squash":"TANH"},
    {"type":"hidden","uuid":"if-1","bias":0.0,"squash":"IF"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"c-1","toUUID":"h-id"},
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-a"},
    {"weight":2.0,"fromUUID":"h-id","toUUID":"if-1","type":"condition"},
    {"weight":3.0,"fromUUID":"h-a","toUUID":"if-1","type":"positive"},
    {"weight":-5.0,"fromUUID":"h-a","toUUID":"if-1","type":"negative"},
    {"weight":1.0,"fromUUID":"if-1","toUUID":"output-0"}
  ]
}"#;

/// `h-1` carries a bias, so the splice owes its target `w_out · bias`.
const BIASED_RELAY: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-1","bias":0.5,"squash":"IDENTITY"},
    {"type":"output","uuid":"output-0","bias":0.25,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":2.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":3.0,"fromUUID":"h-1","toUUID":"output-0"}
  ]
}"#;

/// `input-0` already reaches `output-0`, so the rewired edge lands on one.
const COLLIDING_RELAY: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-1","bias":0.0,"squash":"IDENTITY"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":2.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"},
    {"weight":3.0,"fromUUID":"h-1","toUUID":"output-0"}
  ]
}"#;

/// The same collision into a `MEAN`, which reads its inward **count**.
const COLLIDING_MEAN: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-1","bias":0.0,"squash":"IDENTITY"},
    {"type":"hidden","uuid":"h-m","bias":0.0,"squash":"MEAN"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":2.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-m"},
    {"weight":3.0,"fromUUID":"h-1","toUUID":"h-m"},
    {"weight":1.0,"fromUUID":"h-m","toUUID":"output-0"}
  ]
}"#;

/// One inward edge and bias `0` into a `MEAN`: the whole term moves.
const ONE_EDGE_INTO_MEAN: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-1","bias":0.0,"squash":"IDENTITY"},
    {"type":"hidden","uuid":"h-m","bias":0.2,"squash":"MEAN"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":2.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":1.5,"fromUUID":"input-1","toUUID":"h-m"},
    {"weight":3.0,"fromUUID":"h-1","toUUID":"h-m"},
    {"weight":1.0,"fromUUID":"h-m","toUUID":"output-0"}
  ]
}"#;

/// Two inward edges into a `MEAN`: one edge cannot carry two terms.
const TWO_EDGES_INTO_MEAN: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-1","bias":0.0,"squash":"IDENTITY"},
    {"type":"hidden","uuid":"h-b","bias":0.1,"squash":"TANH"},
    {"type":"hidden","uuid":"h-m","bias":0.2,"squash":"MEAN"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":2.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":0.5,"fromUUID":"input-1","toUUID":"h-1"},
    {"weight":1.0,"fromUUID":"input-1","toUUID":"h-b"},
    {"weight":1.5,"fromUUID":"h-b","toUUID":"h-m"},
    {"weight":3.0,"fromUUID":"h-1","toUUID":"h-m"},
    {"weight":1.0,"fromUUID":"h-m","toUUID":"output-0"}
  ]
}"#;

/// One inward edge but a bias into a `MEAN`: the bias has nowhere exact to go.
const BIASED_EDGE_INTO_MEAN: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-1","bias":0.4,"squash":"IDENTITY"},
    {"type":"hidden","uuid":"h-m","bias":0.2,"squash":"MEAN"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":2.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":1.5,"fromUUID":"input-1","toUUID":"h-m"},
    {"weight":3.0,"fromUUID":"h-1","toUUID":"h-m"},
    {"weight":1.0,"fromUUID":"h-m","toUUID":"output-0"}
  ]
}"#;

/// `h-id` feeds the **negative** arm of a live `IF`.
const RELAY_INTO_IF_ROLE: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-a","bias":0.1,"squash":"TANH"},
    {"type":"hidden","uuid":"h-id","bias":0.0,"squash":"IDENTITY"},
    {"type":"hidden","uuid":"if-1","bias":0.0,"squash":"IF"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-a"},
    {"weight":2.0,"fromUUID":"input-0","toUUID":"h-id"},
    {"weight":1.0,"fromUUID":"h-a","toUUID":"if-1","type":"condition"},
    {"weight":1.0,"fromUUID":"input-1","toUUID":"if-1","type":"positive"},
    {"weight":3.0,"fromUUID":"h-id","toUUID":"if-1","type":"negative"},
    {"weight":1.0,"fromUUID":"if-1","toUUID":"output-0"}
  ]
}"#;

/// A relay fanning `sources` observations into both outputs.
///
/// The splice trades `sources + 2` edges for `2 · sources`, so the net new
/// synapse count is `sources - 2` — the one knob the growth rule reads.
fn fan_relay(sources: usize) -> CreatureExport {
    let mut synapses = String::new();
    for i in 0..sources {
        synapses.push_str(&format!(
            r#"{{"weight":{},"fromUUID":"input-{i}","toUUID":"h-1"}},"#,
            1.0 + i as f64 * 0.25
        ));
    }
    synapses.push_str(r#"{"weight":2.0,"fromUUID":"h-1","toUUID":"output-0"},"#);
    synapses.push_str(r#"{"weight":-1.5,"fromUUID":"h-1","toUUID":"output-1"}"#);
    creature(&format!(
        r#"{{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":{sources},"output":2,
      "neurons":[
        {{"type":"hidden","uuid":"h-1","bias":0.0,"squash":"IDENTITY"}},
        {{"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}},
        {{"type":"output","uuid":"output-1","bias":0.0,"squash":"IDENTITY"}}
      ],
      "synapses":[{synapses}]
    }}"#
    ))
}

/// A relay whose rewired product is a large but finite `f64`.
fn magnitude_relay(inward: f64, outward: f64) -> CreatureExport {
    creature(&format!(
        r#"{{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {{"type":"hidden","uuid":"h-1","bias":0.0,"squash":"IDENTITY"}},
        {{"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}}
      ],
      "synapses":[
        {{"weight":{inward},"fromUUID":"input-0","toUUID":"h-1"}},
        {{"weight":{outward},"fromUUID":"h-1","toUUID":"output-0"}}
      ]
    }}"#
    ))
}

// --- the splice itself ------------------------------------------------------

#[test]
fn a_chain_of_identity_relays_collapses_in_one_call() {
    let before = creature(IDENTITY_CHAIN);
    let outcome = spliced(&before);

    assert_eq!(
        outcome.spliced_neurons,
        vec!["h-1".to_string(), "h-2".to_string()],
        "both relays go, in creature order"
    );
    assert!(!has_neuron(&outcome.creature, "h-1"));
    assert!(!has_neuron(&outcome.creature, "h-2"));
    assert_eq!(outcome.creature.neurons.len(), 1, "only the output is left");
    assert_close(
        "the products multiply through the chain",
        weight(&outcome.creature, "input-0", "output-0"),
        24.0,
    );
    assert_same_function("identity_chain", &before, &outcome.creature);
    assert_valid("identity_chain", &outcome.creature);
}

#[test]
fn a_splice_that_decides_a_condition_flattens_that_if_in_the_same_call() {
    let before = creature(SPLICE_MAKES_IF_STATIC);
    let outcome = spliced(&before);

    assert!(
        outcome.spliced_neurons.contains(&"h-id".to_string()),
        "the relay in front of the condition goes first: {:?}",
        outcome.spliced_neurons
    );
    assert_eq!(
        outcome.static_if_neurons.len(),
        1,
        "the splice left the condition decidable: {:?}",
        outcome.static_if_neurons
    );
    assert_eq!(outcome.static_if_neurons[0].uuid, "if-1");
    assert_eq!(outcome.static_if_neurons[0].branch, SynapseType::Positive);
    assert!(
        outcome.spliced_neurons.contains(&"if-1".to_string()),
        "the flattened IF is itself an IDENTITY relay: {:?}",
        outcome.spliced_neurons
    );
    assert!(!has_neuron(&outcome.creature, "if-1"));
    assert_close(
        "the surviving arm reaches the output at its own weight",
        weight(&outcome.creature, "h-a", "output-0"),
        3.0,
    );
    assert_same_function("splice_makes_if_static", &before, &outcome.creature);
    assert_valid("splice_makes_if_static", &outcome.creature);
}

#[test]
fn a_relay_bias_is_folded_into_every_summing_target() {
    let before = creature(BIASED_RELAY);
    let outcome = spliced(&before);

    assert_eq!(outcome.spliced_neurons, vec!["h-1".to_string()]);
    assert_close(
        "output-0 bias takes w_out · bias",
        neuron(&outcome.creature, "output-0").bias,
        0.25 + 3.0 * 0.5,
    );
    assert_close(
        "the rewired weight is the product",
        weight(&outcome.creature, "input-0", "output-0"),
        6.0,
    );
    assert_same_function("biased_relay", &before, &outcome.creature);
    assert_valid("biased_relay", &outcome.creature);
}

#[test]
fn a_rewired_edge_that_collides_at_a_summing_target_merges_by_sum() {
    let before = creature(COLLIDING_RELAY);
    let outcome = spliced(&before);

    assert_eq!(outcome.spliced_neurons, vec!["h-1".to_string()]);
    assert_eq!(outcome.creature.synapses.len(), 1, "the two rows are one");
    assert_close(
        "the existing edge absorbs the rewired one",
        weight(&outcome.creature, "input-0", "output-0"),
        1.0 + 6.0,
    );
    assert_same_function("colliding_relay", &before, &outcome.creature);
    assert_valid("colliding_relay", &outcome.creature);
}

#[test]
fn a_rewired_edge_that_would_merge_inexactly_keeps_its_relay() {
    let before = creature(COLLIDING_MEAN);
    let outcome = spliced(&before);

    assert!(
        outcome.spliced_neurons.is_empty(),
        "a MEAN reads its inward count, so the merge would change it: {:?}",
        outcome.spliced_neurons
    );
    assert!(has_neuron(&outcome.creature, "h-1"));
    assert_same_function("colliding_mean", &before, &outcome.creature);
    assert_valid("colliding_mean", &outcome.creature);
}

#[test]
fn one_unbiased_edge_into_an_aggregate_target_is_spliced() {
    let before = creature(ONE_EDGE_INTO_MEAN);
    let outcome = spliced(&before);

    assert_eq!(outcome.spliced_neurons, vec!["h-1".to_string()]);
    assert_close(
        "the single term moves at the product weight",
        weight(&outcome.creature, "input-0", "h-m"),
        6.0,
    );
    assert_same_function("one_edge_into_mean", &before, &outcome.creature);
    assert_valid("one_edge_into_mean", &outcome.creature);
}

#[test]
fn two_edges_into_an_aggregate_target_keep_their_relay() {
    let before = creature(TWO_EDGES_INTO_MEAN);
    let outcome = spliced(&before);

    assert!(
        outcome.spliced_neurons.is_empty(),
        "two terms cannot become one at a MEAN: {:?}",
        outcome.spliced_neurons
    );
    assert!(has_neuron(&outcome.creature, "h-1"));
    assert_same_function("two_edges_into_mean", &before, &outcome.creature);
}

#[test]
fn a_biased_relay_into_an_aggregate_target_is_kept() {
    let before = creature(BIASED_EDGE_INTO_MEAN);
    let outcome = spliced(&before);

    assert!(
        outcome.spliced_neurons.is_empty(),
        "a MEAN does not sum, so the bias has nowhere exact to go: {:?}",
        outcome.spliced_neurons
    );
    assert!(has_neuron(&outcome.creature, "h-1"));
    assert_same_function("biased_edge_into_mean", &before, &outcome.creature);
}

#[test]
fn a_rewired_edge_keeps_the_if_role_it_replaces() {
    let before = creature(RELAY_INTO_IF_ROLE);
    let outcome = spliced(&before);

    assert_eq!(outcome.spliced_neurons, vec!["h-id".to_string()]);
    assert_eq!(
        role(&outcome.creature, "input-0", "if-1"),
        SynapseType::Negative,
        "the rewired edge plays the role the relay's edge played"
    );
    assert_close(
        "the product reaches the negative arm",
        weight(&outcome.creature, "input-0", "if-1"),
        6.0,
    );
    assert_same_function("relay_into_if_role", &before, &outcome.creature);
    assert_valid("relay_into_if_role", &outcome.creature);
}

// --- the boundaries ---------------------------------------------------------

#[test]
fn a_splice_that_adds_nine_net_synapses_is_made() {
    // `MAX_NET_NEW_SYNAPSES_PER_SPLICE + 2` sources over two targets: the
    // splice trades `n + 2` edges for `2n`, so the net is `n - 2 == 9`.
    let sources = MAX_NET_NEW_SYNAPSES_PER_SPLICE + 2;
    let before = fan_relay(sources);
    let outcome = spliced(&before);

    assert_eq!(
        outcome.spliced_neurons,
        vec!["h-1".to_string()],
        "nine net new synapses still cost less than the neuron they replace"
    );
    assert_eq!(
        outcome.creature.synapses.len(),
        before.synapses.len() + MAX_NET_NEW_SYNAPSES_PER_SPLICE
    );
    assert_same_function("fan_relay_nine", &before, &outcome.creature);
    assert_valid("fan_relay_nine", &outcome.creature);
}

#[test]
fn a_splice_that_would_add_ten_net_synapses_is_refused() {
    let sources = MAX_NET_NEW_SYNAPSES_PER_SPLICE + 3;
    let before = fan_relay(sources);
    let outcome = spliced(&before);

    assert!(
        outcome.spliced_neurons.is_empty(),
        "ten net new synapses cost more than the neuron: {:?}",
        outcome.spliced_neurons
    );
    assert!(has_neuron(&outcome.creature, "h-1"));
    assert_same_function("fan_relay_ten", &before, &outcome.creature);
}

#[test]
fn a_large_but_finite_rewired_weight_is_spliced() {
    let before = magnitude_relay(1e30, 1e8);
    let outcome = spliced(&before);

    assert_eq!(outcome.spliced_neurons, vec!["h-1".to_string()]);
    assert!(
        weight(&outcome.creature, "input-0", "output-0").is_finite(),
        "the product is finite, so the splice is exact in f64"
    );
    assert_valid("magnitude_finite", &outcome.creature);
}

#[test]
fn a_rewired_weight_that_overflows_keeps_its_relay() {
    let before = magnitude_relay(1e200, 1e200);
    let outcome = spliced(&before);

    assert!(
        outcome.spliced_neurons.is_empty(),
        "an infinite product is not a weight: {:?}",
        outcome.spliced_neurons
    );
    assert!(has_neuron(&outcome.creature, "h-1"));
}

#[test]
fn observation_output_and_constant_neurons_are_never_spliced() {
    // Every `IDENTITY` here is either an observation relay (`input-0`), the
    // output block, or a constant — none of them cleanup's to remove.
    let before = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"constant","uuid":"c-1","bias":1.0},
        {"type":"hidden","uuid":"h-a","bias":0.1,"squash":"TANH"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"h-a"},
        {"weight":0.5,"fromUUID":"c-1","toUUID":"output-0"},
        {"weight":1.0,"fromUUID":"h-a","toUUID":"output-0"}
      ]
    }"#,
    );
    let outcome = spliced(&before);

    assert!(outcome.spliced_neurons.is_empty());
    assert!(has_neuron(&outcome.creature, "c-1"));
    assert!(has_neuron(&outcome.creature, "output-0"));
    assert_eq!(outcome.creature.input, 1);
    assert_same_function("protected", &before, &outcome.creature);
}

#[test]
fn a_constant_source_rewired_into_a_target_obeys_the_constant_rules() {
    let before = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"constant","uuid":"c-1","bias":1.0},
        {"type":"hidden","uuid":"h-1","bias":0.0,"squash":"IDENTITY"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":2.0,"fromUUID":"c-1","toUUID":"h-1"},
        {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
        {"weight":0.5,"fromUUID":"c-1","toUUID":"output-0"},
        {"weight":3.0,"fromUUID":"h-1","toUUID":"output-0"}
      ]
    }"#,
    );
    let outcome = spliced(&before);

    assert_eq!(outcome.spliced_neurons, vec!["h-1".to_string()]);
    assert_close(
        "the constant's rewired term merges into the edge it already had",
        weight(&outcome.creature, "c-1", "output-0"),
        0.5 + 6.0,
    );
    assert_close(
        "the observation term is the product",
        weight(&outcome.creature, "input-0", "output-0"),
        3.0,
    );
    assert_eq!(
        neuron(&outcome.creature, "c-1").bias,
        neat_core::SUPPORT_CONSTANT_BIAS,
        "a support constant still carries bias 1"
    );
    assert_same_function("constant_source", &before, &outcome.creature);
    assert_valid("constant_source", &outcome.creature);
}

// --- the switch -------------------------------------------------------------

#[test]
fn the_parity_default_splices_nothing() {
    let before = creature(IDENTITY_CHAIN);
    let outcome = cleanup_creature(&before).expect("cleanup succeeds");

    assert!(
        outcome.spliced_neurons.is_empty(),
        "the splice is off by default so the TypeScript captures are untouched"
    );
    assert!(has_neuron(&outcome.creature, "h-1"));
    assert!(has_neuron(&outcome.creature, "h-2"));
}

#[test]
fn both_pruning_entry_points_splice_and_report_it() {
    // Removing `h-drop` strands nothing, but it leaves `h-1` a pure relay.
    let before = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-1","bias":0.0,"squash":"IDENTITY"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":2.0,"fromUUID":"input-0","toUUID":"h-1"},
        {"weight":1.0,"fromUUID":"input-1","toUUID":"output-0"},
        {"weight":3.0,"fromUUID":"h-1","toUUID":"output-0"}
      ]
    }"#,
    );

    let key = SynapseKey {
        from_uuid: "input-1".to_string(),
        to_uuid: "output-0".to_string(),
        role: SynapseType::Standard,
    };
    let by_synapse: PruneResult =
        prune_synapse(&before, &key, None).expect("the synapse prune succeeds");
    assert_eq!(by_synapse.spliced_neurons, vec!["h-1".to_string()]);
    assert!(!has_neuron(&by_synapse.creature, "h-1"));

    let chain = creature(IDENTITY_CHAIN);
    let mut with_extra = chain.clone();
    with_extra.neurons.insert(
        0,
        neat_core::NeuronExport {
            id: None,
            neuron_type: "hidden".to_string(),
            uuid: "h-drop".to_string(),
            bias: 0.0,
            squash: Some("TANH".to_string()),
        },
    );
    with_extra.synapses.push(neat_core::SynapseExport {
        from_uuid: "input-0".to_string(),
        to_uuid: "h-drop".to_string(),
        weight: 1.0,
        synapse_type: None,
    });
    with_extra.synapses.push(neat_core::SynapseExport {
        from_uuid: "h-drop".to_string(),
        to_uuid: "output-0".to_string(),
        weight: 1.0,
        synapse_type: None,
    });

    let by_neuron = prune_neuron(&with_extra, "h-drop", None).expect("the neuron prune succeeds");
    assert_eq!(
        by_neuron.spliced_neurons,
        vec!["h-1".to_string(), "h-2".to_string()],
        "the relay chain the removal left goes with it"
    );
}
