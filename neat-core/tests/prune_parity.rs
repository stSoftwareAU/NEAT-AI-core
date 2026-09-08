//! The captured TypeScript pruning behaviour, checked rule by rule (Issue #588).
//!
//! [`neat_core::PRUNE_PARITY_CASES`] holds `(before, request, after)` triples
//! where `after` is the creature NEAT-AI's TypeScript operators actually
//! produced. This suite is what stops those captures drifting: every shared
//! rule the fleet's pruning semantics rest on is asserted against the pair, and
//! the case-specific tests below derive the numbers from the documented formula
//! rather than restating the fixture.
//!
//! The oracle here is deliberately *not* a pruning implementation. It is the
//! TypeScript output plus the rules that output obeys, so this suite keeps its
//! value independently of the shared helpers. Those helpers now exist and are
//! graded by the same fixtures with
//! `prune(case.before(), case.request) == case.after()`:
//! `neat-core/tests/prune_neuron.rs` (Issue #590) for the `RemoveNeuron`
//! captures, `neat-core/tests/prune_synapse.rs` (Issue #591) for the
//! `RemoveSynapse` ones.

use neat_core::{
    CreatureExport, PRUNE_PARITY_CASES, PruneCase, PruneRequest, SquashType, SynapseType,
    ValidateOptions, apply_squash, compile_creature, creature_to_json, creature_validate,
    parse_creature_json, parse_synapse_type, validate_creature_topology,
    validate_no_duplicate_synapses,
};

const OPTIONS: ValidateOptions = ValidateOptions {
    neurons: None,
    connections: None,
    feedback_loop: None,
    forward_only: true,
};

/// `f32` activation slack — the fixtures carry `f64` biases and the compiled
/// network computes in `f32`, so an exact comparison would fail on rounding
/// alone.
const ACTIVATION_TOL: f32 = 1e-6;

/// The role a synapse plays at its target, with the untyped role resolved.
fn role_of(synapse: &neat_core::SynapseExport) -> SynapseType {
    parse_synapse_type(synapse.synapse_type.as_deref())
}

/// Every `(from, to, role)` triple with its weight.
fn triples(creature: &CreatureExport) -> Vec<(String, String, SynapseType, f64)> {
    creature
        .synapses
        .iter()
        .map(|s| (s.from_uuid.clone(), s.to_uuid.clone(), role_of(s), s.weight))
        .collect()
}

/// Wire index of every neuron: inputs first (`input-N`), then the exported
/// list in order — the same assignment NEAT-AI's loader makes.
fn wire_indices(creature: &CreatureExport) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = (0..creature.input)
        .map(|i| (format!("input-{i}"), i))
        .collect();
    for (j, neuron) in creature.neurons.iter().enumerate() {
        out.push((neuron.uuid.clone(), creature.input + j));
    }
    out
}

fn index_of(creature: &CreatureExport, uuid: &str) -> usize {
    wire_indices(creature)
        .into_iter()
        .find(|(u, _)| u == uuid)
        .unwrap_or_else(|| panic!("no neuron {uuid}"))
        .1
}

fn neuron<'a>(creature: &'a CreatureExport, uuid: &str) -> &'a neat_core::NeuronExport {
    creature
        .neurons
        .iter()
        .find(|n| n.uuid == uuid)
        .unwrap_or_else(|| panic!("no neuron {uuid} in the creature"))
}

fn has_neuron(creature: &CreatureExport, uuid: &str) -> bool {
    creature.neurons.iter().any(|n| n.uuid == uuid)
}

fn outward(creature: &CreatureExport, uuid: &str) -> usize {
    creature
        .synapses
        .iter()
        .filter(|s| s.from_uuid == uuid)
        .count()
}

fn inward(creature: &CreatureExport, uuid: &str) -> usize {
    creature
        .synapses
        .iter()
        .filter(|s| s.to_uuid == uuid)
        .count()
}

