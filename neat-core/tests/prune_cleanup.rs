//! The canonical fixed-point cleanup, rule by rule (Issue #589).
//!
//! [`neat_core::cleanup_creature`] is the one entry point every prune operation
//! calls after its requested deletion, so these tests drive it the way those
//! callers will: cut a neuron or an edge out of a creature by hand, hand the
//! wreckage to cleanup, and check what comes back.
//!
//! Two oracles do the heavy lifting, and neither is a second copy of the
//! implementation:
//!
//! - **the function itself.** A creature with a stranded hidden neuron still
//!   computes a perfectly well-defined number for every record, so the
//!   pre-cleanup creature is compiled and activated, and the cleaned one must
//!   agree on every probe. Nothing but an exact rewrite can pass that.
//! - **the TypeScript captures** in [`neat_core::PRUNE_PARITY_CASES`], which
//!   are NEAT-AI's own output for the same deletions (Issue #588).
//!
//! Where cleanup's canonical form differs from the TypeScript capture — a
//! folded constant carries bias `1` here and the folded value in its weights,
//! rather than the value in its bias — the parity assertion is on the numbers
//! the two creatures produce, which is the property that actually matters.

use neat_core::prune_fixtures::{
    CASCADE_ORPHAN_FEEDERS, CONSTANT_MOVES_INTO_PREFIX, EDGE_ROLE_IDENTITY,
    EDGE_TARGET_BECOMES_CONSTANT, IF_REPAIR_COALESCES_ROLES, MEMETIC_DROPPED_ON_REMOVAL,
};
use neat_core::{
    CleanupError, CleanupOutcome, CreatureExport, MAX_SUPPORT_CONSTANTS, PRUNE_PARITY_CASES,
    SUPPORT_CONSTANT_BIAS, SquashType, SynapseType, ValidateOptions, cleanup_creature,
    compile_creature, creature_validate, parse_creature_json, parse_synapse_type,
    validate_creature_topology,
};

const OPTIONS: ValidateOptions = ValidateOptions {
    neurons: None,
    connections: None,
    feedback_loop: None,
    forward_only: true,
};

/// `f32` activation slack: the creatures carry `f64` weights and the compiled
/// network computes in `f32`, so an exact comparison would fail on rounding.
const ACTIVATION_TOL: f32 = 1e-5;

// --- helpers ----------------------------------------------------------------

fn creature(json: &str) -> CreatureExport {
    parse_creature_json(json).expect("test fixture parses")
}

/// The caller's half of a prune: drop one neuron and every edge naming it.
///
/// This is deliberately *not* cleanup — it is the wreckage cleanup is handed.
fn without_neuron(creature: &CreatureExport, uuid: &str) -> CreatureExport {
    let mut cut = creature.clone();
    cut.neurons.retain(|n| n.uuid != uuid);
    cut.synapses
        .retain(|s| s.from_uuid != uuid && s.to_uuid != uuid);
    cut
}

/// The caller's half of a synapse prune: drop one `(from, to, role)` triple.
fn without_synapse(
    creature: &CreatureExport,
    from_uuid: &str,
    to_uuid: &str,
    role: SynapseType,
) -> CreatureExport {
    let mut cut = creature.clone();
    let before = cut.synapses.len();
    cut.synapses.retain(|s| {
        !(s.from_uuid == from_uuid
            && s.to_uuid == to_uuid
            && parse_synapse_type(s.synapse_type.as_deref()) == role)
    });
    assert_eq!(
        cut.synapses.len() + 1,
        before,
        "no {from_uuid} -> {to_uuid} ({role:?}) to remove"
    );
    cut
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

fn neuron<'a>(creature: &'a CreatureExport, uuid: &str) -> &'a neat_core::NeuronExport {
    creature
        .neurons
        .iter()
        .find(|n| n.uuid == uuid)
        .unwrap_or_else(|| panic!("no neuron {uuid}"))
}

fn has_neuron(creature: &CreatureExport, uuid: &str) -> bool {
    creature.neurons.iter().any(|n| n.uuid == uuid)
}

fn weight(creature: &CreatureExport, from_uuid: &str, to_uuid: &str) -> f64 {
    creature
        .synapses
        .iter()
        .find(|s| s.from_uuid == from_uuid && s.to_uuid == to_uuid)
        .unwrap_or_else(|| panic!("no synapse {from_uuid} -> {to_uuid}"))
        .weight
}

fn inward(creature: &CreatureExport, uuid: &str) -> usize {
    creature
        .synapses
        .iter()
        .filter(|s| s.to_uuid == uuid)
        .count()
}

fn outward(creature: &CreatureExport, uuid: &str) -> usize {
    creature
        .synapses
        .iter()
        .filter(|s| s.from_uuid == uuid)
        .count()
}

fn constants(creature: &CreatureExport) -> Vec<&neat_core::NeuronExport> {
    creature
        .neurons
        .iter()
        .filter(|n| n.neuron_type == "constant")
        .collect()
}

/// The documented logistic in `f64` — an independent route to the value a
/// zero-input hidden neuron folds, derived from the formula rather than read
/// back out of the squash kernel the fold uses.
fn logistic(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

fn cleaned(creature: &CreatureExport) -> CleanupOutcome {
    cleanup_creature(creature).expect("cleanup succeeds")
}

/// A creature whose only hidden neuron has **no** inward edge — the shape a
/// synapse prune leaves behind — carrying the named squash and bias.
fn stranded_hidden_creature(squash: &str, bias: f64) -> CreatureExport {
    CreatureExport {
        input: 1,
        output: 1,
        neurons: vec![
            neat_core::NeuronExport {
                id: None,
                neuron_type: "hidden".to_string(),
                uuid: "h-1".to_string(),
                bias,
                squash: Some(squash.to_string()),
            },
            neat_core::NeuronExport {
                id: None,
                neuron_type: "output".to_string(),
                uuid: "output-0".to_string(),
                bias: 0.0,
                squash: Some("IDENTITY".to_string()),
            },
        ],
        synapses: vec![
            neat_core::SynapseExport {
                from_uuid: "input-0".to_string(),
                to_uuid: "output-0".to_string(),
                weight: 1.0,
                synapse_type: None,
            },
            neat_core::SynapseExport {
                from_uuid: "h-1".to_string(),
                to_uuid: "output-0".to_string(),
                weight: 2.0,
                synapse_type: None,
            },
        ],
        semantic_version: Some("4.0.0".to_string()),
        forward_only: true,
        memetic: None,
    }
}

// --- the scenario table -----------------------------------------------------

/// One creature as a caller leaves it, with whether cleanup's rewrite of it is
/// required to be *exact*.
///
/// Only the `IF` repair is inexact: an `IF` that has lost a required role can
/// no longer branch, so replacing it with the sum of what remains is a
/// structural repair, not an identity.
struct Scenario {
    name: &'static str,
    creature: CreatureExport,
    exact: bool,
}

/// A two-level cascade: `input-0 -> h-c -> h-a -> h-x -> output-0`, with a
/// second chain through `h-d`/`h-b` and a direct edge keeping the output fed.
const CASCADE_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-c","bias":0.1,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-d","bias":0.2,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-a","bias":0.3,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-b","bias":0.4,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-x","bias":0.5,"squash":"LOGISTIC"},
    {"type":"output","uuid":"output-0","bias":0.6,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-c"},
    {"weight":0.5,"fromUUID":"input-0","toUUID":"output-0"},
    {"weight":1.0,"fromUUID":"input-1","toUUID":"h-d"},
    {"weight":1.0,"fromUUID":"h-c","toUUID":"h-a"},
    {"weight":1.0,"fromUUID":"h-d","toUUID":"h-b"},
    {"weight":1.0,"fromUUID":"h-a","toUUID":"h-x"},
    {"weight":1.0,"fromUUID":"h-b","toUUID":"h-x"},
    {"weight":1.0,"fromUUID":"h-x","toUUID":"output-0"}
  ]
}"#;

