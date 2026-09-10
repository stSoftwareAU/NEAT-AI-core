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
    PRUNE_PARITY_CASES, PruneRequest, PruneStats, SynapseKey, SynapseType, ValidateOptions,
    creature_validate, parse_creature_json, prune_golden_cases, prune_neuron, prune_neuron_json,
    prune_synapse, prune_synapse_json,
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
    // (Ockham #198), so no request this ABI accepts can fill that list. The key
    // stays on the wire — `cleanup_creature`'s default policy still downgrades
    // for the TypeScript-parity captures — and a case that started filling it
    // would mean an entry point had quietly gone back to the inexact repair.
    for recorded in &golden {
        assert!(
            recorded.response["downgradedIfNeurons"]
                .as_array()
                .is_none_or(|a| a.is_empty()),
            "{}: an ABI entry point downgraded an IF instead of rewriting it",
            recorded.name
        );
    }

    // The IF/typed edge cases and the cascade this milestone is about.
    for required in [
        "if_repair_coalesces_roles",
        "edge_role_identity",
        "cascade_orphan_feeders",
        "static_if_rewrite",
        "restored_if_role",
    ] {
        assert!(
            golden.iter().any(|c| c.name == required),
            "the golden record is missing the {required} case"
        );
    }
}
