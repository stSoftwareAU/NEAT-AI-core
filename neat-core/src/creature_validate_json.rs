//! The JSON ABI [`creature_validate`] crosses the WASM boundary on
//! (Issue #562).
//!
//! NEAT-AI calls the validator from TypeScript, so the rules need a wire form.
//! The existing exports split between packed byte buffers
//! (`propagate_topological`) and scalar arguments; neither fits a whole
//! creature in and a structured failure out, so this one is **JSON in, JSON
//! out** — the creature is already JSON on the host, and the failure is a
//! record, not a number.
//!
//! # Request
//!
//! ```json
//! {
//!   "creature": { "input": 1, "output": 1, "neurons": [], "synapses": [] },
//!   "options": { "neurons": 3, "connections": 2, "feedbackLoop": false, "forwardOnly": true }
//! }
//! ```
//!
//! `creature` is the [`CreatureExport`] wire shape
//! [`crate::parse_creature_json`] already accepts. Keys it does not name are
//! ignored, so a creature carrying NEAT-AI's own extra fields still validates.
//! `options` is optional, every key in it is optional, and an **unknown option
//! key is a failure** rather than a silent default: a payload saying
//! `forwardonly` would otherwise validate a production creature under the
//! wrong rules and call it healthy.
//!
//! ## Two creature shapes, one set of rules (NEAT-AI#3803)
//!
//! A request names **exactly one** creature, in one of two shapes; naming both
//! or neither is a boundary fault:
//!
//! | Key | Shape | Use it when |
//! |-----|-------|-------------|
//! | `creature` | the export wire form: index-free, UUID-wired, input neurons implicit | the caller has a creature file |
//! | `runtimeCreature` | [`crate::creature_validate_runtime::RuntimeCreature`]: every neuron listed, synapses wired by position | the caller holds a creature in memory |
//!
//! The export form cannot carry an input neuron's own id or position, a
//! non-finite or absent bias, a non-integer id or width, or a malformed memetic
//! record — so a host asking about one of those defects over `creature` is told
//! its creature is healthy, or told its payload is malformed, when the rules
//! would have named the fault. `runtimeCreature` is the shape that can carry
//! them; both run the same rules, so the answer does not depend on the shape.
//!
//! The creature is deserialised with serde alone — deliberately *not* through
//! [`crate::parse_creature_json`], whose width check would shadow rules 2 and 3
//! and answer `InvalidInputCount` where NEAT-AI expects
//! `Must have at least one input neurons was: 0`.
//!
//! # Response
//!
//! ```json
//! { "ok": true,  "stats": { "input": 1, "constant": 0, "hidden": 1, "output": 1, "connections": 2 } }
//! { "ok": false, "failure": { "class": "ValidationError", "reason": "NO_INWARD_CONNECTIONS",
//!                             "message": "hidden neuron h1 has no inward connections",
//!                             "neuronIndex": 1, "synapseIndex": null, "malformed": false } }
//! ```
//!
//! `class` and `reason` are the verbatim NEAT-AI union members, so the host
//! rehydrates a `TopologyError` or a `ValidationError` without a translation
//! table. `neuronIndex` / `synapseIndex` are `null` unless the rule stopped on
//! one, and are what lets the host run its own host-side checks
//! (`neuron.creature`, `neuron.index`, `neuron.validate()`, `debugWrite`)
//! against the same neuron.
//!
//! # Malformed input cannot panic
//!
//! A panic in WASM aborts the module and takes the host's session with it, and
//! `catch_unwind` is not available there — so the boundary must not be able to
//! panic in the first place. It mirrors the [`MALFORMED_BUFFER`] convention in
//! [`crate::topology_ops`]: a payload that is not a request comes back as an
//! ordinary structured failure carrying `"malformed": true` and a message
//! leading with [`MALFORMED_REQUEST`], never as a trap.
//!
//! Two faults are refused before any rule runs:
//!
//! | Fault | Why it cannot reach the rules |
//! |-------|-------------------------------|
//! | the payload is not a request | serde reports it; the message names what it read |
//! | the creature declares more than [`MAX_REQUEST_NEURONS`] neurons | the walk allocates one entry per neuron *before* the first rule, so a declared `"input": 17179869180` would abort on the allocation |
//!
//! [`MALFORMED_BUFFER`]: crate::topology_ops::MALFORMED_BUFFER

