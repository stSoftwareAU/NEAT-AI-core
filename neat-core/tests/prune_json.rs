//! The JSON ABI the pruning rewrites cross the WASM boundary on (Issue #592).
//!
//! One Rust implementation, two entry surfaces. These tests grade the *wire*
//! half of that claim natively: every request shape, every failure shape, and
//! the golden record the wasm bundle is compared against in CI
//! (`scripts/check_wasm_prune_parity.ts`). The rewrite semantics themselves are
//! graded by `prune_neuron.rs` / `prune_synapse.rs`; what is asserted here is
//! that routing the same fixtures through JSON changes nothing.

use neat_core::prune_json::{
    PruneOp, PruneResponse, read_golden, reason, run_golden_case, write_golden,
};
use neat_core::{
    CreatureExport, NeuronExport, PRUNE_PARITY_CASES, ProxyStats, PruneGoldenCase, PruneRequest,
    PruneStats, SynapseKey, SynapseType, ValidateOptions, creature_validate, parse_creature_json,
    prune_golden_cases, prune_neuron, prune_neuron_json, prune_synapse, prune_synapse_json,
};

const OPTIONS: ValidateOptions = ValidateOptions {
    neurons: None,
    connections: None,
    feedback_loop: None,
    forward_only: true,
};

/// Parse an answer, failing loudly when the boundary produced something that
/// is not the documented response shape.
fn answer(json: &str) -> PruneResponse {
    serde_json::from_str(json)
        .unwrap_or_else(|e| panic!("answer is not a PruneResponse: {e}\nanswer was: {json}"))
}

#[test]
fn every_neuron_fixture_answers_exactly_what_the_native_call_answers() {
    let mut graded = 0;
    for case in PRUNE_PARITY_CASES {
        let PruneRequest::RemoveNeuron { uuid } = case.request else {
            continue;
        };
        graded += 1;
        let creature = case.before();
        let stats = case.mean_activation.map(PruneStats::mean);
        let native = prune_neuron(&creature, uuid, stats.as_ref());

        let request = serde_json::json!({
            "creature": creature,
            "uuid": uuid,
            "stats": stats.as_ref().map(|s| serde_json::json!({ "meanActivation": s.mean_activation })),
        });
        let response = answer(&prune_neuron_json(&request.to_string()));

        // `constant_bias_fold` asks for a *constant*, which the native call
        // protects; the wire must refuse it in the same words rather than
        // answering a creature the native surface never produced.
        let Ok(native) = native else {
            let error = native.expect_err("checked");
            let failure = response.failure.expect("a refusal names its reason");
            assert_eq!(failure.message, error.to_string(), "{}", case.name);
            assert!(!failure.malformed, "{}", case.name);
            continue;
        };

        assert!(response.ok, "{}: {:?}", case.name, response.failure);
        let over_the_wire = response.creature.expect("an ok answer carries a creature");
        assert_eq!(
            over_the_wire, native.creature,
            "{}: the JSON boundary changed the creature",
            case.name
        );
        assert_eq!(
            response.passes,
            Some(native.passes),
            "{}: pass count lost",
            case.name
        );
    }

    // A fixture list that stopped carrying a neuron removal would leave this
    // test asserting nothing at all.
    assert!(graded > 0, "no RemoveNeuron fixture was graded");
}

#[test]
fn every_synapse_fixture_answers_exactly_what_the_native_call_answers() {
    let mut graded = 0;
    for case in PRUNE_PARITY_CASES {
        let PruneRequest::RemoveSynapse {
            from_uuid,
            to_uuid,
            role,
        } = case.request
        else {
            continue;
        };
        graded += 1;
        let creature = case.before();
        let key = SynapseKey {
            from_uuid: from_uuid.to_string(),
            to_uuid: to_uuid.to_string(),
            role,
        };
        let native = prune_synapse(&creature, &key, None)
            .unwrap_or_else(|e| panic!("{}: native prune failed: {e}", case.name));

        let request = serde_json::json!({
            "creature": creature,
            "synapse": {
                "fromUUID": from_uuid,
                "toUUID": to_uuid,
                "type": match role {
                    SynapseType::Standard => "standard",
                    SynapseType::Condition => "condition",
                    SynapseType::Negative => "negative",
                    SynapseType::Positive => "positive",
                },
            },
        });
        let response = answer(&prune_synapse_json(&request.to_string()));

        assert!(response.ok, "{}: {:?}", case.name, response.failure);
        assert_eq!(
            response.creature.expect("an ok answer carries a creature"),
            native.creature,
            "{}: the JSON boundary changed the creature",
            case.name
        );
    }

    assert!(graded > 0, "no RemoveSynapse fixture was graded");
}

#[test]
fn a_successful_answer_always_carries_a_creature_the_core_validator_accepts() {
    let mut graded = 0;
    for case in prune_golden_cases() {
        let response = answer(&run_golden_case(&case));
        if !response.ok {
            continue;
        }
        graded += 1;
        let creature = response.creature.expect("an ok answer carries a creature");
        creature_validate(&creature, &OPTIONS).unwrap_or_else(|failure| {
            panic!(
                "{}: a successful prune returned a creature the validator rejects: {} ({})",
                case.name, failure.message, failure.reason
            )
        });
    }

    // A record that answered only refusals would pass this vacuously.
    assert!(graded > 0, "no golden case produced a creature to validate");
}