/// `h-shared` feeds both the branch being cut and the output directly, so the
/// cut must cost the branch and nothing else.
const SHARED_SUBGRAPH_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-shared","bias":0.2,"squash":"TANH"},
    {"type":"hidden","uuid":"h-branch","bias":0.3,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-x","bias":0.4,"squash":"LOGISTIC"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-shared"},
    {"weight":1.0,"fromUUID":"input-1","toUUID":"h-branch"},
    {"weight":0.5,"fromUUID":"h-shared","toUUID":"h-branch"},
    {"weight":0.75,"fromUUID":"h-shared","toUUID":"output-0"},
    {"weight":1.0,"fromUUID":"h-branch","toUUID":"h-x"},
    {"weight":1.0,"fromUUID":"h-x","toUUID":"output-0"}
  ]
}"#;

/// A constant supporting one hidden neuron and nothing else, so cutting that
/// neuron leaves the constant unreferenced.
const CONSTANT_SUPPORT_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"constant","uuid":"c-1","bias":1.0},
    {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":0.5,"fromUUID":"input-0","toUUID":"output-0"},
    {"weight":-0.25,"fromUUID":"c-1","toUUID":"h-1"},
    {"weight":1.0,"fromUUID":"h-1","toUUID":"output-0"}
  ]
}"#;

/// Five bias-1 constants, two more than a canonical creature carries.
const FIVE_CONSTANTS_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"constant","uuid":"c-1","bias":1.0},
    {"type":"constant","uuid":"c-2","bias":1.0},
    {"type":"constant","uuid":"c-3","bias":1.0},
    {"type":"constant","uuid":"c-4","bias":1.0},
    {"type":"constant","uuid":"c-5","bias":1.0},
    {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":0.25,"fromUUID":"c-1","toUUID":"h-1"},
    {"weight":-0.5,"fromUUID":"c-2","toUUID":"h-1"},
    {"weight":0.75,"fromUUID":"c-3","toUUID":"output-0"},
    {"weight":-1.25,"fromUUID":"c-4","toUUID":"output-0"},
    {"weight":2.0,"fromUUID":"c-5","toUUID":"h-1"},
    {"weight":1.0,"fromUUID":"h-1","toUUID":"output-0"}
  ]
}"#;

/// A `MEAN` target divides by its inward **count**, so two folded constants
/// must stay two edges; a `MINIMUM` target takes the smallest term, so they may
/// merge to the smaller weight.
const AGGREGATE_TARGETS_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-a","bias":0.3,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-b","bias":-0.2,"squash":"TANH"},
    {"type":"hidden","uuid":"mean-1","bias":0.05,"squash":"MEAN"},
    {"type":"hidden","uuid":"min-1","bias":0.0,"squash":"MINIMUM"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-a"},
    {"weight":1.0,"fromUUID":"input-0","toUUID":"mean-1"},
    {"weight":1.0,"fromUUID":"input-0","toUUID":"min-1"},
    {"weight":2.0,"fromUUID":"h-a","toUUID":"mean-1"},
    {"weight":3.0,"fromUUID":"h-a","toUUID":"min-1"},
    {"weight":-1.5,"fromUUID":"h-b","toUUID":"mean-1"},
    {"weight":0.5,"fromUUID":"h-b","toUUID":"min-1"},
    {"weight":1.0,"fromUUID":"mean-1","toUUID":"output-0"},
    {"weight":1.0,"fromUUID":"min-1","toUUID":"output-0"}
  ]
}"#;

/// Two output neurons, one of which the cut leaves with nothing feeding it.
const TWO_OUTPUTS_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":2,
  "neurons":[
    {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
    {"type":"output","uuid":"output-0","bias":0.2,"squash":"IDENTITY"},
    {"type":"output","uuid":"output-1","bias":0.3,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":1.0,"fromUUID":"h-1","toUUID":"output-0"},
    {"weight":1.0,"fromUUID":"h-1","toUUID":"output-1"}
  ]
}"#;

/// `h-2` is listed *after* `h-1`, so folding it into a constant leaves a
/// constant behind a hidden — the order validation rule 11 refuses.
const FOLD_BEHIND_A_HIDDEN_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-2","bias":0.3,"squash":"TANH"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
    {"weight":1.0,"fromUUID":"input-1","toUUID":"h-2"},
    {"weight":1.0,"fromUUID":"h-1","toUUID":"output-0"},
    {"weight":2.0,"fromUUID":"h-2","toUUID":"output-0"}
  ]
}"#;

fn scenarios() -> Vec<Scenario> {
    let aggregates = creature(AGGREGATE_TARGETS_JSON);
    vec![
        Scenario {
            name: "multi_level_cascade",
            creature: without_neuron(&creature(CASCADE_JSON), "h-x"),
            exact: false, // `h-x` fed the output, so removing it is the caller's change
        },
        Scenario {
            name: "shared_subgraph",
            creature: without_neuron(&creature(SHARED_SUBGRAPH_JSON), "h-x"),
            exact: false,
        },
        Scenario {
            name: "zero_input_hidden",
            creature: without_synapse(
                &EDGE_TARGET_BECOMES_CONSTANT.before(),
                "input-0",
                "h-1",
                SynapseType::Standard,
            ),
            exact: true,
        },
        Scenario {
            name: "unreferenced_constant",
            creature: without_neuron(&creature(CONSTANT_SUPPORT_JSON), "h-1"),
            exact: false,
        },
        Scenario {
            name: "five_constants",
            creature: creature(FIVE_CONSTANTS_JSON),
            exact: true,
        },
        Scenario {
            name: "constant_prefix",
            creature: without_synapse(
                &CONSTANT_MOVES_INTO_PREFIX.before(),
                "input-0",
                "h-2",
                SynapseType::Standard,
            ),
            exact: true,
        },
        Scenario {
            name: "aggregate_targets",
            creature: without_synapse(&aggregates, "input-0", "h-a", SynapseType::Standard),
            exact: true,
        },
        Scenario {
            name: "if_missing_condition",
            creature: without_neuron(&IF_REPAIR_COALESCES_ROLES.before(), "h-cond"),
            exact: false,
        },
        Scenario {
            name: "unfed_output",
            creature: without_neuron(&creature(TWO_OUTPUTS_JSON), "h-1"),
            exact: false,
        },
        Scenario {
            name: "fold_behind_a_hidden",
            creature: without_synapse(
                &creature(FOLD_BEHIND_A_HIDDEN_JSON),
                "input-1",
                "h-2",
                SynapseType::Standard,
            ),
            exact: true,
        },
        Scenario {
            name: "already_canonical",
            creature: creature(TWO_OUTPUTS_JSON),
            exact: true,
        },
    ]
}