use serde::{Deserialize, Serialize};

use crate::creature::CreatureExport;
use crate::creature_validate::{
    ValidateOptions, ValidationFailure, ValidationStats, creature_validate,
};
use crate::creature_validate_runtime::{RuntimeCreature, creature_validate_runtime};
use crate::network::MAX_NODE_COUNT;

/// Message prefix every boundary fault leads with.
///
/// The JSON twin of [`crate::topology_ops::MALFORMED_BUFFER`]: one greppable
/// marker that says "this payload never reached a rule", so a host cannot read
/// a boundary fault as a verdict on its creature.
pub const MALFORMED_REQUEST: &str = "MALFORMED_REQUEST:";

/// Largest creature the boundary will validate, in neurons (implicit inputs
/// included).
///
/// The same ceiling [`crate::compile_creature`] enforces
/// ([`MAX_NODE_COUNT`]) — a creature past it cannot be compiled or scored, and
/// the validator's per-neuron allocation happens before the first rule, so the
/// count is checked at the boundary rather than trusted from the payload.
pub const MAX_REQUEST_NEURONS: usize = MAX_NODE_COUNT;

/// The `options` half of a request — every key optional, unknown keys refused.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RequestOptions {
    #[serde(default)]
    neurons: Option<usize>,
    #[serde(default)]
    connections: Option<usize>,
    #[serde(default)]
    feedback_loop: Option<bool>,
    #[serde(default)]
    forward_only: Option<bool>,
}

impl From<RequestOptions> for ValidateOptions {
    fn from(options: RequestOptions) -> Self {
        Self {
            neurons: options.neurons,
            connections: options.connections,
            feedback_loop: options.feedback_loop,
            forward_only: options.forward_only.unwrap_or(false),
        }
    }
}

/// A whole request: the creature, and the options to validate it under.
///
/// The creature is described in **exactly one** of two shapes — `creature`,
/// the export wire form, or `runtimeCreature`, the in-memory form a host holds
/// (NEAT-AI#3803). Naming both, or neither, is a boundary fault: a request that
/// cannot say which creature it means must not be answered with a verdict.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ValidateRequest {
    #[serde(default)]
    creature: Option<CreatureExport>,
    #[serde(default, rename = "runtimeCreature")]
    runtime_creature: Option<RuntimeCreature>,
    #[serde(default)]
    options: RequestOptions,
}

/// The five counters, as the host reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct ValidationStatsJson {
    /// Input neurons counted.
    pub input: u32,
    /// Constant neurons counted.
    pub constant: u32,
    /// Hidden neurons counted.
    pub hidden: u32,
    /// Output neurons counted.
    pub output: u32,
    /// Synapses counted.
    pub connections: u32,
}

impl From<ValidationStats> for ValidationStatsJson {
    fn from(stats: ValidationStats) -> Self {
        Self {
            input: stats.input,
            constant: stats.constant,
            hidden: stats.hidden,
            output: stats.output,
            connections: stats.connections,
        }
    }
}

/// The first violated rule, or the boundary fault that stopped the request
/// reaching one.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationFailureJson {
    /// `"TopologyError"` or `"ValidationError"` — the class the host rehydrates.
    pub class: String,
    /// The verbatim NEAT-AI `reason` union member.
    pub reason: String,
    /// The human-readable message, reproducing the TypeScript text.
    pub message: String,
    /// Index of the neuron the rule stopped on, `null` when it was not a
    /// neuron rule.
    pub neuron_index: Option<u32>,
    /// Index of the synapse the rule stopped on, `null` when it was not a
    /// synapse rule.
    pub synapse_index: Option<u32>,
    /// `true` when the payload never reached a rule — see [`MALFORMED_REQUEST`].
    pub malformed: bool,
}