#[test]
fn the_transform_label_crosses_the_boundary_unchanged() {
    // A constant source folds exactly; a varying hidden neuron does not.
    let creature = parse_creature_json(
        r#"{"input":1,"output":1,"forwardOnly":true,
            "neurons":[
              {"type":"constant","uuid":"c-1","bias":0.5},
              {"type":"output","uuid":"output-0","bias":0.25,"squash":"IDENTITY"}],
            "synapses":[
              {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"},
              {"weight":0.2,"fromUUID":"c-1","toUUID":"output-0"}]}"#,
    )
    .expect("fixture parses");

    let exact = answer(&prune_synapse_json(
        &serde_json::json!({
            "creature": creature,
            "synapse": { "fromUUID": "c-1", "toUUID": "output-0" },
        })
        .to_string(),
    ));
    assert_eq!(exact.transform.as_deref(), Some("exact"));

    let varying = parse_creature_json(
        r#"{"input":1,"output":1,"forwardOnly":true,
            "neurons":[
              {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
              {"type":"output","uuid":"output-0","bias":0.25,"squash":"IDENTITY"}],
            "synapses":[
              {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
              {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"},
              {"weight":0.2,"fromUUID":"h-1","toUUID":"output-0"}]}"#,
    )
    .expect("fixture parses");
    let approximate = answer(&prune_neuron_json(
        &serde_json::json!({
            "creature": varying, "uuid": "h-1", "stats": { "meanActivation": 0.5 },
        })
        .to_string(),
    ));
    assert_eq!(approximate.transform.as_deref(), Some("approximate"));

    // 0.25 + 0.2 * 0.5 — the caller's mean, folded, and reported as a fold.
    let folded = approximate
        .creature
        .expect("an ok answer carries a creature");
    assert!((folded.neurons[0].bias - 0.35).abs() < 1e-12);
    let fold = &approximate.bias_folds[0];
    assert_eq!(fold.target_uuid, "output-0");
    assert!(!fold.exact);
}

#[test]
fn the_optional_statistics_payload_reaches_the_compensation() {
    // `h-x` and `h-p` both feed the output; `h-p` is the correlated survivor,
    // so part of what `h-x` carried moves onto its edge and the rest folds.
    let creature = parse_creature_json(
        r#"{"input":1,"output":1,"forwardOnly":true,
            "neurons":[
              {"type":"hidden","uuid":"h-x","bias":0.1,"squash":"LOGISTIC"},
              {"type":"hidden","uuid":"h-p","bias":0.2,"squash":"LOGISTIC"},
              {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}],
            "synapses":[
              {"weight":1.0,"fromUUID":"input-0","toUUID":"h-x"},
              {"weight":1.0,"fromUUID":"input-0","toUUID":"h-p"},
              {"weight":2.0,"fromUUID":"h-x","toUUID":"output-0"},
              {"weight":1.0,"fromUUID":"h-p","toUUID":"output-0"}]}"#,
    )
    .expect("fixture parses");

    let response = answer(&prune_neuron_json(
        &serde_json::json!({
            "creature": creature,
            "uuid": "h-x",
            "stats": {
                "meanActivation": 0.5,
                "variance": 0.04,
                "proxy": { "uuid": "h-p", "meanActivation": 0.4, "variance": 0.02, "covariance": 0.01 },
            },
        })
        .to_string(),
    ));

    assert!(response.ok, "{:?}", response.failure);
    // β = cov / σₛ² = 0.5, and the removed neuron carried W = 2.0, so the
    // survivor's edge gains β·W = 1.0 and the bias takes W·(μ − β·μₛ) = 0.6.
    let share = &response.weight_shares[0];
    assert_eq!(share.from_uuid, "h-p");
    assert_eq!(share.to_uuid, "output-0");
    assert!((share.delta - 1.0).abs() < 1e-12, "{share:?}");
    let fold = &response.bias_folds[0];
    assert!((fold.delta - 0.6).abs() < 1e-12, "{fold:?}");
    // The residual the caller accepted is reported, not hidden.
    assert!(fold.residual_variance.is_some());
}

#[test]
fn an_aggregate_target_is_reported_uncompensated_rather_than_folded() {
    let creature = parse_creature_json(
        r#"{"input":2,"output":1,"forwardOnly":true,
            "neurons":[
              {"type":"hidden","uuid":"h-x","bias":0.1,"squash":"LOGISTIC"},
              {"type":"hidden","uuid":"h-y","bias":0.2,"squash":"LOGISTIC"},
              {"type":"output","uuid":"output-0","bias":0.0,"squash":"MINIMUM"}],
            "synapses":[
              {"weight":1.0,"fromUUID":"input-0","toUUID":"h-x"},
              {"weight":1.0,"fromUUID":"input-1","toUUID":"h-y"},
              {"weight":2.0,"fromUUID":"h-x","toUUID":"output-0"},
              {"weight":1.0,"fromUUID":"h-y","toUUID":"output-0"}]}"#,
    )
    .expect("fixture parses");

    let response = answer(&prune_neuron_json(
        &serde_json::json!({
            "creature": creature, "uuid": "h-x", "stats": { "meanActivation": 0.5 },
        })
        .to_string(),
    ));

    assert!(response.ok, "{:?}", response.failure);
    assert!(response.bias_folds.is_empty(), "an aggregate takes no fold");
    let uncompensated = &response.uncompensated[0];
    assert_eq!(uncompensated.target_uuid, "output-0");
    assert_eq!(uncompensated.squash, "MINIMUM");
    assert_eq!(uncompensated.reason, "AGGREGATE_TARGET");
}