// --- rules that hold for every scenario -------------------------------------

#[test]
fn cleanup_never_returns_an_invalid_creature() {
    for scenario in scenarios() {
        let outcome = cleanup_creature(&scenario.creature)
            .unwrap_or_else(|e| panic!("{}: cleanup failed: {e}", scenario.name));
        creature_validate(&outcome.creature, &OPTIONS)
            .unwrap_or_else(|e| panic!("{}: {} — {}", scenario.name, e.reason, e.message));
        validate_creature_topology(&outcome.creature)
            .unwrap_or_else(|e| panic!("{}: {e:?}", scenario.name));
        compile_creature(&outcome.creature)
            .unwrap_or_else(|e| panic!("{}: does not compile: {e:?}", scenario.name));
    }
}

#[test]
fn an_exact_cleanup_computes_the_same_numbers_as_the_creature_it_was_given() {
    let mut checked = 0;
    for scenario in scenarios() {
        if !scenario.exact {
            continue;
        }
        checked += 1;
        let outcome = cleaned(&scenario.creature);
        assert_same_function(scenario.name, &scenario.creature, &outcome.creature);
    }
    assert!(checked >= 4, "too few exact scenarios to prove the rule");
}

#[test]
fn every_surviving_constant_is_a_bias_one_support_node() {
    for scenario in scenarios() {
        let outcome = cleaned(&scenario.creature);
        for constant in constants(&outcome.creature) {
            assert_eq!(
                constant.bias, SUPPORT_CONSTANT_BIAS,
                "{}: constant {} kept bias {}",
                scenario.name, constant.uuid, constant.bias
            );
            assert!(
                constant.squash.is_none(),
                "{}: constant {} kept a squash",
                scenario.name,
                constant.uuid
            );
        }
    }
}

#[test]
fn no_creature_comes_back_with_more_than_three_constants() {
    for scenario in scenarios() {
        let outcome = cleaned(&scenario.creature);
        assert!(
            constants(&outcome.creature).len() <= MAX_SUPPORT_CONSTANTS,
            "{}: {} constants survived",
            scenario.name,
            constants(&outcome.creature).len()
        );
        assert!(
            outcome.surplus_constants.is_empty(),
            "{}: reported a surplus it did not have",
            scenario.name
        );
    }
}

#[test]
fn a_budget_the_maths_will_not_allow_is_reported_rather_than_forced() {
    // Five constants all reading one MEAN: merging any two would change the
    // divisor, so cleanup keeps them and *names* the surplus. Silently leaving
    // the budget broken is the failure this test exists to prevent.
    let five_on_a_mean = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"constant","uuid":"c-1","bias":1.0},
        {"type":"constant","uuid":"c-2","bias":1.0},
        {"type":"constant","uuid":"c-3","bias":1.0},
        {"type":"constant","uuid":"c-4","bias":1.0},
        {"type":"constant","uuid":"c-5","bias":1.0},
        {"type":"hidden","uuid":"mean-1","bias":0.0,"squash":"MEAN"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"mean-1"},
        {"weight":0.1,"fromUUID":"c-1","toUUID":"mean-1"},
        {"weight":0.2,"fromUUID":"c-2","toUUID":"mean-1"},
        {"weight":0.3,"fromUUID":"c-3","toUUID":"mean-1"},
        {"weight":0.4,"fromUUID":"c-4","toUUID":"mean-1"},
        {"weight":0.5,"fromUUID":"c-5","toUUID":"mean-1"},
        {"weight":1.0,"fromUUID":"mean-1","toUUID":"output-0"}
      ]
    }"#,
    );
    let outcome = cleaned(&five_on_a_mean);

    assert_eq!(
        constants(&outcome.creature).len(),
        5,
        "cleanup forced the budget and changed what the MEAN computes"
    );
    assert_eq!(
        outcome.surplus_constants,
        vec!["c-4".to_string(), "c-5".to_string()],
        "the constants over budget were not named"
    );
    assert_same_function("five_on_a_mean", &five_on_a_mean, &outcome.creature);
}

#[test]
fn no_orphan_survives_cleanup() {
    for scenario in scenarios() {
        let outcome = cleaned(&scenario.creature);
        let out = &outcome.creature;
        for n in &out.neurons {
            match n.neuron_type.as_str() {
                "hidden" => {
                    assert!(
                        inward(out, &n.uuid) > 0 && outward(out, &n.uuid) > 0,
                        "{}: hidden {} is an orphan",
                        scenario.name,
                        n.uuid
                    );
                }
                "constant" => {
                    assert_eq!(
                        inward(out, &n.uuid),
                        0,
                        "{}: constant {} takes an inward edge",
                        scenario.name,
                        n.uuid
                    );
                    assert!(
                        outward(out, &n.uuid) > 0,
                        "{}: constant {} is referenced by nothing",
                        scenario.name,
                        n.uuid
                    );
                }
                _ => {}
            }
        }
    }
}

#[test]
fn the_computational_slice_comes_back_constants_then_hiddens_then_outputs() {
    for scenario in scenarios() {
        let outcome = cleaned(&scenario.creature);
        let rank = |t: &str| match t {
            "constant" => 0,
            "hidden" => 1,
            _ => 2,
        };
        let ranks: Vec<usize> = outcome
            .creature
            .neurons
            .iter()
            .map(|n| rank(&n.neuron_type))
            .collect();
        assert!(
            ranks.windows(2).all(|w| w[0] <= w[1]),
            "{}: neuron order is not constants, hiddens, outputs",
            scenario.name
        );
    }
}

#[test]
fn synapses_come_back_in_canonical_from_to_role_order() {
    for scenario in scenarios() {
        let outcome = cleaned(&scenario.creature);
        let out = &outcome.creature;
        let index = |uuid: &str| -> usize {
            if let Some(rest) = uuid.strip_prefix("input-") {
                return rest.parse::<usize>().expect("input index");
            }
            out.input
                + out
                    .neurons
                    .iter()
                    .position(|n| n.uuid == uuid)
                    .unwrap_or_else(|| panic!("{}: unknown neuron {uuid}", scenario.name))
        };
        let keys: Vec<(usize, usize, u8)> = out
            .synapses
            .iter()
            .map(|s| {
                (
                    index(&s.from_uuid),
                    index(&s.to_uuid),
                    parse_synapse_type(s.synapse_type.as_deref()) as u8,
                )
            })
            .collect();
        assert!(
            keys.windows(2).all(|w| w[0] < w[1]),
            "{}: synapses are not in strictly increasing (from, to, role) order: {keys:?}",
            scenario.name
        );
    }
}

