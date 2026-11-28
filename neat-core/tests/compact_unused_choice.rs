//! Neuron choice for `compactUnused`, ported from NEAT-AI `src/compact/CompactUnused.ts`
//! lines 39–117 (Issue #778).
//!
//! The TypeScript rule under test: a neuron's effect is
//! `|max - min| * min(scale, 1) - plank * in_degree`, where `scale` is the largest
//! `|weight|` on its outward synapses and `in_degree` counts the synapses that
//! end at it. The visit runs from the last entry of the shuffled order to the
//! first, the smallest effect wins under a strict `<`, and a negative effect ends
//! the visit. Every expected value below is derived in a comment beside its
//! assertion. The shuffle and the trace collection belong to the caller, so each
//! test passes the visit order in explicitly.

use neat_core::{
    CreatureExport, NeuronExport, NeuronTraceStats, SynapseExport, UnusedChoiceError,
    UnusedNeuronCandidate, UnusedNeuronChoice, VisitOrderFault, choose_unused_neuron,
    remove_neuron,
};

fn neuron(kind: &str, uuid: &str) -> NeuronExport {
    NeuronExport {
        id: None,
        neuron_type: kind.to_string(),
        uuid: uuid.to_string(),
        bias: 0.0,
        squash: Some("IDENTITY".to_string()),
    }
}

fn synapse(from: &str, to: &str, weight: f64) -> SynapseExport {
    SynapseExport {
        from_uuid: from.to_string(),
        to_uuid: to.to_string(),
        weight,
        synapse_type: None,
    }
}

fn creature(
    input: usize,
    neurons: Vec<NeuronExport>,
    synapses: Vec<SynapseExport>,
) -> CreatureExport {
    CreatureExport {
        input,
        output: 1,
        neurons,
        synapses,
        semantic_version: None,
        forward_only: true,
        memetic: None,
    }
}

/// A trace with the given count and range and no total activation.
fn trace(count: u64, maximum_activation: f64, minimum_activation: f64) -> NeuronTraceStats {
    NeuronTraceStats {
        count,
        maximum_activation,
        minimum_activation,
        total_activation: None,
    }
}

/// An eligible candidate: traced, with no newborn grace left.
fn eligible(stats: NeuronTraceStats) -> UnusedNeuronCandidate {
    UnusedNeuronCandidate {
        trace: Some(stats),
        newborn_grace_remaining: 0,
    }
}

