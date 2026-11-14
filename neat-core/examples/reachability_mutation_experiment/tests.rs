use super::*;
use neat_core::reachability::{ReachabilityClass as C, reachability_diagnostics};

/// Splits `(from, to)` pairs into the parallel arrays the API takes.
fn split(edges: &[(u32, u32)]) -> (Vec<u32>, Vec<u32>) {
    (
        edges.iter().map(|e| e.0).collect(),
        edges.iter().map(|e| e.1).collect(),
    )
}

/// Inputs 0,1; interior 2..=7; outputs 8,9 with these edges:
/// 0->2, 2->8, 4->3, 3->8, 0->5, 1->9 and 6 reading from 5 (6 is DeadEnd now).
/// Classes: 0 L, 1 L, 2 L, 3 B, 4 B, 5 D, 6 D, 7 I, 8 L, 9 L.
fn bridge_report() -> ReachabilityReport {
    let (f, t) = split(&[(0, 2), (2, 8), (4, 3), (3, 8), (0, 5), (1, 9), (5, 6)]);
    reachability_diagnostics(&f, &t, 10, 2, 2).unwrap()
}

#[test]
fn bridge_candidates_keep_only_pairs_that_join_a_path() {
    let r = bridge_report();
    assert_eq!(r.class_of(6), Some(C::DeadEnd));
    let pairs = [
        0, 2, // Live -> Live: dropped
        0, 3, // Live -> BiasDriven: kept
        5, 8, // DeadEnd -> output (Live): kept
        4, 8, // BiasDriven source: dropped
        0, 7, // Isolated target: dropped
        6, 3, // DeadEnd -> BiasDriven: kept
        5, 6, // DeadEnd -> DeadEnd: target reaches no output, dropped
        7, 8, // Isolated source: dropped
    ];
    assert_eq!(
        bridge_candidates(&pairs, &r).unwrap(),
        vec![0, 3, 5, 8, 6, 3]
    );
}

#[test]
fn bridge_candidates_refuse_bad_buffers() {
    let r = bridge_report();
    let odd = bridge_candidates(&[0, 3, 5], &r).unwrap_err().to_string();
    assert!(odd.contains("odd"), "unexpected message: {odd}");
    assert!(bridge_candidates(&[0, 3], &r).is_ok());
    let range = bridge_candidates(&[0, 3, 5, 10], &r)
        .unwrap_err()
        .to_string();
    assert!(range.contains("outside"), "unexpected message: {range}");
    assert!(bridge_candidates(&[0, 3, 5, 9], &r).is_ok());
}

#[test]
fn empty_statistics_are_unknown_instead_of_a_zero_effect() {
    let result = summarise(&[], true).unwrap().to_json();
    assert_eq!(result["mean_bridge_minus_unbiased"], Value::Null);
    assert_eq!(result["bootstrap_ci9875"], Value::Null);
    assert_eq!(result["pairs"], 0);
}

#[test]
fn an_unmeasured_evaluation_rate_is_unknown() {
    let result = ArmResult {
        initial_cal_mse: 4.0,
        final_cal_mse: 2.0,
        evaluations: 2,
        wall_seconds: 100.0,
        ..ArmResult::default()
    }
    .to_json();
    assert_eq!(result["improvement_per_eval_hour_indicative"], Value::Null);
}

#[test]
fn useful_bias_driven_structure_is_not_reported_as_dead() {
    let result = ArmResult {
        final_bias_driven: 3,
        final_dead_end: 2,
        final_isolated: 1,
        ..ArmResult::default()
    }
    .to_json();
    assert_eq!(result["final_dead_structure"], 3);
    assert_eq!(result["final_interior"]["bias_driven"], 3);
}