#[test]
fn cleanup_reaches_a_fixed_point_and_stays_there() {
    for scenario in scenarios() {
        let once = cleaned(&scenario.creature);
        let twice = cleaned(&once.creature);
        assert!(
            !twice.changed,
            "{}: a second cleanup still changed the creature",
            scenario.name
        );
        assert_eq!(
            twice.creature, once.creature,
            "{}: cleanup is not idempotent",
            scenario.name
        );
        assert!(
            twice.removed_neurons.is_empty()
                && twice.folded_neurons.is_empty()
                && twice.merged_constants.is_empty()
                && twice.rescaled_constants.is_empty()
                && twice.downgraded_if_neurons.is_empty(),
            "{}: a settled creature still reported work",
            scenario.name
        );
    }
}

#[test]
fn cleanup_is_deterministic() {
    for scenario in scenarios() {
        let first = cleaned(&scenario.creature);
        let second = cleaned(&scenario.creature);
        assert_eq!(
            first.creature, second.creature,
            "{}: two runs disagreed",
            scenario.name
        );
        assert_eq!(
            first.removed_neurons, second.removed_neurons,
            "{}: two runs reported different removals",
            scenario.name
        );
    }
}

#[test]
fn observation_and_output_neurons_are_never_removed() {
    for scenario in scenarios() {
        let before_outputs: Vec<String> = scenario
            .creature
            .neurons
            .iter()
            .filter(|n| n.neuron_type == "output")
            .map(|n| n.uuid.clone())
            .collect();
        let outcome = cleaned(&scenario.creature);
        let after_outputs: Vec<String> = outcome
            .creature
            .neurons
            .iter()
            .filter(|n| n.neuron_type == "output")
            .map(|n| n.uuid.clone())
            .collect();
        assert_eq!(
            before_outputs, after_outputs,
            "{}: the output block moved",
            scenario.name
        );
        assert_eq!(
            outcome.creature.input, scenario.creature.input,
            "{}: the observation width moved",
            scenario.name
        );
        assert_eq!(
            outcome.creature.output, scenario.creature.output,
            "{}: the declared target count moved",
            scenario.name
        );
    }
}

#[test]
fn every_captured_typescript_result_settles_into_the_canonical_form() {
    for case in PRUNE_PARITY_CASES {
        let after = case.after();
        let outcome = cleanup_creature(&after)
            .unwrap_or_else(|e| panic!("{}: cleanup of the capture failed: {e}", case.name));
        // The captures are already free of dead structure, so whatever cleanup
        // does to them must leave the same function behind.
        assert_same_function(case.name, &after, &outcome.creature);
        for constant in constants(&outcome.creature) {
            assert_eq!(
                constant.bias, SUPPORT_CONSTANT_BIAS,
                "{}: constant {} kept bias {}",
                case.name, constant.uuid, constant.bias
            );
        }
    }
}

// --- the cascade ------------------------------------------------------------

#[test]
fn a_multi_level_cascade_removes_every_orphaned_feeder() {
    let cut = without_neuron(&CASCADE_ORPHAN_FEEDERS.before(), "h-x");
    let outcome = cleaned(&cut);

    assert_eq!(
        outcome.creature,
        CASCADE_ORPHAN_FEEDERS.after(),
        "cleanup did not reproduce the TypeScript cascade"
    );
    let mut removed = outcome.removed_neurons.clone();
    removed.sort();
    assert_eq!(removed, vec!["h-a", "h-b", "h-c", "h-d"]);
    assert!(outcome.passes >= 2, "a cascade needs more than one pass");
}

#[test]
fn a_shared_subgraph_keeps_the_branch_that_is_still_read() {
    let cut = without_neuron(&creature(SHARED_SUBGRAPH_JSON), "h-x");
    let outcome = cleaned(&cut);

    assert!(
        !has_neuron(&outcome.creature, "h-branch"),
        "the orphaned branch survived"
    );
    assert!(
        has_neuron(&outcome.creature, "h-shared"),
        "h-shared still feeds the output and must survive"
    );
    assert_eq!(
        weight(&outcome.creature, "h-shared", "output-0"),
        0.75,
        "the surviving edge was rewritten"
    );
}

#[test]
fn a_long_chain_of_orphaned_feeders_is_cleaned_in_one_call() {
    // The fixed point has to hold at scale, not just on a two-level fixture:
    // 100 hidden neurons in a chain behind the neuron the caller removed.
    const CHAIN: usize = 100;
    let mut creature = CreatureExport {
        input: 1,
        output: 1,
        neurons: Vec::new(),
        synapses: Vec::new(),
        semantic_version: Some("4.0.0".to_string()),
        forward_only: true,
        memetic: None,
    };
    for i in 0..CHAIN {
        creature.neurons.push(neat_core::NeuronExport {
            id: None,
            neuron_type: "hidden".to_string(),
            uuid: format!("h-{i}"),
            bias: 0.1,
            squash: Some("LOGISTIC".to_string()),
        });
        creature.synapses.push(neat_core::SynapseExport {
            from_uuid: if i == 0 {
                "input-0".to_string()
            } else {
                format!("h-{}", i - 1)
            },
            to_uuid: format!("h-{i}"),
            weight: 1.0,
            synapse_type: None,
        });
    }
    creature.neurons.push(neat_core::NeuronExport {
        id: None,
        neuron_type: "output".to_string(),
        uuid: "output-0".to_string(),
        bias: 0.0,
        squash: Some("IDENTITY".to_string()),
    });
    creature.synapses.push(neat_core::SynapseExport {
        from_uuid: format!("h-{}", CHAIN - 1),
        to_uuid: "output-0".to_string(),
        weight: 1.0,
        synapse_type: None,
    });
    creature.synapses.push(neat_core::SynapseExport {
        from_uuid: "input-0".to_string(),
        to_uuid: "output-0".to_string(),
        weight: 0.5,
        synapse_type: None,
    });

    let cut = without_neuron(&creature, &format!("h-{}", CHAIN - 1));
    let outcome = cleaned(&cut);

    assert_eq!(
        outcome.removed_neurons.len(),
        CHAIN - 1,
        "the whole chain behind the removed neuron should have gone"
    );
    assert_eq!(
        outcome.creature.neurons.len(),
        1,
        "only the output should remain"
    );
    creature_validate(&outcome.creature, &OPTIONS).expect("the cleaned chain validates");
}

#[test]
fn many_stranded_hidden_neurons_share_a_single_support_constant() {
    // Every fold reuses the constant the first fold made, so a creature that
    // strands fifty hidden neurons comes back with one support node, not fifty.
    const STRANDED: usize = 50;
    let mut creature = CreatureExport {
        input: 1,
        output: 1,
        neurons: Vec::new(),
        synapses: Vec::new(),
        semantic_version: Some("4.0.0".to_string()),
        forward_only: true,
        memetic: None,
    };
    for i in 0..STRANDED {
        creature.neurons.push(neat_core::NeuronExport {
            id: None,
            neuron_type: "hidden".to_string(),
            uuid: format!("h-{i}"),
            bias: 0.01 * i as f64,
            squash: Some("TANH".to_string()),
        });
        creature.synapses.push(neat_core::SynapseExport {
            from_uuid: format!("h-{i}"),
            to_uuid: "output-0".to_string(),
            weight: 0.1,
            synapse_type: None,
        });
    }
    creature.neurons.push(neat_core::NeuronExport {
        id: None,
        neuron_type: "output".to_string(),
        uuid: "output-0".to_string(),
        bias: 0.0,
        squash: Some("IDENTITY".to_string()),
    });
    creature.synapses.push(neat_core::SynapseExport {
        from_uuid: "input-0".to_string(),
        to_uuid: "output-0".to_string(),
        weight: 1.0,
        synapse_type: None,
    });

    let outcome = cleaned(&creature);
    assert_eq!(
        constants(&outcome.creature).len(),
        1,
        "fifty folds should share one support constant"
    );
    assert_eq!(
        outcome.creature.neurons.len(),
        2,
        "one support constant and the output"
    );
    assert_same_function("many_stranded", &creature, &outcome.creature);
}