impl From<ValidationFailure> for ValidationFailureJson {
    fn from(failure: ValidationFailure) -> Self {
        Self {
            class: failure.class.as_str().to_string(),
            reason: failure.reason.to_string(),
            message: failure.message,
            neuron_index: failure.neuron_index,
            synapse_index: failure.synapse_index,
            malformed: false,
        }
    }
}

/// What [`creature_validate_json`] answers with.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ValidateResponse {
    /// `true` when the creature broke no rule.
    pub ok: bool,
    /// The counters — present only when `ok`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<ValidationStatsJson>,
    /// The first violated rule, or the boundary fault — present only when not
    /// `ok`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<ValidationFailureJson>,
}

impl ValidateResponse {
    /// A boundary fault: the payload never reached a rule.
    ///
    /// Reported as `ValidationError` / `OTHER` — the payload said nothing about
    /// the creature's topology, so claiming a `TopologyError` would be a
    /// verdict this call never reached — with `malformed` set and the message
    /// led by [`MALFORMED_REQUEST`] so neither the host nor a log reader can
    /// mistake it for one.
    fn malformed(detail: impl std::fmt::Display) -> Self {
        Self {
            ok: false,
            stats: None,
            failure: Some(ValidationFailureJson {
                class: crate::creature_validate::FailureClass::Validation
                    .as_str()
                    .to_string(),
                reason: crate::creature_validate::reason::OTHER.to_string(),
                message: format!("{MALFORMED_REQUEST} {detail}"),
                neuron_index: None,
                synapse_index: None,
                malformed: true,
            }),
        }
    }
}

/// Validate a creature described by a JSON request, answering with JSON.
///
/// This is the whole of the WASM export: `wasm_exports::wasm_creature_validate`
/// is a `#[wasm_bindgen]` rename over it, so the ABI is testable natively.
/// The request and response shapes, and the malformed-input contract, are in
/// the module documentation.
///
/// Never panics and never returns an error: a payload that is not a request
/// comes back as a structured failure carrying `"malformed": true`.
///
/// ```
/// use neat_core::creature_validate_json;
///
/// let answer = creature_validate_json(r#"{ "creature": "not a creature" }"#);
/// assert!(answer.contains("MALFORMED_REQUEST"));
/// ```
pub fn creature_validate_json(request: &str) -> String {
    let response = validate_request(request);
    // The response is derived `Serialize` over owned `String`s and plain
    // numbers, so this cannot fail — but a fallback that says so beats an
    // `unwrap` that would abort the module if it ever did.
    serde_json::to_string(&response).unwrap_or_else(|error| {
        format!(
            r#"{{"ok":false,"failure":{{"class":"ValidationError","reason":"OTHER","message":"{MALFORMED_REQUEST} response could not be serialised: {error}","neuronIndex":null,"synapseIndex":null,"malformed":true}}}}"#
        )
    })
}

/// The typed half of [`creature_validate_json`], kept separate so the tests
/// read the answer as a value rather than as text.
fn validate_request(request: &str) -> ValidateResponse {
    let request: ValidateRequest = match serde_json::from_str(request) {
        Ok(request) => request,
        Err(error) => return ValidateResponse::malformed(error),
    };

    let options = request.options.into();
    let outcome = match (&request.creature, &request.runtime_creature) {
        (Some(creature), None) => {
            let declared = creature.input.saturating_add(creature.neurons.len());
            if let Some(refusal) = refuse_oversized(declared) {
                return refusal;
            }
            creature_validate(creature, &options)
        }
        (None, Some(runtime)) => {
            if let Some(refusal) = refuse_oversized(runtime.neurons.len()) {
                return refusal;
            }
            creature_validate_runtime(runtime, &options)
        }
        (Some(_), Some(_)) => {
            return ValidateResponse::malformed(
                "a request names both `creature` and `runtimeCreature`; it must name exactly one",
            );
        }
        (None, None) => {
            return ValidateResponse::malformed(
                "a request names neither `creature` nor `runtimeCreature`; it must name exactly one",
            );
        }
    };

    match outcome {
        Ok(stats) => ValidateResponse {
            ok: true,
            stats: Some(stats.into()),
            failure: None,
        },
        Err(failure) => ValidateResponse {
            ok: false,
            stats: None,
            failure: Some(failure.into()),
        },
    }
}