#[test]
fn scoring_refuses_empty_mismatched_and_nonfinite_data() {
    let c = teacher(&mut Rng(1));
    for (inputs, targets, cause) in [
        (vec![], vec![], "empty"),
        (vec![[0.0; NUM_INPUTS]], vec![1.0, 2.0], "length"),
        (vec![[0.0; NUM_INPUTS]], vec![f32::INFINITY], "non-finite"),
    ] {
        let error = mse(&c, &Data { inputs, targets }).unwrap_err().to_string();
        assert!(error.contains(cause), "{error}");
    }
    let data = Data {
        inputs: vec![[0.0; NUM_INPUTS]],
        targets: vec![2.0],
    };
    // A zero-bias TANH teacher at zero emits zero: (0 - 2)^2 = 4.
    assert_eq!(mse(&c, &data).unwrap(), 4.0);
}
#[test]
fn paired_search_keeps_parents_and_uses_only_calibration_for_acceptance() {
    let p = prepare(Workload::Sparse, 7).unwrap();
    let original = p.parent.clone();
    let limits = Limits {
        evaluations: 5,
        proposals: 20,
    };
    let a = run_arm(Arm::Bridge, 7, &p.parent, &p.cal, &p.val, limits).unwrap();
    let different_validation = Data {
        inputs: p.val.inputs.clone(),
        targets: vec![50.0; VAL_RECORDS],
    };
    let b = run_arm(
        Arm::Bridge,
        7,
        &p.parent,
        &p.cal,
        &different_validation,
        limits,
    )
    .unwrap();
    assert_eq!(p.parent, original);
    assert_eq!(a.final_cal_mse, b.final_cal_mse);
    assert_eq!(a.accepted, b.accepted);
    assert_ne!(a.final_val_mse, b.final_val_mse);
    assert_eq!(a.calibration_cost.calls, 1 + limits.evaluations);
    assert_eq!(
        a.calibration_cost.records,
        (1 + limits.evaluations) * CAL_RECORDS
    );
    assert_eq!(
        (a.validation_cost.calls, a.validation_cost.records),
        (2, 2 * VAL_RECORDS)
    );
    assert_eq!(a.proposals, a.accepted + a.rejected.values().sum::<usize>());
    assert_eq!(a.bridge_selected + a.bridge_fallbacks, a.proposals);
    assert_eq!(a.stop, "evaluation_budget");
    let mut recurrent = original;
    recurrent.forward_only = false;
    let error = run_arm(Arm::Bridge, 7, &recurrent, &p.cal, &p.val, limits)
        .err()
        .unwrap();
    assert_eq!(
        error.to_string(),
        "experiment requires forward-only parents"
    );
}

#[test]
fn only_a_strict_finite_improvement_replaces_the_parent() {
    let mut parent = teacher(&mut Rng(1));
    let original = parent.clone();
    let child = teacher(&mut Rng(2));
    let mut score = 4.0;
    for loss in [4.0, 5.0, f64::NAN, f64::INFINITY] {
        assert!(!accept(&mut parent, &mut score, child.clone(), loss));
        assert_eq!(parent, original);
        assert_eq!(score, 4.0);
    }
    assert!(accept(&mut parent, &mut score, child.clone(), 3.0));
    assert_eq!(parent, child);
    assert_eq!(score, 3.0);
}

#[test]
fn extended_workloads_retain_bias_paths_and_typed_aggregate_semantics() {
    for workload in WORKLOADS.into_iter().skip(1) {
        let p = prepare(workload, 3).unwrap();
        let c = report(&graph(&p.parent).unwrap(), &p.parent)
            .unwrap()
            .interior_counts();
        // One constant and three quarters of the hidden layer remain input-independent.
        assert_eq!(c.bias_driven, 1 + 3 * workload.hidden() / 4);
        assert_eq!(c.dead_end + c.isolated, 0);
        validate_creature_topology(&p.parent).unwrap();
        if workload == Workload::Mixed {
            assert_eq!(
                p.parent
                    .neurons
                    .iter()
                    .filter(|n| n.squash.as_deref() == Some("MEAN"))
                    .count(),
                workload.hidden() / 4
            );
            assert_eq!(
                p.parent
                    .neurons
                    .iter()
                    .filter(|n| n.squash.as_deref() == Some("MAXIMUM"))
                    .count(),
                workload.hidden() / 4
            );
        }
        if workload == Workload::Conditional {
            assert_eq!(
                p.parent
                    .neurons
                    .iter()
                    .filter(|n| n.squash.as_deref() == Some("IF"))
                    .count(),
                1
            );
            let roles: Vec<_> = p
                .parent
                .synapses
                .iter()
                .filter(|s| s.to_uuid == "decision")
                .map(|s| s.synapse_type.as_deref())
                .collect();
            assert_eq!(roles.iter().filter(|&&r| r == Some("condition")).count(), 1);
            assert_eq!(
                roles.iter().filter(|&&r| r == Some("positive")).count(),
                workload.hidden() / 2
            );
            assert_eq!(
                roles.iter().filter(|&&r| r == Some("negative")).count(),
                workload.hidden() / 2
            );
        }
    }
}