// --- the constant fold ------------------------------------------------------

#[test]
fn a_hidden_neuron_with_no_inward_edge_folds_its_value_into_its_outward_weights() {
    let cut = without_synapse(
        &EDGE_TARGET_BECOMES_CONSTANT.before(),
        "input-0",
        "h-1",
        SynapseType::Standard,
    );
    let outcome = cleaned(&cut);

    let folded = neuron(&outcome.creature, "h-1");
    assert_eq!(folded.neuron_type, "constant", "h-1 is now a constant");
    assert_eq!(
        folded.bias, SUPPORT_CONSTANT_BIAS,
        "a support constant carries bias 1"
    );
    assert!(folded.squash.is_none(), "a constant carries no squash");
    assert_eq!(outcome.folded_neurons, vec!["h-1".to_string()]);

    // The documented value, derived here rather than read from the fold:
    // LOGISTIC(0.4) scaled into the edge that used to read the neuron.
    let expected = 2.0 * logistic(0.4);
    let actual = weight(&outcome.creature, "h-1", "output-0");
    assert!(
        (actual - expected).abs() < 1e-6,
        "folded weight {actual} is not 2.0 * LOGISTIC(0.4) = {expected}"
    );

    // ... and NEAT-AI's own capture of this removal computes the same numbers,
    // even though it carries the value in the constant's bias instead.
    assert_same_function(
        "edge_target_becomes_constant",
        &EDGE_TARGET_BECOMES_CONSTANT.after(),
        &outcome.creature,
    );
}

#[test]
fn a_fold_reuses_an_existing_support_constant_rather_than_adding_one() {
    let cut = without_synapse(
        &CONSTANT_MOVES_INTO_PREFIX.before(),
        "input-0",
        "h-2",
        SynapseType::Standard,
    );
    let outcome = cleaned(&cut);

    assert_eq!(
        constants(&outcome.creature).len(),
        1,
        "the fold minted a constant instead of reusing c-1"
    );
    assert!(
        !has_neuron(&outcome.creature, "h-2"),
        "h-2 folded onto c-1 and should be gone"
    );
    // c-1 was worth 0.5 at weight 0.2, and h-2 folds to LOGISTIC(0.3) at
    // weight 1.0; a bias-1 support constant carries the sum of the two terms.
    let expected = 0.2 * 0.5 + logistic(0.3);
    let actual = weight(&outcome.creature, "c-1", "output-0");
    assert!(
        (actual - expected).abs() < 1e-6,
        "merged support weight {actual} is not {expected}"
    );
    assert_same_function(
        "constant_moves_into_prefix",
        &CONSTANT_MOVES_INTO_PREFIX.after(),
        &outcome.creature,
    );
}

#[test]
fn a_fold_behind_a_surviving_hidden_neuron_moves_into_the_constant_prefix() {
    let cut = without_synapse(
        &creature(FOLD_BEHIND_A_HIDDEN_JSON),
        "input-1",
        "h-2",
        SynapseType::Standard,
    );
    let outcome = cleaned(&cut);

    // `h-2` was listed second; as a constant it has to lead the slice, and the
    // synapses have to be re-sorted into the new index order.
    assert_eq!(neuron(&outcome.creature, "h-2").neuron_type, "constant");
    assert_eq!(
        outcome
            .creature
            .neurons
            .iter()
            .map(|n| n.uuid.as_str())
            .collect::<Vec<_>>(),
        vec!["h-2", "h-1", "output-0"],
        "the folded constant did not move ahead of the surviving hidden neuron"
    );
    assert_same_function("fold_behind_a_hidden", &cut, &outcome.creature);
}

#[test]
fn a_folded_constant_moves_ahead_of_the_hidden_neurons() {
    let cut = without_synapse(
        &EDGE_TARGET_BECOMES_CONSTANT.before(),
        "input-0",
        "h-1",
        SynapseType::Standard,
    );
    let outcome = cleaned(&cut);
    assert_eq!(
        outcome.creature.neurons[0].uuid, "h-1",
        "the folded constant did not move into the constant prefix"
    );
}

#[test]
fn a_fold_reproduces_the_forward_pass_for_every_squash() {
    // `apply_squash` is a *fallback* for the aggregate squashes, not the
    // forward pass, so a fold that trusted it computed the wrong constant for
    // HYPOT and HYPOTv2. The oracle is the network itself: fold a stranded
    // neuron of every squash at several biases and demand the same outputs.
    let mut checked = 0;
    for code in 0u8..=37 {
        let squash = SquashType::from(code);
        let name = neat_core::squash_name_from(squash);
        // The extremes matter: they are where a raw activation leaves the
        // squash's own output range and the forward pass clamps it.
        for bias in [-1.0e6f64, -1.5, -0.5, 0.0, 0.7, 3.0, 1.0e6] {
            let before = stranded_hidden_creature(name, bias);
            let outcome = cleanup_creature(&before)
                .unwrap_or_else(|e| panic!("{name} at bias {bias}: cleanup failed: {e}"));
            assert_same_function(&format!("{name}@{bias}"), &before, &outcome.creature);
            assert_eq!(
                neuron(&outcome.creature, "h-1").neuron_type,
                "constant",
                "{name} at bias {bias}: the stranded neuron did not fold"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 38 * 7, "every squash and bias must be exercised");
}

// --- constant support invariants -------------------------------------------

#[test]
fn a_constant_the_cut_left_unreferenced_is_removed() {
    let cut = without_neuron(&creature(CONSTANT_SUPPORT_JSON), "h-1");
    let outcome = cleaned(&cut);

    assert!(
        !has_neuron(&outcome.creature, "c-1"),
        "an unreferenced constant survived"
    );
    assert!(
        outcome.removed_neurons.contains(&"c-1".to_string()),
        "the removal was not reported: {:?}",
        outcome.removed_neurons
    );
    assert_eq!(
        outcome.creature.neurons.len(),
        1,
        "only the output should remain"
    );
}

#[test]
fn a_constant_another_branch_still_reads_is_kept() {
    let shared = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"constant","uuid":"c-1","bias":1.0},
        {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
        {"weight":-0.25,"fromUUID":"c-1","toUUID":"h-1"},
        {"weight":0.5,"fromUUID":"c-1","toUUID":"output-0"},
        {"weight":1.0,"fromUUID":"h-1","toUUID":"output-0"}
      ]
    }"#,
    );
    let cut = without_neuron(&shared, "h-1");
    let outcome = cleaned(&cut);

    assert!(
        has_neuron(&outcome.creature, "c-1"),
        "a constant the output still reads was removed"
    );
    assert_eq!(weight(&outcome.creature, "c-1", "output-0"), 0.5);
}