/// A candidate with no trace, which is never chosen.
fn untraced() -> UnusedNeuronCandidate {
    UnusedNeuronCandidate {
        trace: None,
        newborn_grace_remaining: 0,
    }
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-15,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn picks_the_neuron_with_the_smallest_effect() {
    // a: |1.0 - 0.0| * min(0.5, 1) - 0 * 1 = 0.5
    // b: |0.25 - 0.0| * min(1.0, 1) - 0 * 1 = 0.25, the smaller effect.
    // b is one observation, so its activation is the midpoint (0.25 + 0.0) / 2 = 0.125.
    let traced = creature(
        2,
        vec![
            neuron("hidden", "a"),
            neuron("hidden", "b"),
            neuron("output", "output-0"),
        ],
        vec![
            synapse("input-0", "a", 1.0),
            synapse("input-1", "b", 1.0),
            synapse("a", "output-0", 0.5),
            synapse("b", "output-0", 1.0),
        ],
    );
    let candidates = [
        eligible(trace(1, 1.0, 0.0)),
        eligible(trace(1, 0.25, 0.0)),
        untraced(),
    ];

    let choice = choose_unused_neuron(&traced, &candidates, &[0, 1, 2], 0.0)
        .unwrap()
        .unwrap();

    assert_eq!(
        choice,
        UnusedNeuronChoice {
            uuid: "b".to_string(),
            effect: 0.25,
            activation: 0.125,
        }
    );
}

#[test]
fn the_plank_term_flips_the_choice_through_in_degree() {
    // p: in-degree 1, outward |weight| 0.5, range 1.0.
    // q: in-degree 3, outward |weight| 0.75, range 1.0.
    let traced = creature(
        3,
        vec![
            neuron("hidden", "p"),
            neuron("hidden", "q"),
            neuron("output", "output-0"),
        ],
        vec![
            synapse("input-0", "p", 1.0),
            synapse("input-0", "q", 1.0),
            synapse("input-1", "q", 1.0),
            synapse("input-2", "q", 1.0),
            synapse("p", "output-0", 0.5),
            synapse("q", "output-0", 0.75),
        ],
    );
    let candidates = [
        eligible(trace(1, 1.0, 0.0)),
        eligible(trace(1, 1.0, 0.0)),
        untraced(),
    ];

    // Plank 0: p = 1.0 * 0.5 - 0 = 0.5, q = 1.0 * 0.75 - 0 = 0.75, so p is chosen.
    let without_plank = choose_unused_neuron(&traced, &candidates, &[0, 1, 2], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(without_plank.uuid, "p");
    assert_eq!(without_plank.effect, 0.5);

    // Plank 0.25: p = 0.5 - 0.25 * 1 = 0.25, q = 0.75 - 0.25 * 3 = 0.0, so q is chosen.
    let with_plank = choose_unused_neuron(&traced, &candidates, &[0, 1, 2], 0.25)
        .unwrap()
        .unwrap();
    assert_eq!(with_plank.uuid, "q");
    assert_eq!(with_plank.effect, 0.0);
}

#[test]
fn an_outward_weight_above_one_counts_as_one() {
    // s: outward weights -5.0 and 0.5 range -> scale min(5, 1) = 1, effect 0.5 * 1 = 0.5.
    // Uncapped, s would score 0.5 * 5 = 2.5 and t (0.75) would win.
    // t: outward 0.75, effect 1.0 * 0.75 = 0.75.
    let traced = creature(
        2,
        vec![
            neuron("hidden", "s"),
            neuron("hidden", "t"),
            neuron("output", "output-0"),
        ],
        vec![
            synapse("input-0", "s", 1.0),
            synapse("input-1", "t", 1.0),
            synapse("s", "output-0", -5.0),
            synapse("t", "output-0", 0.75),
        ],
    );
    let candidates = [
        eligible(trace(1, 0.5, 0.0)),
        eligible(trace(1, 1.0, 0.0)),
        untraced(),
    ];

    let choice = choose_unused_neuron(&traced, &candidates, &[0, 1, 2], 0.0)
        .unwrap()
        .unwrap();

    assert_eq!(choice.uuid, "s");
    assert_eq!(choice.effect, 0.5);
}

#[test]
fn a_nan_outward_weight_never_raises_the_scale() {
    // n: its only outward weight is NaN, so its scale stays at 0 and its effect is
    // 0.5 * 0 - 0 = 0.0.
    // m: outward 0.25, effect 0.5 * 0.25 = 0.125.
    // A propagated NaN would make n's effect NaN, and then m would be chosen.
    let traced = creature(
        2,
        vec![
            neuron("hidden", "n"),
            neuron("hidden", "m"),
            neuron("output", "output-0"),
        ],
        vec![
            synapse("input-0", "n", 1.0),
            synapse("input-1", "m", 1.0),
            synapse("n", "output-0", f64::NAN),
            synapse("m", "output-0", 0.25),
        ],
    );
    let candidates = [
        eligible(trace(1, 0.5, 0.0)),
        eligible(trace(1, 0.5, 0.0)),
        untraced(),
    ];

    let choice = choose_unused_neuron(&traced, &candidates, &[0, 1, 2], 0.0)
        .unwrap()
        .unwrap();

    assert_eq!(choice.uuid, "n");
    assert_eq!(choice.effect, 0.0);
}

#[test]
fn equal_effects_keep_the_neuron_visited_first() {
    // a and b are identical: effect 1.0 * 0.5 - 0 = 0.5 each.
    // Visiting runs from the end of the order, so shuffled [0, 1, 2] visits b first
    // and the strict `<` keeps b. Reversing the order flips the winner to a.
    let traced = creature(
        2,
        vec![
            neuron("hidden", "a"),
            neuron("hidden", "b"),
            neuron("output", "output-0"),
        ],
        vec![
            synapse("input-0", "a", 1.0),
            synapse("input-1", "b", 1.0),
            synapse("a", "output-0", 0.5),
            synapse("b", "output-0", 0.5),
        ],
    );
    let candidates = [
        eligible(trace(1, 1.0, 0.0)),
        eligible(trace(1, 1.0, 0.0)),
        untraced(),
    ];

    let forward = choose_unused_neuron(&traced, &candidates, &[0, 1, 2], 0.0)
        .unwrap()
        .unwrap();
    let reversed = choose_unused_neuron(&traced, &candidates, &[1, 0, 2], 0.0)
        .unwrap()
        .unwrap();

    assert_eq!(forward.uuid, "b");
    assert_eq!(reversed.uuid, "a");
}

#[test]
fn a_negative_effect_stops_the_visit_before_a_smaller_later_effect() {
    // x: range 1.0, outward 0.5, in-degree 1. Plank 1.0 gives 0.5 - 1.0 = -0.5.
    // y: range 1.0, outward 0.25, in-degree 2. Plank 1.0 gives 0.25 - 2.0 = -1.75.
    // The visit runs x, then output-0 (skipped), then y. A negative effect breaks
    // the loop, so y, whose effect is smaller, is never reached and x is chosen.
    let traced = creature(
        2,
        vec![
            neuron("hidden", "x"),
            neuron("hidden", "y"),
            neuron("output", "output-0"),
        ],
        vec![
            synapse("input-0", "x", 1.0),
            synapse("input-0", "y", 1.0),
            synapse("input-1", "y", 1.0),
            synapse("x", "output-0", 0.5),
            synapse("y", "output-0", 0.25),
        ],
    );
    let candidates = [
        eligible(trace(1, 1.0, 0.0)),
        eligible(trace(1, 1.0, 0.0)),
        untraced(),
    ];
    let visit = [1, 2, 0];

    let stopped = choose_unused_neuron(&traced, &candidates, &visit, 1.0)
        .unwrap()
        .unwrap();
    assert_eq!(stopped.uuid, "x");
    assert_eq!(stopped.effect, -0.5);

    // Control with no negative effect: plank 0 gives x = 0.5 and y = 0.25, and
    // without a break the smaller y wins.
    let no_negative = choose_unused_neuron(&traced, &candidates, &visit, 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(no_negative.uuid, "y");
    assert_eq!(no_negative.effect, 0.25);
}

#[test]
fn a_newborn_still_in_grace_is_skipped() {
    // a: effect 1.0 * 0.25 = 0.25. b: effect 1.0 * 0.5 = 0.5.
    // With grace 1 on a, a is skipped and b is chosen.
    let traced = creature(
        2,
        vec![
            neuron("hidden", "a"),
            neuron("hidden", "b"),
            neuron("output", "output-0"),
        ],
        vec![
            synapse("input-0", "a", 1.0),
            synapse("input-1", "b", 1.0),
            synapse("a", "output-0", 0.25),
            synapse("b", "output-0", 0.5),
        ],
    );
    let graced = [
        UnusedNeuronCandidate {
            trace: Some(trace(1, 1.0, 0.0)),
            newborn_grace_remaining: 1,
        },
        eligible(trace(1, 1.0, 0.0)),
        untraced(),
    ];
    let chosen = choose_unused_neuron(&traced, &graced, &[0, 1, 2], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(chosen.uuid, "b");

    // Control: with grace 0, a is eligible and its effect 0.25 wins.
    let ungraced = [
        eligible(trace(1, 1.0, 0.0)),
        eligible(trace(1, 1.0, 0.0)),
        untraced(),
    ];
    let control = choose_unused_neuron(&traced, &ungraced, &[0, 1, 2], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(control.uuid, "a");
}

#[test]
fn a_neuron_without_a_trace_is_skipped() {
    // a would score 0.25 if it had a trace, and b scores 0.5. With a untraced, b wins.
    let traced = creature(
        2,
        vec![
            neuron("hidden", "a"),
            neuron("hidden", "b"),
            neuron("output", "output-0"),
        ],
        vec![
            synapse("input-0", "a", 1.0),
            synapse("input-1", "b", 1.0),
            synapse("a", "output-0", 0.25),
            synapse("b", "output-0", 0.5),
        ],
    );
    let untraced_a = [untraced(), eligible(trace(1, 1.0, 0.0)), untraced()];
    let chosen = choose_unused_neuron(&traced, &untraced_a, &[0, 1, 2], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(chosen.uuid, "b");

    // Control: with a traced, a wins at 0.25.
    let traced_a = [
        eligible(trace(1, 1.0, 0.0)),
        eligible(trace(1, 1.0, 0.0)),
        untraced(),
    ];
    let control = choose_unused_neuron(&traced, &traced_a, &[0, 1, 2], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(control.uuid, "a");
}

#[test]
fn a_neuron_with_no_observations_is_skipped() {
    // Count 0 means the neuron was never observed. a would score 0.25 and b 0.5.
    let traced = creature(
        2,
        vec![
            neuron("hidden", "a"),
            neuron("hidden", "b"),
            neuron("output", "output-0"),
        ],
        vec![
            synapse("input-0", "a", 1.0),
            synapse("input-1", "b", 1.0),
            synapse("a", "output-0", 0.25),
            synapse("b", "output-0", 0.5),
        ],
    );
    let unobserved = [
        eligible(trace(0, 1.0, 0.0)),
        eligible(trace(1, 1.0, 0.0)),
        untraced(),
    ];
    let chosen = choose_unused_neuron(&traced, &unobserved, &[0, 1, 2], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(chosen.uuid, "b");

    // Control: count 1 makes a eligible, and its 0.25 wins.
    let observed = [
        eligible(trace(1, 1.0, 0.0)),
        eligible(trace(1, 1.0, 0.0)),
        untraced(),
    ];
    let control = choose_unused_neuron(&traced, &observed, &[0, 1, 2], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(control.uuid, "a");
}

#[test]
fn only_hidden_neurons_are_eligible() {
    // output-0 and k both have range 0.1 and would score below h (0.5) if eligible:
    // k scores 0.1 * 0.5 = 0.05. Neither is hidden, so h wins.
    let traced = creature(
        2,
        vec![
            neuron("output", "output-0"),
            neuron("constant", "k"),
            neuron("hidden", "h"),
        ],
        vec![
            synapse("input-0", "k", 1.0),
            synapse("k", "output-0", 0.5),
            synapse("input-1", "h", 1.0),
            synapse("h", "output-0", 0.5),
        ],
    );
    let candidates = [
        eligible(trace(1, 0.1, 0.0)),
        eligible(trace(1, 0.1, 0.0)),
        eligible(trace(1, 1.0, 0.0)),
    ];
    let chosen = choose_unused_neuron(&traced, &candidates, &[0, 1, 2], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(chosen.uuid, "h");

    // Control: the same constant as a hidden neuron is eligible, and k's 0.05 wins.
    let as_hidden = creature(
        2,
        vec![
            neuron("output", "output-0"),
            neuron("hidden", "k"),
            neuron("hidden", "h"),
        ],
        traced.synapses.clone(),
    );
    let control = choose_unused_neuron(&as_hidden, &candidates, &[0, 1, 2], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(control.uuid, "k");
}

#[test]
fn a_neuron_with_an_empty_uuid_is_skipped() {
    // e (range 0.1, outward 0.5) would score 0.05. h (range 1.0, outward 0.5) scores 0.5.
    // The empty uuid is skipped, so h wins.
    fn fixture(uuid: &str) -> CreatureExport {
        creature(
            2,
            vec![
                neuron("hidden", uuid),
                neuron("hidden", "h"),
                neuron("output", "output-0"),
            ],
            vec![
                synapse("input-0", uuid, 1.0),
                synapse(uuid, "output-0", 0.5),
                synapse("input-1", "h", 1.0),
                synapse("h", "output-0", 0.5),
            ],
        )
    }
    let candidates = [
        eligible(trace(1, 0.1, 0.0)),
        eligible(trace(1, 1.0, 0.0)),
        untraced(),
    ];

    let skipped = choose_unused_neuron(&fixture(""), &candidates, &[0, 1, 2], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(skipped.uuid, "h");

    // Control: the same neuron with a uuid is eligible, and its 0.05 wins.
    let control = choose_unused_neuron(&fixture("e"), &candidates, &[0, 1, 2], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(control.uuid, "e");
}

#[test]
fn nothing_eligible_gives_none_and_one_usable_trace_gives_a_choice() {
    // a has count 0, so nothing is eligible and the result is Ok(None).
    // Control: a hidden neuron with a usable trace is eligible.
    let traced = creature(
        1,
        vec![neuron("hidden", "a"), neuron("output", "output-0")],
        vec![synapse("input-0", "a", 1.0), synapse("a", "output-0", 0.5)],
    );
    let nothing = [eligible(trace(0, 1.0, 0.0)), untraced()];
    assert_eq!(
        choose_unused_neuron(&traced, &nothing, &[0, 1], 0.0),
        Ok(None)
    );

    let something = [eligible(trace(1, 1.0, 0.0)), untraced()];
    let control = choose_unused_neuron(&traced, &something, &[0, 1], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(control.uuid, "a");
}

#[test]
fn a_single_observation_uses_the_midpoint_even_with_a_total() {
    // count 1 is not above 1, so the midpoint (1.0 + 0.0) / 2 = 0.5 is used, not total 0.9.
    let traced = single_hidden_with_output();
    let stats = NeuronTraceStats {
        count: 1,
        maximum_activation: 1.0,
        minimum_activation: 0.0,
        total_activation: Some(0.9),
    };
    let candidates = [eligible(stats), untraced()];
    let choice = choose_unused_neuron(&traced, &candidates, &[0, 1], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(choice.activation, 0.5);
}

#[test]
fn several_observations_use_the_total_over_the_count() {
    // count 4 > 1 and total 3.0 is finite, so the activation is 3.0 / 4 = 0.75.
    // The midpoint (1.0 + 0.0) / 2 = 0.5 would be wrong.
    let traced = single_hidden_with_output();
    let stats = NeuronTraceStats {
        count: 4,
        maximum_activation: 1.0,
        minimum_activation: 0.0,
        total_activation: Some(3.0),
    };
    let candidates = [eligible(stats), untraced()];
    let choice = choose_unused_neuron(&traced, &candidates, &[0, 1], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(choice.activation, 0.75);
}

#[test]
fn a_missing_or_non_finite_total_falls_back_to_the_midpoint() {
    // count 4 > 1, but the total is absent, NaN or +inf, so the midpoint 0.5 is used.
    let traced = single_hidden_with_output();
    for total in [None, Some(f64::NAN), Some(f64::INFINITY)] {
        let stats = NeuronTraceStats {
            count: 4,
            maximum_activation: 1.0,
            minimum_activation: 0.0,
            total_activation: total,
        };
        let candidates = [eligible(stats), untraced()];
        let choice = choose_unused_neuron(&traced, &candidates, &[0, 1], 0.0)
            .unwrap()
            .unwrap();
        assert_eq!(choice.activation, 0.5, "total {total:?}");
    }
}

#[test]
fn a_non_finite_activation_range_is_never_chosen() {
    // A NaN minimum gives a NaN effect, and an infinite maximum gives an infinite one.
    // Neither is below f64::MAX, so neither is chosen.
    let traced = single_hidden_with_output();
    let nan_range = [eligible(trace(1, 1.0, f64::NAN)), untraced()];
    assert_eq!(
        choose_unused_neuron(&traced, &nan_range, &[0, 1], 0.0),
        Ok(None)
    );

    let infinite_range = [eligible(trace(1, f64::INFINITY, 0.0)), untraced()];
    assert_eq!(
        choose_unused_neuron(&traced, &infinite_range, &[0, 1], 0.0),
        Ok(None)
    );

    // Control: a finite range is chosen (1.0 * 0.5 = 0.5).
    let finite = [eligible(trace(1, 1.0, 0.0)), untraced()];
    let control = choose_unused_neuron(&traced, &finite, &[0, 1], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(control.uuid, "a");

    // Mixed: the NaN neuron is visited first (it is last in the order) and is passed
    // over, so the finite neuron wins.
    let mixed = creature(
        2,
        vec![
            neuron("hidden", "bad"),
            neuron("hidden", "good"),
            neuron("output", "output-0"),
        ],
        vec![
            synapse("input-0", "bad", 1.0),
            synapse("input-1", "good", 1.0),
            synapse("bad", "output-0", 0.5),
            synapse("good", "output-0", 0.5),
        ],
    );
    let candidates = [
        eligible(trace(1, 1.0, f64::NAN)),
        eligible(trace(1, 1.0, 0.0)),
        untraced(),
    ];
    let chosen = choose_unused_neuron(&mixed, &candidates, &[1, 0, 2], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(chosen.uuid, "good");
}

#[test]
fn a_candidate_count_that_differs_from_the_neuron_list_is_refused() {
    // Three neurons with two candidates is refused. Three candidates is accepted.
    let traced = creature(
        2,
        vec![
            neuron("hidden", "a"),
            neuron("hidden", "b"),
            neuron("output", "output-0"),
        ],
        vec![
            synapse("input-0", "a", 1.0),
            synapse("input-1", "b", 1.0),
            synapse("a", "output-0", 0.5),
            synapse("b", "output-0", 0.5),
        ],
    );
    let short = [eligible(trace(1, 1.0, 0.0)), eligible(trace(1, 1.0, 0.0))];
    assert_eq!(
        choose_unused_neuron(&traced, &short, &[0, 1, 2], 0.0),
        Err(UnusedChoiceError::CandidateCountMismatch {
            neurons: 3,
            candidates: 2,
        })
    );

    let full = [
        eligible(trace(1, 1.0, 0.0)),
        eligible(trace(1, 1.0, 0.0)),
        untraced(),
    ];
    assert!(choose_unused_neuron(&traced, &full, &[0, 1, 2], 0.0).is_ok());
}

#[test]
fn a_visit_order_that_is_not_a_permutation_is_refused() {
    // a has effect 0.5 and b has 0.25, so the valid order [2, 1, 0] chooses b.
    let traced = creature(
        2,
        vec![
            neuron("hidden", "a"),
            neuron("hidden", "b"),
            neuron("output", "output-0"),
        ],
        vec![
            synapse("input-0", "a", 1.0),
            synapse("input-1", "b", 1.0),
            synapse("a", "output-0", 0.5),
            synapse("b", "output-0", 0.25),
        ],
    );
    let candidates = [
        eligible(trace(1, 1.0, 0.0)),
        eligible(trace(1, 1.0, 0.0)),
        untraced(),
    ];

    assert_eq!(
        choose_unused_neuron(&traced, &candidates, &[0, 1], 0.0),
        Err(UnusedChoiceError::InvalidVisitOrder {
            neurons: 3,
            visits: 2,
            fault: VisitOrderFault::WrongLength,
        })
    );
    assert_eq!(
        choose_unused_neuron(&traced, &candidates, &[0, 1, 3], 0.0),
        Err(UnusedChoiceError::InvalidVisitOrder {
            neurons: 3,
            visits: 3,
            fault: VisitOrderFault::OutOfRange { index: 3 },
        })
    );
    assert_eq!(
        choose_unused_neuron(&traced, &candidates, &[0, 0, 2], 0.0),
        Err(UnusedChoiceError::InvalidVisitOrder {
            neurons: 3,
            visits: 3,
            fault: VisitOrderFault::Duplicate { index: 0 },
        })
    );

    // Control: the valid permutation is accepted and chooses b.
    let valid = choose_unused_neuron(&traced, &candidates, &[2, 1, 0], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(valid.uuid, "b");
}

#[test]
fn a_reached_neuron_without_an_inward_synapse_is_refused() {
    // a has an outward synapse but no inward one, so TypeScript's counter assert fails.
    let defective = creature(
        1,
        vec![neuron("hidden", "a"), neuron("output", "output-0")],
        vec![synapse("a", "output-0", 0.5)],
    );
    let candidates = [eligible(trace(1, 1.0, 0.0)), untraced()];
    assert_eq!(
        choose_unused_neuron(&defective, &candidates, &[0, 1], 0.0),
        Err(UnusedChoiceError::NoInwardSynapse {
            uuid: "a".to_string(),
        })
    );

    // Control: with an inward synapse from the input, a is chosen.
    let repaired = creature(
        1,
        vec![neuron("hidden", "a"), neuron("output", "output-0")],
        vec![synapse("input-0", "a", 1.0), synapse("a", "output-0", 0.5)],
    );
    let control = choose_unused_neuron(&repaired, &candidates, &[0, 1], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(control.uuid, "a");
}

#[test]
fn a_reached_neuron_without_an_outward_synapse_is_refused() {
    // a has an inward synapse but no outward one, so TypeScript's scale assert fails.
    let defective = creature(
        1,
        vec![neuron("hidden", "a"), neuron("output", "output-0")],
        vec![synapse("input-0", "a", 1.0)],
    );
    let candidates = [eligible(trace(1, 1.0, 0.0)), untraced()];
    assert_eq!(
        choose_unused_neuron(&defective, &candidates, &[0, 1], 0.0),
        Err(UnusedChoiceError::NoOutwardSynapse {
            uuid: "a".to_string(),
        })
    );

    // Control: with an outward synapse, a is chosen.
    let repaired = creature(
        1,
        vec![neuron("hidden", "a"), neuron("output", "output-0")],
        vec![synapse("input-0", "a", 1.0), synapse("a", "output-0", 0.5)],
    );
    let control = choose_unused_neuron(&repaired, &candidates, &[0, 1], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(control.uuid, "a");
}

#[test]
fn a_synapse_with_an_empty_end_does_not_count_as_inward() {
    // The only synapse into a has an empty from-uuid, so it is skipped and a has no
    // inward synapse.
    let defective = creature(
        1,
        vec![neuron("hidden", "a"), neuron("output", "output-0")],
        vec![synapse("", "a", 1.0), synapse("a", "output-0", 0.5)],
    );
    let candidates = [eligible(trace(1, 1.0, 0.0)), untraced()];
    assert_eq!(
        choose_unused_neuron(&defective, &candidates, &[0, 1], 0.0),
        Err(UnusedChoiceError::NoInwardSynapse {
            uuid: "a".to_string(),
        })
    );

    // Control: an input synapse with a real from-uuid counts, and a is chosen.
    let repaired = creature(
        1,
        vec![neuron("hidden", "a"), neuron("output", "output-0")],
        vec![synapse("input-0", "a", 1.0), synapse("a", "output-0", 0.5)],
    );
    let control = choose_unused_neuron(&repaired, &candidates, &[0, 1], 0.0)
        .unwrap()
        .unwrap();
    assert_eq!(control.uuid, "a");
}

#[test]
fn a_missing_inward_synapse_after_a_negative_break_is_not_reported() {
    // x has effect 0.5 - 1.0 = -0.5 at plank 1.0, which breaks the visit. The visit
    // order [1, 2, 0] reaches x first, so the defective a (visited after x) is never
    // checked.
    let traced = creature(
        1,
        vec![
            neuron("hidden", "x"),
            neuron("hidden", "a"),
            neuron("output", "output-0"),
        ],
        vec![
            synapse("input-0", "x", 1.0),
            synapse("x", "output-0", 0.5),
            synapse("a", "output-0", 0.5),
        ],
    );
    let candidates = [
        eligible(trace(1, 1.0, 0.0)),
        eligible(trace(1, 1.0, 0.0)),
        untraced(),
    ];
    let visit = [1, 2, 0];

    let stopped = choose_unused_neuron(&traced, &candidates, &visit, 1.0)
        .unwrap()
        .unwrap();
    assert_eq!(stopped.uuid, "x");

    // Control: at plank 0 there is no break, so a is reached and refused.
    assert_eq!(
        choose_unused_neuron(&traced, &candidates, &visit, 0.0),
        Err(UnusedChoiceError::NoInwardSynapse {
            uuid: "a".to_string(),
        })
    );
}

#[test]
fn a_missing_outward_synapse_after_a_negative_break_is_not_reported() {
    // As above, but the defect is a missing outward synapse on a, which has an
    // inward one.
    let traced = creature(
        1,
        vec![
            neuron("hidden", "x"),
            neuron("hidden", "a"),
            neuron("output", "output-0"),
        ],
        vec![
            synapse("input-0", "x", 1.0),
            synapse("x", "output-0", 0.5),
            synapse("input-0", "a", 1.0),
        ],
    );
    let candidates = [
        eligible(trace(1, 1.0, 0.0)),
        eligible(trace(1, 1.0, 0.0)),
        untraced(),
    ];
    let visit = [1, 2, 0];

    let stopped = choose_unused_neuron(&traced, &candidates, &visit, 1.0)
        .unwrap()
        .unwrap();
    assert_eq!(stopped.uuid, "x");

    // Control: at plank 0 there is no break, so a is reached and refused.
    assert_eq!(
        choose_unused_neuron(&traced, &candidates, &visit, 0.0),
        Err(UnusedChoiceError::NoOutwardSynapse {
            uuid: "a".to_string(),
        })
    );
}

#[test]
fn an_error_message_names_the_neuron_it_refused() {
    let error = UnusedChoiceError::NoInwardSynapse {
        uuid: "hidden-7".to_string(),
    };
    assert!(error.to_string().contains("hidden-7"), "{error}");
}

/// The creature and candidate for the activation tests: one hidden neuron `a`
/// with an inward and an outward synapse, and an output that is never eligible.
fn single_hidden_with_output() -> CreatureExport {
    creature(
        1,
        vec![neuron("hidden", "a"), neuron("output", "output-0")],
        vec![synapse("input-0", "a", 1.0), synapse("a", "output-0", 0.5)],
    )
}

#[test]
fn the_newborn_grace_fixture_removes_the_quiet_neuron_and_folds_its_activation() {
    // The TypeScript NewbornGrace fixture (`tracedCreature`), with PLANK = 1e-7.
    // hidden-quiet: range 1.0, outward 1e-6, in-degree 1.
    //   effect = 1.0 * 1e-6 - 1e-7 * 1 = 9e-7.
    // hidden-loud: range 1.0, outward 0.9, in-degree 1.
    //   effect = 1.0 * 0.9 - 1e-7 * 1 = 0.8999999.
    // Both have count 10 and total 5, so the activation is 5 / 10 = 0.5.
    let plank = 1e-7;
    let swing = NeuronTraceStats {
        count: 10,
        maximum_activation: 1.0,
        minimum_activation: 0.0,
        total_activation: Some(5.0),
    };
    // Listed in TypeScript's (from, to) order: input-0 -> hidden-quiet,
    // input-0 -> output-0, input-1 -> hidden-loud, hidden-quiet -> output-0,
    // hidden-loud -> output-0.
    let traced = creature(
        2,
        vec![
            neuron("hidden", "hidden-quiet"),
            neuron("hidden", "hidden-loud"),
            neuron("output", "output-0"),
        ],
        vec![
            synapse("input-0", "hidden-quiet", 1.0),
            synapse("input-0", "output-0", 0.5),
            synapse("input-1", "hidden-loud", 1.0),
            synapse("hidden-quiet", "output-0", 1e-6),
            synapse("hidden-loud", "output-0", 0.9),
        ],
    );

    let no_grace = [eligible(swing), eligible(swing), untraced()];
    let quiet = choose_unused_neuron(&traced, &no_grace, &[0, 1, 2], plank)
        .unwrap()
        .unwrap();
    assert_eq!(quiet.uuid, "hidden-quiet");
    assert_close(quiet.effect, 9e-7);
    assert_eq!(quiet.activation, 0.5);

    // With one round of grace on hidden-quiet, it is skipped and hidden-loud is chosen.
    let graced = [
        UnusedNeuronCandidate {
            trace: Some(swing),
            newborn_grace_remaining: 1,
        },
        eligible(swing),
        untraced(),
    ];
    let loud = choose_unused_neuron(&traced, &graced, &[0, 1, 2], plank)
        .unwrap()
        .unwrap();
    assert_eq!(loud.uuid, "hidden-loud");
    assert_eq!(loud.activation, 0.5);

    // The bias fold: output-0 has IDENTITY and two inbound synapses left after the
    // removal, so no draw is needed. Its bias becomes 0.0 + 1e-6 * 0.5 = 5e-7.
    let removal = remove_neuron(
        &traced,
        &quiet.uuid,
        quiet.activation,
        &[],
        || panic!("the bias fold mints no constant"),
        || 0.0,
    )
    .unwrap()
    .unwrap();

    assert!(removal.unity_constant.is_none());
    let remaining: Vec<&str> = removal
        .creature
        .neurons
        .iter()
        .map(|n| n.uuid.as_str())
        .collect();
    assert_eq!(remaining, ["hidden-loud", "output-0"]);
    let output = removal
        .creature
        .neurons
        .iter()
        .find(|n| n.uuid == "output-0")
        .unwrap();
    assert_close(output.bias, 5e-7);
}