#[test]
fn selection_uses_bridges_then_falls_back_and_handles_a_full_graph() {
    let p = prepare(Workload::Sparse, 0).unwrap();
    let g = graph(&p.parent).unwrap();
    let ((from, to), guided) = pick(Arm::Bridge, &p.parent, &g, &mut Rng(1))
        .unwrap()
        .unwrap();
    assert!(guided);
    let r = report(&g, &p.parent).unwrap();
    assert!(matches!(
        r.class_of(from as usize),
        Some(C::Live | C::DeadEnd)
    ));
    assert_eq!(r.class_of(to as usize), Some(C::BiasDriven));
    let mut full = creature(
        vec![neuron("output", "output-0", 0.0, "IDENTITY")],
        vec![edge("input-0", "output-0", 1.0, None)],
    );
    let g = graph(&full).unwrap();
    let (pair, guided) = pick(Arm::Bridge, &full, &g, &mut Rng(1)).unwrap().unwrap();
    assert!(guided); // The three unused inputs can still be bridged to the output.
    assert_eq!(pair.1, NUM_INPUTS as u32);
    for i in 1..NUM_INPUTS {
        full.synapses
            .push(edge(&format!("input-{i}"), "output-0", 1.0, None));
    }
    let g = graph(&full).unwrap();
    assert_eq!(pick(Arm::Bridge, &full, &g, &mut Rng(1)).unwrap(), None);
    // With all inputs already reaching output, another Live -> Live pair uses fallback.
    let mut live = p.parent.clone();
    for s in &mut live.synapses {
        if s.from_uuid == "c-0" {
            s.from_uuid = "input-3".into();
        }
    }
    let live = cleanup_creature(&live).unwrap().creature;
    let g = graph(&live).unwrap();
    let (a, guided) = pick(Arm::Bridge, &live, &g, &mut Rng(2)).unwrap().unwrap();
    let (b, _) = pick(Arm::Unbiased, &live, &g, &mut Rng(2))
        .unwrap()
        .unwrap();
    assert!(!guided);
    assert_eq!(a, b);
}

#[test]
fn proposal_limit_is_incomplete_but_an_empty_pool_is_a_valid_terminal_state() {
    let p = prepare(Workload::Sparse, 0).unwrap();
    let r = run_arm(
        Arm::Bridge,
        0,
        &p.parent,
        &p.cal,
        &p.val,
        Limits {
            evaluations: 2,
            proposals: 0,
        },
    )
    .unwrap();
    assert_eq!(r.stop, "proposal_limit");
    assert!(!r.complete());
    let full = creature(
        vec![neuron("output", "output-0", 0.0, "IDENTITY")],
        (0..NUM_INPUTS)
            .map(|i| edge(&format!("input-{i}"), "output-0", 1.0, None))
            .collect(),
    );
    let r = run_arm(
        Arm::Bridge,
        0,
        &full,
        &p.cal,
        &p.val,
        Limits {
            evaluations: 2,
            proposals: 3,
        },
    )
    .unwrap();
    assert_eq!(r.stop, "empty_pool");
    assert!(r.complete());
    assert_eq!((r.proposals, r.evaluations), (0, 1));
    assert_eq!(
        r.to_json()["mean_dead_interior_over_valid_offspring"],
        Value::Null
    );
    assert_eq!(
        r.to_json()["mean_bias_driven_interior_over_valid_offspring"],
        Value::Null
    );
}