#[test]
fn a_surplus_of_constants_is_merged_down_to_the_budget() {
    let before = creature(FIVE_CONSTANTS_JSON);
    let outcome = cleaned(&before);

    assert_eq!(
        constants(&outcome.creature).len(),
        MAX_SUPPORT_CONSTANTS,
        "the surplus was not merged"
    );
    assert!(
        !outcome.merged_constants.is_empty(),
        "the merge was not reported"
    );
    assert_same_function("five_constants", &before, &outcome.creature);
}

#[test]
fn a_constant_carrying_its_value_in_its_bias_is_rescaled_into_its_weights() {
    let before = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"constant","uuid":"c-1","bias":0.25},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"},
        {"weight":2.0,"fromUUID":"c-1","toUUID":"output-0"}
      ]
    }"#,
    );
    let outcome = cleaned(&before);

    assert_eq!(neuron(&outcome.creature, "c-1").bias, SUPPORT_CONSTANT_BIAS);
    assert_eq!(
        weight(&outcome.creature, "c-1", "output-0"),
        0.5,
        "the constant's value did not move into the weight"
    );
    assert_eq!(outcome.rescaled_constants, vec!["c-1".to_string()]);
    assert_same_function("rescale", &before, &outcome.creature);
}

// --- aggregate targets ------------------------------------------------------

#[test]
fn a_mean_target_keeps_one_edge_per_folded_source() {
    let before = without_synapse(
        &creature(AGGREGATE_TARGETS_JSON),
        "input-0",
        "h-a",
        SynapseType::Standard,
    );
    let outcome = cleaned(&before);

    // MEAN divides by its inward count, so the two folded sources must stay
    // two edges — merging them would change what the neuron computes.
    assert_eq!(
        inward(&outcome.creature, "mean-1"),
        3,
        "a MEAN target lost an inward edge to a merge"
    );
    assert_same_function("aggregate_targets", &before, &outcome.creature);
}

#[test]
fn a_minimum_target_merges_two_constant_terms_to_the_smaller_weight() {
    let before = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-a","bias":1.0,"squash":"IDENTITY"},
        {"type":"hidden","uuid":"h-b","bias":0.5,"squash":"IDENTITY"},
        {"type":"hidden","uuid":"min-1","bias":0.0,"squash":"MINIMUM"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"min-1"},
        {"weight":3.0,"fromUUID":"h-a","toUUID":"min-1"},
        {"weight":0.5,"fromUUID":"h-b","toUUID":"min-1"},
        {"weight":1.0,"fromUUID":"min-1","toUUID":"output-0"}
      ]
    }"#,
    );
    // Neither hidden neuron has an inward edge, so each is a constant term at
    // the MINIMUM: `3.0 * IDENTITY(1.0) = 3.0` and `0.5 * IDENTITY(0.5) = 0.25`.
    // MINIMUM takes the smallest term, so the surviving support edge must carry
    // 0.25 — summing the two would put 3.25 in front of every smaller term.
    let outcome = cleaned(&before);
    assert_same_function("minimum_merge", &before, &outcome.creature);
    assert_eq!(
        constants(&outcome.creature).len(),
        1,
        "the two folds should share one support constant"
    );
    assert_eq!(
        weight(&outcome.creature, "h-a", "min-1"),
        0.25,
        "the merge did not keep the smaller of the two constant terms"
    );
}

#[test]
fn roles_are_stripped_and_summed_at_a_target_that_cannot_read_them() {
    // Only an `IF` keeps a sum per role. The same pair in two roles into an
    // IDENTITY target is the same edge written twice, and rule 26b refuses it,
    // so cleanup canonicalises it to one untyped edge of the summed weight.
    let before = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-a","bias":0.1,"squash":"LOGISTIC"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"h-a"},
        {"weight":2.0,"fromUUID":"h-a","toUUID":"output-0","type":"positive"},
        {"weight":-0.5,"fromUUID":"h-a","toUUID":"output-0","type":"negative"}
      ]
    }"#,
    );
    let outcome = cleaned(&before);

    assert_eq!(
        outward(&outcome.creature, "h-a"),
        1,
        "the two readings of one edge were not coalesced"
    );
    assert_eq!(
        weight(&outcome.creature, "h-a", "output-0"),
        1.5,
        "the coalesced weight is not the sum"
    );
    assert!(
        outcome
            .creature
            .synapses
            .iter()
            .all(|s| s.synapse_type.is_none()),
        "a role survived at a target that cannot read it"
    );
    // No activation oracle here: this `before` is exactly the shape
    // `compile_creature` refuses (`TypedDuplicateSynapse`), which is why the
    // canonical form has to exist. The sum is the derivation instead — an
    // IDENTITY target adds both rows, so `2.0 + -0.5` is what one edge must
    // carry.
}

// --- IF repair --------------------------------------------------------------

#[test]
fn an_if_that_loses_a_required_role_is_downgraded_and_its_rows_are_summed() {
    let cut = without_neuron(&IF_REPAIR_COALESCES_ROLES.before(), "h-cond");
    let outcome = cleaned(&cut);

    assert_eq!(
        outcome.creature,
        IF_REPAIR_COALESCES_ROLES.after(),
        "cleanup did not reproduce the TypeScript IF repair"
    );
    assert_eq!(outcome.downgraded_if_neurons, vec!["if-1".to_string()]);
    assert_eq!(
        weight(&outcome.creature, "h-a", "if-1"),
        -1.0,
        "the positive and negative rows were not summed"
    );
}

#[test]
fn an_if_that_still_has_all_three_roles_is_left_alone() {
    let before = EDGE_ROLE_IDENTITY.after();
    let outcome = cleaned(&before);
    assert!(
        outcome.downgraded_if_neurons.is_empty(),
        "a healthy IF was downgraded"
    );
    assert_eq!(
        neuron(&outcome.creature, "if-1").squash.as_deref(),
        Some("IF"),
        "a healthy IF lost its squash"
    );
    // Two conditions, one positive from `h-a`, and a positive and a negative
    // from `h-b` — five inward edges, none of which the repair may touch.
    assert_eq!(
        inward(&outcome.creature, "if-1"),
        5,
        "a healthy IF lost an inward role"
    );
}

// --- the memetic record -----------------------------------------------------

#[test]
fn the_memetic_record_is_pruned_of_the_structure_the_cleanup_removed() {
    let cut = without_neuron(&MEMETIC_DROPPED_ON_REMOVAL.before(), "h-x");
    let outcome = cleaned(&cut);

    let memetic = outcome
        .creature
        .memetic
        .as_ref()
        .expect("the record itself survives — only dangling references go");
    assert!(
        memetic.biases.is_empty(),
        "a bias naming the removed h-x survived: {:?}",
        memetic.biases
    );
    assert_eq!(
        memetic.extra.get("generation").and_then(|v| v.as_i64()),
        Some(3),
        "the fine-tuning history was thrown away with the dangling keys"
    );
    // Rule 31 is what a surviving dangling reference would break.
    creature_validate(&outcome.creature, &OPTIONS).expect("the pruned record validates");
}