#[test]
fn a_refused_request_returns_a_structured_failure_and_no_creature() {
    let creature = parse_creature_json(
        r#"{"input":1,"output":1,"forwardOnly":true,
            "neurons":[{"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}],
            "synapses":[{"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"}]}"#,
    )
    .expect("fixture parses");

    for (uuid, expected) in [
        ("nope", reason::UNKNOWN_NEURON),
        ("output-0", reason::PROTECTED_NEURON),
        ("input-0", reason::PROTECTED_NEURON),
    ] {
        let response = answer(&prune_neuron_json(
            &serde_json::json!({ "creature": creature, "uuid": uuid }).to_string(),
        ));
        assert!(!response.ok, "{uuid} should be refused");
        assert!(response.creature.is_none(), "a refusal returns no creature");
        let failure = response.failure.expect("a refusal names its reason");
        assert_eq!(failure.reason, expected, "for {uuid}");
        assert!(!failure.malformed, "a refusal is a verdict, not a fault");
        assert!(!failure.message.is_empty());
    }

    // A triple no edge of the creature carries. (`type` is deliberately not
    // what makes it unknown here: outside an `IF` every squash sums whatever
    // reaches it, so `positive` on this pair names the untyped edge — the
    // reading `prune_cleanup::canonical_role` owns.)
    let response = answer(&prune_synapse_json(
        &serde_json::json!({
            "creature": creature,
            "synapse": { "fromUUID": "nope", "toUUID": "output-0" },
        })
        .to_string(),
    ));
    assert!(!response.ok);
    assert_eq!(
        response.failure.expect("named").reason,
        reason::UNKNOWN_SYNAPSE
    );
}

#[test]
fn an_unusable_statistic_is_refused_before_anything_is_rewritten() {
    let creature = parse_creature_json(
        r#"{"input":1,"output":1,"forwardOnly":true,
            "neurons":[
              {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
              {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}],
            "synapses":[
              {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
              {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"},
              {"weight":0.2,"fromUUID":"h-1","toUUID":"output-0"}]}"#,
    )
    .expect("fixture parses");

    let negative = answer(&prune_neuron_json(
        &serde_json::json!({
            "creature": creature,
            "uuid": "h-1",
            "stats": { "meanActivation": 0.5, "variance": -1.0 },
        })
        .to_string(),
    ));
    assert!(!negative.ok);
    assert!(negative.creature.is_none());
    assert_eq!(
        negative.failure.expect("named").reason,
        reason::NEGATIVE_VARIANCE
    );

    // JSON cannot spell NaN or Infinity, so a non-finite statistic arrives as
    // a payload serde refuses — the boundary must call that malformed rather
    // than answer a verdict on the creature.
    let non_finite = answer(&prune_neuron_json(&format!(
        r#"{{"creature":{},"uuid":"h-1","stats":{{"meanActivation":NaN}}}}"#,
        serde_json::to_string(&creature).expect("creature serialises")
    )));
    assert!(!non_finite.ok);
    assert!(non_finite.failure.expect("named").malformed);
}

#[test]
fn a_payload_that_is_not_a_request_is_malformed_and_never_a_verdict() {
    let creature = r#"{"input":1,"output":1,"forwardOnly":true,
        "neurons":[{"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}],
        "synapses":[{"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"}]}"#;

    let payloads = [
        // Not JSON at all.
        "not json".to_string(),
        // A creature, but no target named.
        format!(r#"{{"creature":{creature}}}"#),
        // A key the boundary does not know — a typo must not default silently.
        format!(r#"{{"creature":{creature},"uuid":"output-0","statz":{{}}}}"#),
        // No creature.
        r#"{"uuid":"h-1"}"#.to_string(),
    ];

    for payload in payloads {
        let response = answer(&prune_neuron_json(&payload));
        assert!(!response.ok, "payload should be refused: {payload}");
        assert!(response.creature.is_none());
        let failure = response.failure.expect("a malformed payload says so");
        assert!(failure.malformed, "for {payload}");
        assert_eq!(failure.reason, reason::MALFORMED_REQUEST);
        assert!(
            failure.message.starts_with("MALFORMED_REQUEST:"),
            "for {payload}: {}",
            failure.message
        );
    }

    // A role spelling the wire does not carry is malformed, not "standard".
    let response = answer(&prune_synapse_json(&format!(
        r#"{{"creature":{creature},"synapse":{{"fromUUID":"input-0","toUUID":"output-0","type":"POSITIVE"}}}}"#
    )));
    assert!(!response.ok);
    assert!(response.failure.expect("named").malformed);
}

#[test]
fn a_creature_larger_than_the_boundary_walks_is_refused_before_it_allocates() {
    let response = answer(&prune_neuron_json(
        r#"{"creature":{"input":17179869180,"output":1,"neurons":[],"synapses":[]},"uuid":"h-1"}"#,
    ));
    assert!(!response.ok);
    assert!(response.failure.expect("named").malformed);
}

#[test]
fn the_golden_record_is_what_the_native_abi_answers_today() {
    // Regenerating is deliberate and explicit: a record that rewrote itself on
    // every run would grade nothing.
    if std::env::var_os("UPDATE_PRUNE_GOLDEN").is_some() {
        write_golden().expect("the golden record is writable");
    }

    let golden = read_golden().expect("the committed golden record loads");
    let cases = prune_golden_cases();

    assert_eq!(
        golden.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        cases.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        "the golden record and the case list have drifted; regenerate with \
         UPDATE_PRUNE_GOLDEN=1 cargo test -p neat-core --test prune_json"
    );

    for (recorded, case) in golden.iter().zip(cases.iter()) {
        assert_eq!(recorded.request, case.request, "{}: request", case.name);
        let fresh: serde_json::Value =
            serde_json::from_str(&run_golden_case(case)).expect("the answer is JSON");
        assert_eq!(
            recorded.response, fresh,
            "{}: the native answer no longer matches the golden record; \
             regenerate with UPDATE_PRUNE_GOLDEN=1 cargo test -p neat-core --test prune_json",
            case.name
        );
    }
}

#[test]
fn the_golden_record_covers_the_shapes_the_wasm_bundle_is_graded_on() {
    let golden = read_golden().expect("the committed golden record loads");

    assert!(
        golden.iter().any(|c| c.op == PruneOp::Neuron),
        "no neuron removal in the golden record"
    );
    assert!(
        golden.iter().any(|c| c.op == PruneOp::Synapse),
        "no synapse removal in the golden record"
    );
    assert!(
        golden
            .iter()
            .any(|c| c.response["failure"]["malformed"] == serde_json::json!(true)),
        "no malformed payload in the golden record"
    );
    assert!(
        golden
            .iter()
            .any(|c| c.response["failure"]["malformed"] == serde_json::json!(false)),
        "no refused request in the golden record"
    );
    assert!(
        golden
            .iter()
            .any(|c| c.response["transform"] == serde_json::json!("exact")),
        "no exact transform in the golden record"
    );
    assert!(
        golden
            .iter()
            .any(|c| c.response["transform"] == serde_json::json!("approximate")),
        "no approximate transform in the golden record"
    );
    // Every list the response can carry is reached by some case, so no payload
    // shape crosses the boundary ungraded.
    for payload in [
        "removedSynapses",
        "cascadeNeurons",
        "cascadeSynapses",
        "foldedNeurons",
        "staticIfNeurons",
        "restoredIfRoles",
        "biasFolds",
        "weightShares",
        "uncompensated",
        // Ockham #197's rewrite report: a record that stopped carrying one
        // would leave the wasm bundle ungraded on every conversion.
        "convertedNeurons",
    ] {
        assert!(
            golden.iter().any(|c| c.response[payload]
                .as_array()
                .is_some_and(|a| !a.is_empty())),
            "no golden case carries a non-empty {payload}"
        );
    }

    // `downgradedIfNeurons` is the exception, and it is asserted the other way
    // round: both JSON entry points ask cleanup for `IfRepair::Rewrite`
    // (Ockham #198), so no request this ABI accepts can fill that list and no
    // golden case may carry one. A case that started filling it would mean an
    // entry point had quietly gone back to the inexact repair.
    //
    // Satisfied by absence, so on its own it would let the field be renamed or
    // deleted unnoticed. `a_downgraded_if_list_still_crosses_under_its_wire_name`
    // is the positive half that keeps the key pinned.
    for recorded in &golden {
        assert!(
            recorded.response["downgradedIfNeurons"]
                .as_array()
                .is_none_or(|a| a.is_empty()),
            "{}: an ABI entry point downgraded an IF instead of rewriting it",
            recorded.name
        );
    }

    // `droppedMean` rides inside an `uncompensated` entry rather than at the
    // top level, so a non-empty `uncompensated` alone does not reach it: a
    // record whose every entry had dropped its magnitude would grade the
    // Ockham #197 report on nothing.
    assert!(
        golden.iter().any(|c| c.response["uncompensated"]
            .as_array()
            .is_some_and(|entries| entries
                .iter()
                .any(|entry| entry.get("droppedMean").is_some()))),
        "no golden case carries an uncompensated target with a droppedMean"
    );

    // The IF/typed edge cases and the cascade this milestone is about, then
    // the twelve corner cases the pruning guarantee is stated in (Ockham
    // #201). A record that stopped carrying one of these would leave that
    // shape ungraded on the built bundle.
    for required in [
        "if_repair_coalesces_roles",
        "edge_role_identity",
        "cascade_orphan_feeders",
        "static_if_rewrite",
        "restored_if_role",
        "last_edge_into_output_folds_a_mean",
        "last_edge_from_a_constant_folds_exactly",
        "last_edge_from_an_observation_folds_a_mean",
        "sole_source_of_two_outputs_folds_into_both",
        "one_output_of_several_loses_its_last_edge",
        "output_if_rewritten_in_place",
        "no_statistic_prunes_uncompensated",
        "single_edge_aggregate_converted",
        "single_edge_hypot_v2_becomes_absolute",
        "aggregate_keeps_its_squash_with_two_edges",
        "zero_edge_aggregate_outputs_fold",
        "three_deep_chain_collapses",
    ] {
        assert!(
            golden.iter().any(|c| c.name == required),
            "the golden record is missing the {required} case"
        );
    }

    // Both halves of the one-edge conversion table, and the one squash a
    // zero-edge fold rewrites: a record carrying only `IDENTITY` conversions
    // would let the `ABSOLUTE` arm go ungraded.
    for (from, to) in [("MINIMUM", "IDENTITY"), ("HYPOTv2", "ABSOLUTE")] {
        assert!(
            golden.iter().any(|c| c.response["convertedNeurons"]
                .as_array()
                .is_some_and(|list| list
                    .iter()
                    .any(|conversion| conversion["from"] == from && conversion["to"] == to))),
            "no golden case converts {from} to {to}"
        );
    }
}

#[test]
fn a_downgraded_if_list_still_crosses_under_its_wire_name() {
    // No request this ABI accepts fills `downgraded_if_neurons` any more
    // (Ockham #198), so the golden record can only assert it is *absent* — an
    // assertion a rename or a deletion would also satisfy. This is the half
    // that cannot be satisfied by absence: a filled response must serialise
    // under `downgradedIfNeurons` and read back as what it was.
    let filled = PruneResponse {
        ok: true,
        downgraded_if_neurons: vec!["if-1".to_string()],
        ..PruneResponse::default()
    };
    let wire: serde_json::Value = serde_json::to_value(&filled).expect("the response serialises");
    assert_eq!(
        wire["downgradedIfNeurons"],
        serde_json::json!(["if-1"]),
        "the downgraded-IF list lost its wire name: {wire}"
    );
    assert_eq!(
        answer(&serde_json::to_string(&filled).expect("the response serialises")),
        filled,
        "the downgraded-IF list did not survive the round trip"
    );

    // And empty stays skipped, which is why every golden case omits it.
    let empty = serde_json::to_value(PruneResponse {
        ok: true,
        ..PruneResponse::default()
    })
    .expect("the response serialises");
    assert!(
        empty.get("downgradedIfNeurons").is_none(),
        "an empty list must be skipped, not written: {empty}"
    );
}

/// `h-agg` is a `MINIMUM` reading `input-1` and `h-x`, so removing `h-x` leaves
/// it one edge; `h-mean` reads three sources and keeps two.
const AGGREGATE_WIRE_JSON: &str = r#"{"input":2,"output":1,"forwardOnly":true,
    "neurons":[
      {"type":"hidden","uuid":"h-x","bias":0.1,"squash":"IDENTITY"},
      {"type":"hidden","uuid":"h-agg","bias":0.2,"squash":"MINIMUM"},
      {"type":"hidden","uuid":"h-mean","bias":0.3,"squash":"MEAN"},
      {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}],
    "synapses":[
      {"weight":1.0,"fromUUID":"input-0","toUUID":"h-x"},
      {"weight":0.75,"fromUUID":"input-1","toUUID":"h-agg"},
      {"weight":0.5,"fromUUID":"h-x","toUUID":"h-agg"},
      {"weight":0.75,"fromUUID":"input-0","toUUID":"h-mean"},
      {"weight":0.25,"fromUUID":"input-1","toUUID":"h-mean"},
      {"weight":2.0,"fromUUID":"h-x","toUUID":"h-mean"},
      {"weight":1.0,"fromUUID":"h-agg","toUUID":"output-0"},
      {"weight":1.0,"fromUUID":"h-mean","toUUID":"output-0"}]}"#;

#[test]
fn a_conversion_and_a_dropped_magnitude_both_cross_the_wire() {
    let creature = parse_creature_json(AGGREGATE_WIRE_JSON).expect("fixture parses");
    let response = answer(&prune_neuron_json(
        &serde_json::json!({
            "creature": creature, "uuid": "h-x", "stats": { "meanActivation": 0.5 },
        })
        .to_string(),
    ));

    assert!(response.ok, "{:?}", response.failure);
    assert_eq!(
        response.converted_neurons.len(),
        1,
        "{:?}",
        response.converted_neurons
    );
    let conversion = &response.converted_neurons[0];
    assert_eq!(conversion.uuid, "h-agg");
    assert_eq!(conversion.from, "MINIMUM");
    assert_eq!(conversion.to, "IDENTITY");

    // `MEAN` keeps two terms, so it keeps its squash and reports what it lost.
    let mean = response
        .uncompensated
        .iter()
        .find(|u| u.target_uuid == "h-mean")
        .expect("the MEAN target is named");
    assert_eq!(mean.squash, "MEAN");
    let dropped = mean
        .dropped_mean
        .expect("a supplied mean names a magnitude");
    assert!((dropped - 2.0 * 0.5).abs() < 1e-12, "{dropped}");
}

#[test]
fn a_report_with_no_conversion_and_no_magnitude_omits_both_keys() {
    let creature = parse_creature_json(AGGREGATE_WIRE_JSON).expect("fixture parses");
    let json =
        prune_neuron_json(&serde_json::json!({ "creature": creature, "uuid": "h-x" }).to_string());

    assert!(!json.contains("droppedMean"), "{json}");
    // The `MINIMUM` is still converted, so the key is present here — what must
    // not appear is an empty list.
    assert!(!json.contains("[]"), "empty lists are omitted: {json}");

    let bare = prune_synapse_json(
        &serde_json::json!({
            "creature": creature,
            "synapse": { "fromUUID": "input-0", "toUUID": "h-mean" },
        })
        .to_string(),
    );
    assert!(!bare.contains("convertedNeurons"), "{bare}");
    assert!(!bare.contains("droppedMean"), "{bare}");
}

// --- corner cases 1-12 across the JSON boundary (Ockham #201) ---------------
//
// `prune_neuron.rs` and `prune_synapse.rs` grade what each of these shapes
// *means*. What is graded here is that asking for it over JSON — the surface
// NEAT-AI's `WasmPruneNeuron.ts` calls, and the one Ockham's native calls
// mirror — answers exactly what the native call answers, comes back `ok`, and
// carries a creature `creature_validate` accepts.
//
// Each case is a named entry of `prune_golden_cases`, so the request a test
// drives and the request the wasm bundle is graded on in CI are the same
// bytes, and there is one home for every fixture.
//
// Two oracles, deliberately: the wire/native comparison **shares** the rewrite
// — that is the claim, since this module is a translation layer and nothing
// else — so each test also asserts the numbers the documented forward-pass
// forms require (`W · μ` into a bias, `|W · μ|` for a `HYPOT`, the squash a
// conversion lands on), derived in the test rather than read back out of the
// answer. A fault in the rewrite moves both sides of the first assertion and
// exactly one side of the second.

/// The golden case carrying `name`, or a loud failure naming what is there.
fn golden_case(name: &str) -> PruneGoldenCase {
    let cases = prune_golden_cases();
    cases
        .into_iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| {
            panic!(
                "no golden case named {name}; the record carries: {}",
                prune_golden_cases()
                    .iter()
                    .map(|c| c.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

/// The statistics a golden request carries, read back into the native type.
///
/// # Panics
///
/// Panics on a `stats` object this ABI would not accept. The requests are
/// built in `prune_json::prune_golden_cases`, so that is a defect in the
/// record rather than a runtime condition, and it must fail loudly rather than
/// silently grade the native call against *no* statistics.
fn request_stats(request: &serde_json::Value) -> Option<PruneStats> {
    let stats = request.get("stats")?;
    let number = |value: &serde_json::Value, key: &str| {
        value[key]
            .as_f64()
            .unwrap_or_else(|| panic!("golden stats key {key} is not a number: {value}"))
    };
    Some(PruneStats {
        mean_activation: number(stats, "meanActivation"),
        variance: stats.get("variance").map(|v| {
            v.as_f64()
                .unwrap_or_else(|| panic!("golden variance is not a number: {v}"))
        }),
        proxy: stats.get("proxy").map(|proxy| ProxyStats {
            uuid: proxy["uuid"]
                .as_str()
                .unwrap_or_else(|| panic!("golden proxy names no uuid: {proxy}"))
                .to_string(),
            mean_activation: number(proxy, "meanActivation"),
            variance: number(proxy, "variance"),
            covariance: number(proxy, "covariance"),
        }),
    })
}

/// The role a golden synapse request names, in the wire spelling the ABI
/// accepts.
fn request_role(synapse: &serde_json::Value) -> SynapseType {
    match synapse.get("type").and_then(serde_json::Value::as_str) {
        None | Some("standard") => SynapseType::Standard,
        Some("condition") => SynapseType::Condition,
        Some("negative") => SynapseType::Negative,
        Some("positive") => SynapseType::Positive,
        Some(other) => panic!("golden request names an unknown role: {other}"),
    }
}

/// What the **native** call answers for a golden case, written down in the
/// wire shape so the two can be compared whole.
///
/// # Panics
///
/// Panics when the case's request is not one this helper can make natively, or
/// when the native call refuses it. Both are defects in a record whose cases
/// are meant to succeed.
fn native_answer(case: &PruneGoldenCase) -> PruneResponse {
    let creature: CreatureExport = serde_json::from_value(case.request["creature"].clone())
        .unwrap_or_else(|e| panic!("{}: the golden creature does not parse: {e}", case.name));
    let stats = request_stats(&case.request);
    let result = match case.op {
        PruneOp::Neuron => {
            let uuid = case.request["uuid"]
                .as_str()
                .unwrap_or_else(|| panic!("{}: a neuron case names no uuid", case.name));
            prune_neuron(&creature, uuid, stats.as_ref())
        }
        PruneOp::Synapse => {
            let synapse = &case.request["synapse"];
            let key = SynapseKey {
                from_uuid: synapse["fromUUID"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{}: a synapse case names no source", case.name))
                    .to_string(),
                to_uuid: synapse["toUUID"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{}: a synapse case names no target", case.name))
                    .to_string(),
                role: request_role(synapse),
            };
            prune_synapse(&creature, &key, stats.as_ref())
        }
    };
    PruneResponse::from(
        &result.unwrap_or_else(|e| panic!("{}: the native call refused it: {e}", case.name)),
    )
}

/// Drive the named golden case through the JSON entry point and assert the
/// boundary changed nothing: the same `PruneResponse` the native call builds,
/// `ok: true`, and a creature the core validator accepts.
fn crosses_unchanged(name: &str) -> PruneResponse {
    let case = golden_case(name);
    let wire = answer(&run_golden_case(&case));

    assert!(wire.ok, "{name}: refused over the wire: {:?}", wire.failure);
    assert_eq!(
        wire,
        native_answer(&case),
        "{name}: the JSON boundary changed the answer"
    );

    let creature = wire
        .creature
        .clone()
        .expect("an ok answer carries a creature");
    creature_validate(&creature, &OPTIONS).unwrap_or_else(|failure| {
        panic!(
            "{name}: the wire answered a creature the validator rejects: {} ({})",
            failure.message, failure.reason
        )
    });
    wire
}

/// The bias the answer gives `uuid`.
fn bias_of(creature: &CreatureExport, uuid: &str) -> f64 {
    neuron_of(creature, uuid).bias
}

/// The squash the answer gives `uuid`, defaulted the way the wire shape does.
fn squash_of(creature: &CreatureExport, uuid: &str) -> String {
    neuron_of(creature, uuid)
        .squash
        .clone()
        .unwrap_or_else(|| "IDENTITY".to_string())
}

fn neuron_of<'a>(creature: &'a CreatureExport, uuid: &str) -> &'a NeuronExport {
    creature
        .neurons
        .iter()
        .find(|n| n.uuid == uuid)
        .unwrap_or_else(|| panic!("the answer carries no neuron {uuid}"))
}

/// Assert a fold landed on `target` and moved its bias by `delta`.
fn assert_fold(response: &PruneResponse, target: &str, delta: f64) {
    let fold = response
        .bias_folds
        .iter()
        .find(|f| f.target_uuid == target)
        .unwrap_or_else(|| panic!("no fold for {target}: {:?}", response.bias_folds));
    assert!(
        (fold.delta - delta).abs() < 1e-12,
        "{target}: folded {} where {delta} was owed",
        fold.delta
    );
}

#[test]
fn corner_case_1_the_last_edge_into_an_output_folds_the_callers_mean() {
    let response = crosses_unchanged("last_edge_into_output_folds_a_mean");
    let creature = response.creature.clone().expect("ok");

    // `W · μ` = 2.0 · 0.6 on top of the output's own 0.3.
    assert_fold(&response, "output-0", 2.0 * 0.6);
    assert!((bias_of(&creature, "output-0") - (0.3 + 1.2)).abs() < 1e-12);
    assert_eq!(response.transform.as_deref(), Some("approximate"));
    // The source kept no outward edge, so the cascade took it.
    assert!(
        response.cascade_neurons.contains(&"h-1".to_string()),
        "{:?}",
        response.cascade_neurons
    );
}

#[test]
fn corner_case_2_the_last_edge_from_a_constant_folds_exactly() {
    let response = crosses_unchanged("last_edge_from_a_constant_folds_exactly");
    let creature = response.creature.clone().expect("ok");

    // A constant is worth its own bias on every record, so `w · b` is the
    // whole of what went: no statistic is involved and the label says exact.
    assert_eq!(response.transform.as_deref(), Some("exact"));
    assert_fold(&response, "output-0", 0.2 * 0.5);
    assert!((bias_of(&creature, "output-0") - (0.25 + 0.1)).abs() < 1e-12);
    assert!(response.bias_folds[0].exact, "{:?}", response.bias_folds);
    // The corner case is the **bare** target: the constant was its only
    // source, and the cut took the constant with it.
    assert!(
        !creature.synapses.iter().any(|s| s.to_uuid == "output-0"),
        "output-0 still has something to sum, so this is not the zero-edge case"
    );
    assert!(
        !creature.neurons.iter().any(|n| n.uuid == "c-1"),
        "the constant kept no outward edge, so the cascade should have taken it"
    );
}

#[test]
fn corner_case_3_the_last_edge_from_an_observation_folds_the_callers_mean() {
    let response = crosses_unchanged("last_edge_from_an_observation_folds_a_mean");
    let creature = response.creature.clone().expect("ok");

    assert_fold(&response, "output-0", 1.5 * 0.6);
    assert!((bias_of(&creature, "output-0") - (0.3 + 0.9)).abs() < 1e-12);
    // The other output keeps the observation it reads, untouched.
    assert!((bias_of(&creature, "output-1") - 0.1).abs() < 1e-12);
    assert!(
        creature
            .synapses
            .iter()
            .any(|s| s.from_uuid == "input-0" && s.to_uuid == "output-1"),
        "the untouched output lost its observation edge"
    );
}

#[test]
fn corner_case_4_the_sole_source_of_two_outputs_folds_into_both() {
    let response = crosses_unchanged("sole_source_of_two_outputs_folds_into_both");
    let creature = response.creature.clone().expect("ok");

    assert_eq!(response.bias_folds.len(), 2, "one fold per target");
    assert_fold(&response, "output-0", 2.0 * 0.6);
    assert_fold(&response, "output-1", -0.5 * 0.6);
    assert!((bias_of(&creature, "output-0") - 1.5).abs() < 1e-12);
    assert!((bias_of(&creature, "output-1") - -0.5).abs() < 1e-12);
    assert!(
        response.uncompensated.is_empty(),
        "{:?}",
        response.uncompensated
    );
}

#[test]
fn corner_case_5_only_the_output_that_loses_its_last_edge_goes_bare() {
    let response = crosses_unchanged("one_output_of_several_loses_its_last_edge");
    let creature = response.creature.clone().expect("ok");

    assert_fold(&response, "output-0", 2.0 * 0.6);
    assert_fold(&response, "output-1", -0.5 * 0.6);
    assert!(
        !creature.synapses.iter().any(|s| s.to_uuid == "output-0"),
        "output-0 should have nothing left to sum"
    );
    assert!(
        creature
            .synapses
            .iter()
            .any(|s| s.from_uuid == "input-1" && s.to_uuid == "output-1"),
        "output-1 lost the observation it still reads"
    );
}

#[test]
fn corner_case_6_an_output_carrying_if_is_rewritten_in_place() {
    let response = crosses_unchanged("output_if_rewritten_in_place");
    let creature = response.creature.clone().expect("ok");

    // The condition is empty, so it settles at 0 — not `> 0` — and the output
    // flattens onto its negative arm. The exact repair, never the downgrade.
    assert_eq!(
        response.static_if_neurons.len(),
        1,
        "{:?}",
        response.static_if_neurons
    );
    assert_eq!(response.static_if_neurons[0].uuid, "output-0");
    assert_eq!(response.static_if_neurons[0].branch, "negative");
    assert!(
        response.downgraded_if_neurons.is_empty(),
        "the wire downgraded an IF instead of rewriting it"
    );
    // The declared target width is the fleet's contract: an output is repaired
    // where it stands, never removed or reordered.
    assert_eq!(creature.output, 1);
    assert_eq!(
        creature
            .neurons
            .iter()
            .filter(|n| n.neuron_type == "output")
            .count(),
        1
    );
}

#[test]
fn corner_case_7_a_prune_with_no_statistic_reports_what_went_uncompensated() {
    let response = crosses_unchanged("no_statistic_prunes_uncompensated");
    let creature = response.creature.clone().expect("ok");

    // No statistic is not a refusal: what cannot be compensated is named.
    assert!(response.bias_folds.is_empty(), "{:?}", response.bias_folds);
    let entry = &response.uncompensated[0];
    assert_eq!(entry.target_uuid, "output-0");
    assert_eq!(entry.reason, "NO_STATISTICS");
    assert!(
        entry.dropped_mean.is_none(),
        "no statistic and no fixed value proves no magnitude"
    );
    assert_eq!(response.transform.as_deref(), Some("approximate"));
    assert!(
        (bias_of(&creature, "output-0") - 0.0).abs() < 1e-12,
        "the target's bias moved with nothing to move it by"
    );
}

#[test]
fn corner_case_8_an_aggregate_left_with_one_edge_becomes_identity() {
    let response = crosses_unchanged("single_edge_aggregate_converted");
    let creature = response.creature.clone().expect("ok");

    assert_eq!(
        response.converted_neurons.len(),
        1,
        "{:?}",
        response.converted_neurons
    );
    let conversion = &response.converted_neurons[0];
    assert_eq!(conversion.uuid, "h-agg");
    assert_eq!(conversion.from, "MINIMUM");
    assert_eq!(conversion.to, "IDENTITY");
    assert_eq!(squash_of(&creature, "h-agg"), "IDENTITY");
    // Reducing one term is that term, so the bias is carried over untouched.
    assert!((bias_of(&creature, "h-agg") - 0.2).abs() < 1e-12);
}

#[test]
fn corner_case_9_a_hypot_v2_left_with_one_edge_becomes_absolute() {
    let response = crosses_unchanged("single_edge_hypot_v2_becomes_absolute");
    let creature = response.creature.clone().expect("ok");

    let conversion = &response.converted_neurons[0];
    assert_eq!(conversion.uuid, "h-agg");
    assert_eq!(conversion.from, "HYPOTv2");
    assert_eq!(conversion.to, "ABSOLUTE");
    assert_eq!(squash_of(&creature, "h-agg"), "ABSOLUTE");
    // `|bias + w·a|` is what both forms compute with one term, so the bias
    // stays where it was.
    assert!((bias_of(&creature, "h-agg") - -0.35).abs() < 1e-12);
}

#[test]
fn corner_case_10_an_aggregate_left_with_two_edges_reports_the_dropped_term() {
    let response = crosses_unchanged("aggregate_keeps_its_squash_with_two_edges");
    let creature = response.creature.clone().expect("ok");

    // Still reducing two terms, so no point-wise form says the same thing.
    assert!(
        response.converted_neurons.is_empty(),
        "{:?}",
        response.converted_neurons
    );
    assert_eq!(squash_of(&creature, "h-mean"), "MEAN");
    let entry = &response.uncompensated[0];
    assert_eq!(entry.target_uuid, "h-mean");
    assert_eq!(entry.reason, "AGGREGATE_TARGET");
    let dropped = entry
        .dropped_mean
        .expect("a supplied mean names the magnitude that went");
    assert!((dropped - 2.0 * 0.5).abs() < 1e-12, "{dropped}");
    assert_eq!(response.transform.as_deref(), Some("approximate"));
}

#[test]
fn corner_case_11_every_aggregate_left_with_no_inward_edge_takes_the_fold() {
    let response = crosses_unchanged("zero_edge_aggregate_outputs_fold");
    let creature = response.creature.clone().expect("ok");

    // Nothing left to aggregate is a point-wise reading again, so each output
    // takes the term the removal cost it — in the shape its own empty
    // forward-pass form implies.
    let summed: f64 = -2.0 * 0.6;
    for (uuid, squash, delta) in [
        ("output-0", "MINIMUM", summed),
        ("output-1", "MAXIMUM", summed),
        ("output-2", "MEAN", summed),
        // `HYPOT` reads one term as `|w·a|`, so a magnitude is what folds.
        ("output-3", "HYPOT", summed.abs()),
        // `HYPOTv2` never reads its bias with nothing to square, so the fold
        // only means something with the squash rewritten (Ockham #196).
        ("output-4", "ABSOLUTE", summed),
    ] {
        assert_fold(&response, uuid, delta);
        assert_eq!(squash_of(&creature, uuid), squash, "{uuid}: squash");
        assert!(
            (bias_of(&creature, uuid) - (0.25 + delta)).abs() < 1e-12,
            "{uuid}: bias is {}",
            bias_of(&creature, uuid)
        );
    }
    assert!(
        response.uncompensated.is_empty(),
        "a bare aggregate went uncompensated: {:?}",
        response.uncompensated
    );
    // The one squash that moved is **reported**, and only that one: a caller
    // reading the report must never have to discover a rewrite by diffing the
    // creature it got back against the one it sent.
    assert_eq!(
        response.converted_neurons.len(),
        1,
        "{:?}",
        response.converted_neurons
    );
    let conversion = &response.converted_neurons[0];
    assert_eq!(conversion.uuid, "output-4");
    assert_eq!(conversion.from, "HYPOTv2");
    assert_eq!(conversion.to, "ABSOLUTE");
}

#[test]
fn corner_case_12_one_cut_collapses_a_three_deep_hidden_chain() {
    let response = crosses_unchanged("three_deep_chain_collapses");
    let creature = response.creature.clone().expect("ok");

    for uuid in ["h-1", "h-2", "h-3"] {
        assert!(
            response.cascade_neurons.contains(&uuid.to_string()),
            "{uuid} was left behind: {:?}",
            response.cascade_neurons
        );
        assert!(
            !creature.neurons.iter().any(|n| n.uuid == uuid),
            "{uuid} is still in the answer"
        );
    }
    assert_fold(&response, "output-0", 2.0 * 0.6);
    assert!(
        creature
            .synapses
            .iter()
            .any(|s| s.from_uuid == "input-0" && s.to_uuid == "output-0"),
        "the output lost the observation it still reads"
    );
}