/// The documented logistic, in `f64` — an independent route to the bias a
/// hidden→constant flip folds, derived from the formula rather than read back
/// out of the kernel under test.
fn logistic(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

fn probe_inputs(width: usize) -> Vec<Vec<f32>> {
    let seeds: [f32; 5] = [-1.5, -0.25, 0.0, 0.75, 2.0];
    seeds
        .iter()
        .map(|s| (0..width).map(|i| s + i as f32 * 0.125).collect())
        .collect()
}

fn outputs(creature: &CreatureExport, inputs: &[f32]) -> Vec<f32> {
    let mut net = compile_creature(creature).expect("fixture compiles");
    net.activate(inputs, creature.output)
}

// --- Rules every captured case obeys ----------------------------------------

#[test]
fn every_captured_pair_is_a_creature_the_shared_validator_accepts() {
    assert!(!PRUNE_PARITY_CASES.is_empty(), "no parity cases captured");
    for case in PRUNE_PARITY_CASES {
        for (half, creature) in [("before", case.before()), ("after", case.after())] {
            creature_validate(&creature, &OPTIONS)
                .unwrap_or_else(|e| panic!("{} {half}: {} — {}", case.name, e.reason, e.message));
            validate_creature_topology(&creature)
                .unwrap_or_else(|e| panic!("{} {half}: {e:?}", case.name));
            validate_no_duplicate_synapses(&creature)
                .unwrap_or_else(|e| panic!("{} {half}: {e:?}", case.name));
            compile_creature(&creature)
                .unwrap_or_else(|e| panic!("{} {half} does not compile: {e:?}", case.name));
        }
    }
}

#[test]
fn every_captured_creature_round_trips_through_its_json() {
    for case in PRUNE_PARITY_CASES {
        for (half, creature) in [("before", case.before()), ("after", case.after())] {
            let json = creature_to_json(&creature).expect("serialise");
            let reparsed = parse_creature_json(&json).expect("re-parse");
            assert_eq!(
                reparsed, creature,
                "{} {half} does not round trip",
                case.name
            );
        }
    }
}

#[test]
fn the_requested_structure_is_present_before_and_gone_after() {
    for case in PRUNE_PARITY_CASES {
        let before = case.before();
        let after = case.after();
        match case.request {
            PruneRequest::RemoveNeuron { uuid } => {
                assert!(
                    has_neuron(&before, uuid),
                    "{}: asked to remove {uuid}, which the before creature does not carry",
                    case.name
                );
                assert!(
                    !has_neuron(&after, uuid),
                    "{}: {uuid} survived its own removal",
                    case.name
                );
                assert!(
                    !after
                        .synapses
                        .iter()
                        .any(|s| s.from_uuid == uuid || s.to_uuid == uuid),
                    "{}: a synapse still names the removed {uuid}",
                    case.name
                );
            }
            PruneRequest::RemoveSynapse {
                from_uuid,
                to_uuid,
                role,
            } => {
                let wanted = |t: &(String, String, SynapseType, f64)| {
                    t.0 == from_uuid && t.1 == to_uuid && t.2 == role
                };
                assert!(
                    triples(&before).iter().any(wanted),
                    "{}: asked to remove {from_uuid} -> {to_uuid} ({role:?}), which is not there",
                    case.name
                );
                assert!(
                    !triples(&after).iter().any(wanted),
                    "{}: {from_uuid} -> {to_uuid} ({role:?}) survived its own removal",
                    case.name
                );
            }
        }
    }
}

#[test]
fn the_other_roles_of_the_requested_pair_are_left_alone() {
    let mut pairs_checked = 0;
    for case in PRUNE_PARITY_CASES {
        let PruneRequest::RemoveSynapse {
            from_uuid,
            to_uuid,
            role,
        } = case.request
        else {
            continue;
        };
        let before = case.before();
        let after = case.after();
        // A removal that also removes an endpoint takes the whole pair with it;
        // the rule is about the roles that *can* survive.
        if !(has_neuron(&after, to_uuid)
            && (from_uuid.starts_with("input-") || has_neuron(&after, from_uuid)))
        {
            continue;
        }
        for (f, t, r, w) in triples(&before) {
            if f != from_uuid || t != to_uuid || r == role {
                continue;
            }
            pairs_checked += 1;
            assert!(
                triples(&after)
                    .iter()
                    .any(|(af, at, ar, aw)| *af == f && *at == t && *ar == r && *aw == w),
                "{}: removing the {role:?} role also lost {f} -> {t} ({r:?}, weight {w})",
                case.name
            );
        }
    }
    assert!(
        pairs_checked > 0,
        "no captured case carries a second role on the requested pair — the rule is untested"
    );
}

#[test]
fn no_orphan_neuron_survives_a_rewrite() {
    for case in PRUNE_PARITY_CASES {
        let after = case.after();
        for n in &after.neurons {
            match n.neuron_type.as_str() {
                "hidden" => {
                    assert!(
                        outward(&after, &n.uuid) > 0,
                        "{}: hidden {} has nothing to feed",
                        case.name,
                        n.uuid
                    );
                    assert!(
                        inward(&after, &n.uuid) > 0,
                        "{}: hidden {} has no inward edge and was not turned into a constant",
                        case.name,
                        n.uuid
                    );
                }
                "constant" => {
                    assert!(
                        outward(&after, &n.uuid) > 0,
                        "{}: constant {} has nothing to feed",
                        case.name,
                        n.uuid
                    );
                    assert_eq!(
                        inward(&after, &n.uuid),
                        0,
                        "{}: constant {} carries an inward edge",
                        case.name,
                        n.uuid
                    );
                }
                _ => {}
            }
        }
    }
}

#[test]
fn the_computational_slice_stays_constants_then_hiddens_then_outputs() {
    for case in PRUNE_PARITY_CASES {
        let after = case.after();
        let mut seen_hidden = false;
        let mut seen_output = false;
        for n in &after.neurons {
            match n.neuron_type.as_str() {
                "constant" => {
                    assert!(
                        !seen_hidden && !seen_output,
                        "{}: constant {} sits after a hidden or output neuron",
                        case.name,
                        n.uuid
                    );
                }
                "hidden" => {
                    assert!(
                        !seen_output,
                        "{}: hidden {} sits after an output neuron",
                        case.name, n.uuid
                    );
                    seen_hidden = true;
                }
                "output" => seen_output = true,
                other => panic!("{}: unexpected neuron type {other}", case.name),
            }
        }
        assert_eq!(
            after
                .neurons
                .iter()
                .filter(|n| n.neuron_type == "output")
                .count(),
            after.output,
            "{}: output count does not match the declared width",
            case.name
        );
    }
}

#[test]
fn synapses_come_back_in_canonical_from_to_role_order() {
    for case in PRUNE_PARITY_CASES {
        let after = case.after();
        let keys: Vec<(usize, usize, u8)> = after
            .synapses
            .iter()
            .map(|s| {
                (
                    index_of(&after, &s.from_uuid),
                    index_of(&after, &s.to_uuid),
                    role_of(s) as u8,
                )
            })
            .collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(
            keys, sorted,
            "{}: synapses are not sorted by (from, to, role) after the rewrite",
            case.name
        );
    }
}

#[test]
fn every_surviving_if_neuron_still_carries_its_three_roles() {
    let mut if_neurons_checked = 0;
    for case in PRUNE_PARITY_CASES {
        let after = case.after();
        for n in after
            .neurons
            .iter()
            .filter(|n| n.squash.as_deref() == Some("IF"))
        {
            if_neurons_checked += 1;
            let roles: Vec<SynapseType> = after
                .synapses
                .iter()
                .filter(|s| s.to_uuid == n.uuid)
                .map(role_of)
                .collect();
            for required in [
                SynapseType::Condition,
                SynapseType::Positive,
                SynapseType::Negative,
            ] {
                assert!(
                    roles.contains(&required),
                    "{}: IF neuron {} lost its {required:?} role and was not repaired",
                    case.name,
                    n.uuid
                );
            }
        }
    }
    assert!(
        if_neurons_checked > 0,
        "no captured case keeps an IF neuron — the rule is untested"
    );
}

#[test]
fn a_rewrite_sheds_the_content_derived_memetic_record() {
    for case in PRUNE_PARITY_CASES {
        assert!(
            case.after().memetic.is_none(),
            "{}: the memetic record survived a structural change",
            case.name
        );
    }
    assert!(
        PRUNE_PARITY_CASES
            .iter()
            .any(|c| c.before().memetic.is_some()),
        "no captured case starts with a memetic record — the rule is untested"
    );
}

#[test]
fn every_case_names_the_typescript_behaviour_it_captures() {
    let mut names: Vec<&str> = Vec::new();
    for case in PRUNE_PARITY_CASES {
        for (field, value) in [
            ("name", case.name),
            ("rule", case.rule),
            ("ts_source", case.ts_source),
            ("ts_test", case.ts_test),
        ] {
            assert!(
                !value.trim().is_empty(),
                "{}: {field} is empty — a capture with no provenance cannot be graded",
                case.name
            );
        }
        assert!(
            case.ts_source.contains(".ts"),
            "{}: ts_source does not name a TypeScript file",
            case.name
        );
        assert!(
            case.ts_test.contains(".ts"),
            "{}: ts_test does not name a TypeScript test",
            case.name
        );
        assert!(
            !names.contains(&case.name),
            "duplicate case name {}",
            case.name
        );
        names.push(case.name);
    }
}

#[test]
fn the_memetic_capture_differs_from_the_cascade_capture_only_by_the_record() {
    let cascade = case("cascade_orphan_feeders");
    let memetic = case("memetic_dropped_on_removal");
    assert_eq!(
        cascade.request, memetic.request,
        "the two captures must isolate the memetic record, not a second variable"
    );

    let mut stripped = memetic.before();
    stripped.memetic = None;
    assert_eq!(
        stripped,
        cascade.before(),
        "the memetic capture's topology has drifted from the cascade capture it reuses"
    );
    assert_eq!(memetic.after(), cascade.after());
}

// --- The rules each case was captured for -----------------------------------

fn case(name: &str) -> &'static PruneCase {
    PRUNE_PARITY_CASES
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no parity case named {name}"))
}