#[test]
fn evaluation_and_preparation_costs_have_distinct_denominators() {
    let mut r = ArmResult {
        initial_cal_mse: 4.0,
        final_cal_mse: 2.0,
        wall_seconds: 20.0,
        preparation_share_seconds: 10.0,
        ..ArmResult::default()
    };
    r.calibration_cost.add(4.0, 256);
    r.calibration_cost.add(6.0, 256);
    r.validation_cost.add(3.0, 256);
    let json = r.to_json();
    assert_eq!(json["total_seconds_indicative"], 30.0);
    assert_eq!(json["improvement_per_eval_hour_indicative"], 720.0); // 2 * 3600 / (4 + 6).
    assert_eq!(json["improvement_per_total_hour_indicative"], 240.0); // 2 * 3600 / (20 + 10).
    assert_eq!(json["calibration_scoring"]["calls"], 2);
    assert_eq!(json["calibration_scoring"]["records"], 512);
}

#[test]
fn paired_statistics_and_decisions_refuse_unsupported_success_claims() {
    let s = summarise(&[-2.0, -2.0, -2.0], true).unwrap();
    assert_eq!(s.mean_diff, -2.0);
    assert_eq!(s.ci, Some((-2.0, -2.0)));
    assert_eq!((s.bridge_wins, s.bridge_losses, s.ties), (3, 0, 0));
    let mixed = summarise(&[-1.0, 0.0, 1.0], false).unwrap();
    assert_eq!(mixed.mean_diff, 0.0);
    assert_eq!(
        (mixed.bridge_wins, mixed.bridge_losses, mixed.ties),
        (1, 1, 1)
    );
    let invalid = summarise(&[0.0, 0.0, 0.0], true).unwrap();
    assert_eq!(verdict(&s, &invalid, 0.2), "better");
    assert_eq!(verdict(&s, &invalid, 0.19), "inconclusive");
    let more_invalid = summarise(&[1.0, 1.0, 1.0], true).unwrap();
    assert_eq!(verdict(&s, &more_invalid, 1.0), "inconclusive");
    assert_eq!(verdict(&mixed, &invalid, 1.0), "inconclusive");
    let worse = summarise(&[2.0, 2.0, 2.0], true).unwrap();
    assert_eq!(verdict(&worse, &invalid, 1.0), "worse");
    assert_eq!(
        summarise(&[f64::NAN], true).err().unwrap().to_string(),
        "non-finite paired difference"
    );
}

#[test]
fn candidates_obey_authoritative_refusals_without_mutating_the_parent() {
    let parent = cleanup_creature(&creature(
        vec![
            neuron("constant", "c-0", 1.0, "IDENTITY"),
            neuron("hidden", "h", 0.0, "IDENTITY"),
            neuron("output", "output-0", 0.0, "IDENTITY"),
        ],
        vec![
            edge("input-0", "h", 1.0, None),
            edge("c-0", "h", 0.25, None),
            edge("h", "output-0", 1.0, None),
        ],
    ))
    .unwrap()
    .creature;
    let original = parent.clone();
    let child = candidate(&parent, 1, 5, 0.5).unwrap();
    let data = Data {
        inputs: vec![[1.0, 2.0, 0.0, 0.0]],
        targets: vec![0.0],
    };
    // (input0 + 0.5*input1 + constant*0.25)^2 = 2.25^2.
    assert_eq!(mse(&child, &data).unwrap(), 5.0625);
    neat_core::creature_validate(
        &child,
        &neat_core::ValidateOptions {
            forward_only: true,
            ..Default::default()
        },
    )
    .unwrap();
    for (from, to, code) in [(5, 5, 1), (6, 5, 2), (0, 5, 5)] {
        let cause = candidate(&parent, from, to, 0.5).unwrap_err().to_string();
        assert!(
            cause.contains(&format!("MalformedTopology {{ code: {code},")),
            "{cause}"
        );
    }
    let cause = candidate(&parent, 0, 4, 0.5).unwrap_err().to_string();
    assert!(cause.contains("MalformedStructure { code: 2,"), "{cause}");
    let cause = candidate(&parent, 1, 5, f64::NAN).unwrap_err().to_string();
    assert_eq!(cause, "cleanup: Non-finite weight NaN from input-1 to h");
    assert_eq!(parent, original);
}