// --- the report -------------------------------------------------------------

#[test]
fn cleanup_reports_the_structure_it_removed() {
    let cut = without_neuron(&creature(CASCADE_JSON), "h-x");
    let outcome = cleaned(&cut);

    assert!(outcome.changed);
    assert_eq!(outcome.removed_neurons.len(), 4);
    assert!(
        outcome
            .removed_synapses
            .iter()
            .any(|k| k.from_uuid == "input-0" && k.to_uuid == "h-c"),
        "the feeder edges were not reported: {:?}",
        outcome.removed_synapses
    );
}

#[test]
fn a_canonical_creature_comes_back_untouched() {
    let before = creature(TWO_OUTPUTS_JSON);
    let outcome = cleaned(&before);
    assert!(!outcome.changed, "a canonical creature was rewritten");
    assert_eq!(outcome.creature, before);
    assert_eq!(outcome.passes, 1, "a no-op needs exactly one pass");
}

#[test]
fn the_constant_budget_is_three() {
    // The budget is a documented number, not whatever the constant happens to
    // say: a creature carrying four constants comes back with three.
    assert_eq!(MAX_SUPPORT_CONSTANTS, 3, "the documented constant budget");
    let four = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"constant","uuid":"c-1","bias":1.0},
        {"type":"constant","uuid":"c-2","bias":1.0},
        {"type":"constant","uuid":"c-3","bias":1.0},
        {"type":"constant","uuid":"c-4","bias":1.0},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"},
        {"weight":0.1,"fromUUID":"c-1","toUUID":"output-0"},
        {"weight":0.2,"fromUUID":"c-2","toUUID":"output-0"},
        {"weight":0.3,"fromUUID":"c-3","toUUID":"output-0"},
        {"weight":0.4,"fromUUID":"c-4","toUUID":"output-0"}
      ]
    }"#,
    );
    let outcome = cleaned(&four);
    assert_eq!(
        constants(&outcome.creature).len(),
        3,
        "four constants must canonicalise to three"
    );
    // The merged pair is summed at the IDENTITY output: 0.1 + 0.4.
    assert_eq!(weight(&outcome.creature, "c-1", "output-0"), 0.5);
    assert_same_function("four_constants", &four, &outcome.creature);
}

#[test]
fn a_maximum_target_merges_two_constant_terms_to_the_larger_weight() {
    // The mirror of the MINIMUM rule: MAXIMUM takes the largest term, and two
    // terms from one bias-1 constant are just their weights.
    let before = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-a","bias":1.0,"squash":"IDENTITY"},
        {"type":"hidden","uuid":"h-b","bias":0.5,"squash":"IDENTITY"},
        {"type":"hidden","uuid":"max-1","bias":0.0,"squash":"MAXIMUM"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"max-1"},
        {"weight":0.25,"fromUUID":"h-a","toUUID":"max-1"},
        {"weight":3.0,"fromUUID":"h-b","toUUID":"max-1"},
        {"weight":1.0,"fromUUID":"max-1","toUUID":"output-0"}
      ]
    }"#,
    );
    let outcome = cleaned(&before);

    // Terms: `0.25 * IDENTITY(1.0) = 0.25` and `3.0 * IDENTITY(0.5) = 1.5`.
    assert_eq!(
        weight(&outcome.creature, "h-a", "max-1"),
        1.5,
        "the merge did not keep the larger of the two constant terms"
    );
    assert_same_function("maximum_merge", &before, &outcome.creature);
}

#[test]
fn a_hypotenuse_target_keeps_one_edge_per_folded_source() {
    // HYPOT squares each term, so `hypot(a, b) != a + b` and the two folded
    // sources must stay two edges.
    let before = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-a","bias":0.6,"squash":"IDENTITY"},
        {"type":"hidden","uuid":"h-b","bias":0.8,"squash":"IDENTITY"},
        {"type":"hidden","uuid":"hyp-1","bias":0.0,"squash":"HYPOT"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"hyp-1"},
        {"weight":1.0,"fromUUID":"h-a","toUUID":"hyp-1"},
        {"weight":1.0,"fromUUID":"h-b","toUUID":"hyp-1"},
        {"weight":1.0,"fromUUID":"hyp-1","toUUID":"output-0"}
      ]
    }"#,
    );
    let outcome = cleaned(&before);

    assert_eq!(
        inward(&outcome.creature, "hyp-1"),
        3,
        "a HYPOT target lost an inward edge to a merge"
    );
    assert_eq!(
        constants(&outcome.creature).len(),
        2,
        "the two folds must keep their own support constants"
    );
    assert_same_function("hypotenuse_targets", &before, &outcome.creature);
}

#[test]
fn two_roles_into_an_aggregate_target_are_never_folded_together() {
    // The role is only readable at an `IF`; at a MEAN both edges are the same
    // key, and merging them would change the divisor. So the second fold must
    // not land on the first fold's support constant.
    let before = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-a","bias":0.3,"squash":"IDENTITY"},
        {"type":"hidden","uuid":"h-b","bias":0.7,"squash":"IDENTITY"},
        {"type":"hidden","uuid":"mean-1","bias":0.0,"squash":"MEAN"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"mean-1"},
        {"weight":2.0,"fromUUID":"h-a","toUUID":"mean-1","type":"positive"},
        {"weight":4.0,"fromUUID":"h-b","toUUID":"mean-1","type":"negative"}
      ]
    }"#,
    );
    let mut wired = before.clone();
    wired.synapses.push(neat_core::SynapseExport {
        from_uuid: "mean-1".to_string(),
        to_uuid: "output-0".to_string(),
        weight: 1.0,
        synapse_type: None,
    });
    let outcome = cleaned(&wired);

    assert_eq!(
        inward(&outcome.creature, "mean-1"),
        3,
        "the two roles were folded onto one constant and the divisor moved"
    );
    assert_eq!(constants(&outcome.creature).len(), 2);
    assert_same_function("two_roles_into_mean", &wired, &outcome.creature);
}

#[test]
fn a_fold_will_not_reuse_a_constant_the_target_cannot_tell_it_apart_from() {
    // `c-1` already feeds the MEAN under `positive` and the stranded `h-b`
    // under `negative`. The MEAN cannot read either role, so both edges are the
    // same key there — reusing `c-1` would collapse two terms into one and move
    // the divisor. The fold must mint its own support node instead.
    let before = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"constant","uuid":"c-1","bias":1.0},
        {"type":"hidden","uuid":"h-b","bias":0.7,"squash":"IDENTITY"},
        {"type":"hidden","uuid":"mean-1","bias":0.0,"squash":"MEAN"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"mean-1"},
        {"weight":2.0,"fromUUID":"c-1","toUUID":"mean-1","type":"positive"},
        {"weight":4.0,"fromUUID":"h-b","toUUID":"mean-1","type":"negative"},
        {"weight":1.0,"fromUUID":"mean-1","toUUID":"output-0"}
      ]
    }"#,
    );
    let outcome = cleaned(&before);

    assert_eq!(
        constants(&outcome.creature).len(),
        2,
        "the fold reused a constant the MEAN cannot tell apart from it"
    );
    assert_eq!(
        inward(&outcome.creature, "mean-1"),
        3,
        "the MEAN divisor moved"
    );
    assert_same_function("reuse_blocked_at_mean", &before, &outcome.creature);
}