#[test]
fn removing_a_neuron_cascades_through_every_orphaned_feeder() {
    let case = case("cascade_orphan_feeders");
    let after = case.after();
    for orphaned in ["h-a", "h-b", "h-c", "h-d"] {
        assert!(
            !has_neuron(&after, orphaned),
            "{orphaned} fed only the removed neuron's branch and should have cascaded away"
        );
    }
    assert_eq!(after.neurons.len(), 1, "only the output should be left");
    assert_eq!(triples(&after).len(), 1);
    assert_eq!(
        triples(&after)[0],
        (
            "input-0".to_string(),
            "output-0".to_string(),
            SynapseType::Standard,
            0.5
        ),
        "the direct path that kept the output fed must survive untouched"
    );
}

#[test]
fn a_target_that_loses_its_last_inward_edge_folds_its_squash_into_a_constant() {
    for (name, uuid, old_bias) in [
        ("edge_target_becomes_constant", "h-1", 0.4_f64),
        ("constant_moves_into_prefix", "h-2", 0.3_f64),
    ] {
        let case = case(name);
        let before = neuron(&case.before(), uuid).clone();
        assert_eq!(before.neuron_type, "hidden");
        assert_eq!(before.squash.as_deref(), Some("LOGISTIC"));
        assert_eq!(before.bias, old_bias);

        let after = neuron(&case.after(), uuid).clone();
        assert_eq!(after.neuron_type, "constant", "{name}: not converted");
        assert_eq!(
            after.squash, None,
            "{name}: a constant carries no activation function"
        );
        // Independent oracle: the documented logistic in f64 …
        assert!(
            (after.bias - logistic(old_bias)).abs() < 1e-15,
            "{name}: folded bias {} is not LOGISTIC({old_bias})",
            after.bias
        );
        // … cross-checked against this crate's own kernel, at f32 precision.
        assert!(
            (after.bias as f32 - apply_squash(SquashType::Logistic, old_bias as f32)).abs() < 1e-7,
            "{name}: folded bias disagrees with apply_squash"
        );
    }
}