/// Refuse a creature bigger than the boundary will walk, before the per-neuron
/// allocation a declared count of `17179869180` would abort on.
fn refuse_oversized(declared: usize) -> Option<ValidateResponse> {
    (declared > MAX_REQUEST_NEURONS).then(|| {
        ValidateResponse::malformed(format!(
            "creature declares {declared} neurons, exceeding the maximum of {MAX_REQUEST_NEURONS}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_response_shape_is_the_documented_wire_shape() {
        let answer = creature_validate_json(
            r#"{ "creature": { "input": 1, "output": 1,
                 "neurons": [ { "type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY" } ],
                 "synapses": [ { "fromUUID": "input-0", "toUUID": "output-0", "weight": 1.0 } ] } }"#,
        );

        assert_eq!(
            answer,
            r#"{"ok":true,"stats":{"input":1,"constant":0,"hidden":0,"output":1,"connections":1}}"#
        );
    }

    #[test]
    fn a_failure_names_the_class_reason_message_and_index() {
        let answer = creature_validate_json(
            r#"{ "creature": { "input": 1, "output": 1,
                 "neurons": [ { "type": "banana", "id": 1000001, "uuid": "b1", "bias": 0.0 },
                              { "type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY" } ],
                 "synapses": [ { "fromUUID": "input-0", "toUUID": "b1", "weight": 1.0 },
                               { "fromUUID": "b1", "toUUID": "output-0", "weight": 1.0 } ] } }"#,
        );

        assert_eq!(
            answer,
            r#"{"ok":false,"failure":{"class":"TopologyError","reason":"INVALID_NEURON_TYPE",
"message":"1000001) Invalid type: banana","neuronIndex":1,"synapseIndex":null,"malformed":false}}"#
                .replace('\n', "")
        );
    }

    #[test]
    fn a_runtime_creature_is_validated_by_the_same_rules() {
        let answer = creature_validate_json(
            r#"{ "runtimeCreature": { "input": 1, "output": 1,
                 "neurons": [ { "type": "input", "id": 0, "uuid": "input-0" },
                              { "type": "output", "id": -1, "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY" } ],
                 "synapses": [ { "from": 0, "to": 1, "weight": 1.0 } ] } }"#,
        );

        assert_eq!(
            answer,
            r#"{"ok":true,"stats":{"input":1,"constant":0,"hidden":0,"output":1,"connections":1}}"#
        );
    }

    #[test]
    fn a_defect_the_export_form_cannot_carry_is_named_through_the_runtime_shape() {
        // The same creature, described both ways: an input neuron whose id is
        // not its index. The export form derives `id == index` and answers
        // "healthy"; the runtime form carries the defect and rule 7 names it.
        let export = creature_validate_json(
            r#"{ "creature": { "input": 2, "output": 1,
                 "neurons": [ { "type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY" } ],
                 "synapses": [ { "fromUUID": "input-0", "toUUID": "output-0", "weight": 1.0 },
                               { "fromUUID": "input-1", "toUUID": "output-0", "weight": 1.0 } ] } }"#,
        );
        assert!(export.contains(r#""ok":true"#), "{export}");

        let runtime = creature_validate_json(
            r#"{ "runtimeCreature": { "input": 2, "output": 1,
                 "neurons": [ { "type": "input", "id": 0, "uuid": "input-0" },
                              { "type": "input", "id": 5, "uuid": "input-1" },
                              { "type": "output", "id": -1, "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY" } ],
                 "synapses": [ { "from": 0, "to": 2, "weight": 1.0 },
                               { "from": 1, "to": 2, "weight": 1.0 } ] } }"#,
        );
        assert!(
            runtime.contains(r#""message":"5) invalid input neuron id: 5""#),
            "{runtime}"
        );
        assert!(runtime.contains(r#""malformed":false"#), "{runtime}");
    }

    #[test]
    fn a_request_names_exactly_one_creature() {
        let both = creature_validate_json(
            r#"{ "creature": { "input": 1, "output": 1, "neurons": [], "synapses": [] },
                 "runtimeCreature": { "input": 1, "output": 1, "neurons": [], "synapses": [] } }"#,
        );
        assert!(both.contains(MALFORMED_REQUEST), "{both}");
        assert!(both.contains("must name exactly one"), "{both}");

        let neither = creature_validate_json(r#"{ "options": { "forwardOnly": true } }"#);
        assert!(neither.contains(MALFORMED_REQUEST), "{neither}");
        assert!(neither.contains("must name exactly one"), "{neither}");
    }

    #[test]
    fn an_absurd_declared_width_reaches_the_rules_rather_than_an_allocation() {
        let neurons: Vec<String> = (0..3)
            .map(|index| format!(r#"{{ "type": "input", "id": {index} }}"#))
            .collect();
        let request = format!(
            r#"{{ "runtimeCreature": {{ "input": {}, "output": 1, "neurons": [{}], "synapses": [] }} }}"#,
            MAX_REQUEST_NEURONS + 1,
            neurons.join(",")
        );
        // The declared width is absurd but the payload is small: the walk runs
        // over the neurons that are actually listed, so this reaches the rules
        // and is answered as a miscount rather than as an allocation.
        let answer = creature_validate_json(&request);
        assert!(answer.contains(r#""ok":false"#), "{answer}");
        assert!(answer.contains("input neurons found"), "{answer}");
    }

    #[test]
    fn a_runtime_creature_past_the_ceiling_is_refused_before_the_rules() {
        let neurons: Vec<String> = (0..=MAX_REQUEST_NEURONS)
            .map(|index| format!(r#"{{"type":"input","id":{index}}}"#))
            .collect();
        let request = format!(
            r#"{{ "runtimeCreature": {{ "input": 1, "output": 1, "neurons": [{}], "synapses": [] }} }}"#,
            neurons.join(",")
        );

        let answer = creature_validate_json(&request);
        assert!(
            answer.contains(MALFORMED_REQUEST),
            "{}",
            &answer[..200.min(answer.len())]
        );
        assert!(
            answer.contains("exceeding the maximum"),
            "{}",
            &answer[..200.min(answer.len())]
        );
    }

    #[test]
    fn unknown_creature_keys_are_ignored_but_unknown_request_keys_are_not() {
        let with_extra_creature_key = creature_validate_json(
            r#"{ "creature": { "input": 1, "output": 1, "tags": ["evolved"],
                 "neurons": [ { "type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY" } ],
                 "synapses": [ { "fromUUID": "input-0", "toUUID": "output-0", "weight": 1.0 } ] } }"#,
        );
        assert!(
            with_extra_creature_key.contains(r#""ok":true"#),
            "{with_extra_creature_key}"
        );

        let with_extra_request_key = creature_validate_json(
            r#"{ "creature": { "input": 1, "output": 1, "neurons": [], "synapses": [] }, "verbose": true }"#,
        );
        assert!(
            with_extra_request_key.contains(MALFORMED_REQUEST),
            "{with_extra_request_key}"
        );
    }

    #[test]
    fn the_options_bag_maps_onto_the_validate_options() {
        let options = RequestOptions {
            neurons: Some(3),
            connections: Some(0),
            feedback_loop: Some(true),
            forward_only: Some(true),
        };
        let mapped: ValidateOptions = options.into();

        assert_eq!(mapped.expected_neurons(), Some(3));
        assert_eq!(mapped.expected_connections(), Some(0));
        assert!(mapped.forward_only);
        // `forward_only` wins over an explicit `feedbackLoop: true`.
        assert_eq!(mapped.resolved_feedback_loop(), Some(false));
    }

    #[test]
    fn an_absent_feedback_loop_stays_absent() {
        let mapped: ValidateOptions = RequestOptions::default().into();
        assert_eq!(mapped.resolved_feedback_loop(), None);
        assert!(!mapped.rejects_recursive_synapses());
    }
}