#[test]
fn a_surviving_if_comes_back_with_its_roles_in_canonical_order() {
    // The role is the third leg of the `(from, to, role)` key (Issue #577), so
    // `h-b`'s two edges must come back negative (2) before positive (3) even
    // though the creature lists them the other way around.
    let mut shuffled = EDGE_ROLE_IDENTITY.after();
    let positive = shuffled
        .synapses
        .iter()
        .position(|s| s.from_uuid == "h-b" && s.synapse_type.as_deref() == Some("positive"))
        .expect("the positive row");
    let negative = shuffled
        .synapses
        .iter()
        .position(|s| s.from_uuid == "h-b" && s.synapse_type.as_deref() == Some("negative"))
        .expect("the negative row");
    shuffled.synapses.swap(positive, negative);

    let outcome = cleaned(&shuffled);
    let roles: Vec<Option<&str>> = outcome
        .creature
        .synapses
        .iter()
        .filter(|s| s.from_uuid == "h-b")
        .map(|s| s.synapse_type.as_deref())
        .collect();
    assert_eq!(
        roles,
        vec![Some("negative"), Some("positive")],
        "the role leg of the canonical sort did not run"
    );
}

// --- failing closed ---------------------------------------------------------

#[test]
fn a_synapse_naming_a_neuron_that_does_not_exist_fails_loudly() {
    let mut broken = creature(CASCADE_JSON);
    broken.synapses[0].from_uuid = "no-such-neuron".to_string();
    match cleanup_creature(&broken) {
        Err(CleanupError::UnknownEndpoint { uuid }) => assert_eq!(uuid, "no-such-neuron"),
        other => panic!("expected an unknown endpoint, got {other:?}"),
    }
}

#[test]
fn a_non_finite_bias_is_never_folded() {
    let mut broken = creature(CASCADE_JSON);
    for n in &mut broken.neurons {
        if n.uuid == "h-c" {
            n.bias = f64::NAN;
        }
    }
    match cleanup_creature(&broken) {
        Err(CleanupError::NonFiniteBias { uuid }) => assert_eq!(uuid, "h-c"),
        other => panic!("expected a non-finite bias, got {other:?}"),
    }
}

#[test]
fn a_constant_with_an_inward_edge_is_refused_rather_than_repaired() {
    let mut broken = creature(CONSTANT_SUPPORT_JSON);
    broken.synapses.push(neat_core::SynapseExport {
        from_uuid: "input-0".to_string(),
        to_uuid: "c-1".to_string(),
        weight: 1.0,
        synapse_type: None,
    });
    match cleanup_creature(&broken) {
        Err(CleanupError::ConstantHasInward { uuid }) => assert_eq!(uuid, "c-1"),
        other => panic!("expected a constant with an inward edge, got {other:?}"),
    }
}

#[test]
fn a_creature_cleanup_cannot_make_valid_is_reported_not_returned() {
    // A backward edge in a `forwardOnly` creature is not wreckage a removal
    // left behind — there is no exact repair, so nothing comes back.
    let backward = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"h-2","bias":0.2,"squash":"LOGISTIC"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"h-2"},
        {"weight":1.0,"fromUUID":"h-2","toUUID":"h-1"},
        {"weight":1.0,"fromUUID":"h-1","toUUID":"h-2"},
        {"weight":1.0,"fromUUID":"h-2","toUUID":"output-0"}
      ]
    }"#,
    );
    match cleanup_creature(&backward) {
        Err(CleanupError::Invalid(failure)) => {
            // `h-1 -> h-2` reads an activation `h-2` has not computed yet, so
            // rule 27 is the rule that must name it — not merely "something".
            assert_eq!(
                failure.reason, "RECURSIVE_SYNAPSE",
                "the failure must name the rule the creature broke"
            );
        }
        other => panic!("expected an invalid result, got {other:?}"),
    }
}

#[test]
fn a_synapse_pointing_at_an_observation_neuron_is_refused() {
    let mut broken = creature(CASCADE_JSON);
    broken.synapses[0].to_uuid = "input-1".to_string();
    match cleanup_creature(&broken) {
        Err(CleanupError::SynapseTargetsInput { uuid }) => assert_eq!(uuid, "input-1"),
        other => panic!("expected a synapse targeting an input, got {other:?}"),
    }
}

#[test]
fn two_neurons_sharing_a_uuid_are_refused() {
    let mut broken = creature(CASCADE_JSON);
    broken.neurons[1].uuid = "h-c".to_string();
    match cleanup_creature(&broken) {
        Err(CleanupError::DuplicateUuid { uuid }) => assert_eq!(uuid, "h-c"),
        other => panic!("expected a duplicate UUID, got {other:?}"),
    }
}

#[test]
fn a_neuron_of_an_unknown_type_is_refused() {
    let mut broken = creature(CASCADE_JSON);
    broken.neurons[0].neuron_type = "gate".to_string();
    match cleanup_creature(&broken) {
        Err(CleanupError::UnknownNeuronType { uuid, declared }) => {
            assert_eq!(uuid, "h-c");
            assert_eq!(declared, "gate");
        }
        other => panic!("expected an unknown neuron type, got {other:?}"),
    }
}

#[test]
fn a_non_finite_weight_is_refused() {
    let mut broken = creature(CASCADE_JSON);
    broken.synapses[0].weight = f64::INFINITY;
    match cleanup_creature(&broken) {
        Err(CleanupError::NonFiniteWeight {
            from_uuid,
            to_uuid,
            weight,
        }) => {
            assert_eq!(from_uuid, "input-0");
            assert_eq!(to_uuid, "h-c");
            assert!(weight.is_infinite());
        }
        other => panic!("expected a non-finite weight, got {other:?}"),
    }
}

#[test]
fn a_merge_that_could_not_be_exact_is_refused_rather_than_guessed() {
    // Two roles from one *hidden* source into a MINIMUM target: the target
    // cannot read the roles, so they are the same edge written twice — but a
    // hidden source's term is not its weight, so no single edge carries the
    // same value. Cleanup refuses instead of picking one.
    let broken = creature(
        r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-a","bias":0.1,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"min-1","bias":0.0,"squash":"MINIMUM"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"h-a"},
        {"weight":1.0,"fromUUID":"input-0","toUUID":"min-1"},
        {"weight":2.0,"fromUUID":"h-a","toUUID":"min-1","type":"positive"},
        {"weight":0.5,"fromUUID":"h-a","toUUID":"min-1","type":"negative"},
        {"weight":1.0,"fromUUID":"min-1","toUUID":"output-0"}
      ]
    }"#,
    );
    match cleanup_creature(&broken) {
        Err(CleanupError::InexactMerge {
            from_uuid,
            to_uuid,
            squash,
        }) => {
            assert_eq!(from_uuid, "h-a");
            assert_eq!(to_uuid, "min-1");
            assert_eq!(squash, "MINIMUM");
        }
        other => panic!("expected an inexact merge, got {other:?}"),
    }
}