#[test]
fn a_source_left_with_nothing_to_feed_is_removed() {
    let case = case("edge_source_becomes_dead");
    let after = case.after();
    assert!(
        !has_neuron(&after, "h-1"),
        "h-1's only outward edge was removed, so h-1 must go"
    );
    assert!(
        has_neuron(&after, "h-2"),
        "h-2 keeps its other inward edge and must stay"
    );
    assert_eq!(
        neuron(&after, "h-2").neuron_type,
        "hidden",
        "h-2 still has an inward edge, so it is not converted"
    );
    assert_eq!(
        triples(&after),
        vec![
            (
                "input-1".to_string(),
                "h-2".to_string(),
                SynapseType::Standard,
                0.25
            ),
            (
                "h-2".to_string(),
                "output-0".to_string(),
                SynapseType::Standard,
                1.0
            ),
        ]
    );
}

#[test]
fn removing_one_role_keeps_the_other_role_of_the_same_pair() {
    let case = case("edge_role_identity");
    let before = case.before();
    let after = case.after();
    let pair = |c: &CreatureExport| -> Vec<(SynapseType, f64)> {
        triples(c)
            .into_iter()
            .filter(|(f, t, _, _)| f == "h-a" && t == "if-1")
            .map(|(_, _, r, w)| (r, w))
            .collect()
    };
    assert_eq!(
        pair(&before),
        vec![(SynapseType::Negative, -1.0), (SynapseType::Positive, 2.0)],
        "the pair must start with both roles"
    );
    assert_eq!(
        pair(&after),
        vec![(SynapseType::Positive, 2.0)],
        "only the requested negative role may go, at its own weight"
    );
    assert_eq!(
        neuron(&after, "if-1").squash.as_deref(),
        Some("IF"),
        "the IF keeps all three roles, so no repair is triggered"
    );
}

#[test]
fn an_if_that_loses_a_role_is_downgraded_and_its_rows_are_summed() {
    let case = case("if_repair_coalesces_roles");
    let before = case.before();
    let after = case.after();

    let coalesced: f64 = triples(&before)
        .iter()
        .filter(|(f, t, _, _)| f == "h-a" && t == "if-1")
        .map(|(_, _, _, w)| *w)
        .sum();
    assert_eq!(coalesced, -1.0, "the two role weights sum to -1.0");

    assert_eq!(
        neuron(&after, "if-1").squash.as_deref(),
        Some("IDENTITY"),
        "an IF that cannot keep its three roles is downgraded"
    );
    let into_if: Vec<(SynapseType, f64)> = triples(&after)
        .into_iter()
        .filter(|(_, t, _, _)| t == "if-1")
        .map(|(_, _, r, w)| (r, w))
        .collect();
    assert_eq!(
        into_if,
        vec![(SynapseType::Standard, coalesced)],
        "the roles are stripped and the two rows become one of the summed weight"
    );
}

#[test]
fn a_converted_constant_moves_ahead_of_the_hidden_neurons() {
    let case = case("constant_moves_into_prefix");
    let order =
        |c: &CreatureExport| -> Vec<String> { c.neurons.iter().map(|n| n.uuid.clone()).collect() };
    assert_eq!(
        order(&case.before()),
        vec!["c-1", "h-1", "h-2"]
            .into_iter()
            .chain(["output-0"])
            .map(String::from)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        order(&case.after()),
        ["c-1", "h-2", "h-1", "output-0"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>(),
        "the converted constant joins the constant prefix, ahead of the surviving hidden"
    );
}

#[test]
fn a_memetic_record_naming_removed_structure_is_dropped_whole() {
    let case = case("memetic_dropped_on_removal");
    let before = case.before();
    let memetic = before
        .memetic
        .as_ref()
        .expect("the case starts with a memetic record");
    assert!(
        memetic.biases.contains_key("h-x"),
        "the record must name structure the removal deletes"
    );
    assert!(
        case.after().memetic.is_none(),
        "TypeScript drops the record wholesale rather than pruning it"
    );
}

#[test]
fn the_constant_bias_fold_leaves_the_creature_scoring_identically() {
    let case = case("constant_bias_fold");
    let before = case.before();
    let after = case.after();
    let mean = case
        .mean_activation
        .expect("the compensation case carries the caller's statistic");
    assert!(case.output_preserving);

    // The documented fold: bias += Σ w · meanActivation over the removed
    // neuron's outgoing edges into that target.
    let folded: f64 = triples(&before)
        .iter()
        .filter(|(f, t, _, _)| f == "c-1" && t == "output-0")
        .map(|(_, _, _, w)| w * mean)
        .sum();
    assert_eq!(folded, 0.1);
    assert!(
        (neuron(&after, "output-0").bias - (neuron(&before, "output-0").bias + folded)).abs()
            < 1e-15,
        "the target's bias does not carry the fold"
    );

    for inputs in probe_inputs(before.input) {
        let lhs = outputs(&before, &inputs);
        let rhs = outputs(&after, &inputs);
        assert_eq!(lhs.len(), rhs.len());
        for (a, b) in lhs.iter().zip(rhs.iter()) {
            assert!(
                (a - b).abs() < ACTIVATION_TOL,
                "removing a constant with its mean folded in changed the output: {a} vs {b} on {inputs:?}"
            );
        }
    }
}
