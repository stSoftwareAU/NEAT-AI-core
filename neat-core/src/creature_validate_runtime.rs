//! The **runtime** creature shape [`creature_validate_runtime`] validates for a host
//! that holds a creature in memory (NEAT-AI#3803).
//!
//! [`mod@crate::creature_validate_json`] takes the export wire form — index-free,
//! UUID-wired, with the input neurons implicit. That form is lossy against
//! what NEAT-AI's `creatureValidate` actually walks, and the loss is not
//! cosmetic: replaying NEAT-AI's own conformance corpus over the wire, eleven
//! cases disagreed with the TypeScript, **four of them by accepting a creature
//! TypeScript rejects**. A validator that answers "valid" for a creature the
//! rules reject is worse than no validator at all, so the shape a host sends
//! has to be able to carry the defect it is asking about:
//!
//! | What the export form cannot carry | Rules it puts out of reach |
//! |-----------------------------------|-----------------------------|
//! | an input neuron listed with its own id and position | 4, 7, 10, 21 |
//! | a bias that is `undefined`, `NaN` or infinite | 8 (and 19) |
//! | a neuron id, or a width, that is not an integer | 2, 3, 5 |
//! | a memetic `weights` value that is not an array | 31 |
//!
//! # Request
//!
//! ```json
//! {
//!   "runtimeCreature": {
//!     "input": 1,
//!     "output": 1,
//!     "neurons": [
//!       { "type": "input",  "id": 0,  "uuid": "input-0" },
//!       { "type": "hidden", "id": 5,  "uuid": "h1", "bias": 0.5, "squash": "TANH" },
//!       { "type": "output", "id": -1, "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY" }
//!     ],
//!     "synapses": [
//!       { "from": 0, "to": 1, "weight": 1.0 },
//!       { "from": 1, "to": 2, "weight": 1.0 }
//!     ]
//!   },
//!   "options": { "forwardOnly": true }
//! }
//! ```
//!
//! Every neuron is listed, inputs included, and a synapse names the **array
//! positions** of its endpoints — the same integers the host holds — so
//! nothing has to be derived and nothing can be derived *away*. Unknown keys
//! on a neuron or on the creature are ignored, so a host's own extra fields
//! (`index`, `tags`, …) travel harmlessly.
//!
//! ## The values JSON has no literal for
//!
//! `bias`, `id`, `input` and `output` accept a JSON number, and `bias`
//! additionally accepts the string sentinels `"NaN"`, `"Infinity"` and
//! `"-Infinity"` — the same convention NEAT-AI's conformance corpus uses. An
//! absent or `null` field is JavaScript's `undefined`, which is how a host says
//! "this neuron carries no bias at all" (rule 8) or "no id" (rule 4).
//!
//! # Response
//!
//! Identical to the export form's — see [`mod@crate::creature_validate_json`].
//! Both shapes run the same rules through
//! the shared rule seam in [`mod@crate::creature_validate`], so a creature cannot be
//! judged differently depending on how it was described.

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::creature::parse_synapse_type;
use crate::creature_validate::{
    MemeticEntry, MemeticWeightEntries, MemeticWeightsView, NeuronKind, NeuronView,
    ValidateOptions, ValidationFailure, ValidationStats, is_js_integer, reason,
    validate_declared_widths, validate_prepared,
};
use crate::synapse_type::SynapseType;

/// A creature exactly as a host holds it: every neuron listed, synapses wired
/// by array position.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RuntimeCreature {
    /// Declared observation count. Any JSON number — rule 2 is what rejects a
    /// non-integer or a value below one.
    #[serde(default)]
    pub input: Option<f64>,
    /// Declared target count, read the same way by rule 3.
    #[serde(default)]
    pub output: Option<f64>,
    /// Every neuron, in position order, input neurons included.
    #[serde(default)]
    pub neurons: Vec<RuntimeNeuron>,
    /// Every synapse, wired by neuron position.
    #[serde(default)]
    pub synapses: Vec<RuntimeSynapse>,
    /// The memetic record, read verbatim so rule 31 can report a malformed one.
    #[serde(default)]
    pub memetic: Option<Value>,
}

/// One neuron in the runtime shape.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RuntimeNeuron {
    /// Declared type, verbatim — an unknown one is rule 20's to report.
    #[serde(default, rename = "type")]
    pub neuron_type: Option<String>,
    /// Runtime id. Absent or `null` is `undefined` (rule 4); a non-integer
    /// reaches rule 5.
    #[serde(default)]
    pub id: Option<f64>,
    /// Wire identity, used by the diagnostic labels in the messages.
    #[serde(default)]
    pub uuid: Option<String>,
    /// Bias — a number, a non-finite sentinel, or absent for `undefined`.
    #[serde(default)]
    pub bias: Option<Value>,
    /// Activation function name.
    #[serde(default)]
    pub squash: Option<String>,
}

/// One synapse in the runtime shape, wired by neuron position.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RuntimeSynapse {
    /// Source neuron position.
    #[serde(default)]
    pub from: Option<f64>,
    /// Destination neuron position.
    #[serde(default)]
    pub to: Option<f64>,
    /// Connection weight. Unread by the rules, so anything the host holds is
    /// accepted rather than refused at the boundary.
    #[serde(default)]
    pub weight: Option<Value>,
    /// `"positive"`, `"negative"` or `"condition"`.
    #[serde(default, rename = "type")]
    pub synapse_type: Option<String>,
}

/// Read a bias: a number, a `"NaN"` / `"Infinity"` / `"-Infinity"` sentinel, or
/// `undefined` for anything else — including a value of the wrong type, which
/// is no more a bias than an absent one and is rule 8's to report.
pub(crate) fn bias_value(bias: Option<&Value>) -> Option<f64> {
    match bias {
        Some(Value::Number(number)) => number.as_f64(),
        Some(Value::String(text)) => match text.as_str() {
            "NaN" => Some(f64::NAN),
            "Infinity" => Some(f64::INFINITY),
            "-Infinity" => Some(f64::NEG_INFINITY),
            _ => None,
        },
        _ => None,
    }
}

/// Validate a creature described in the runtime shape.
///
/// Rules 1–3 are checked here because they read the declared widths; from the
/// neuron walk onwards this is
/// the shared rule seam in [`mod@crate::creature_validate`], the same code the export
/// form runs.
///
/// # Errors
///
/// Returns the [`ValidationFailure`] for the first violated rule. A synapse
/// naming a position no neuron occupies is `Topology` /
/// `INVALID_SYNAPSE_REFERENCE`: the in-memory TypeScript would read `undefined`
/// off the neuron array and throw a `TypeError`, so there is no rule to
/// reproduce and the boundary reports it rather than indexing out of bounds.
pub fn creature_validate_runtime(
    creature: &RuntimeCreature,
    options: &ValidateOptions,
) -> Result<ValidationStats, ValidationFailure> {
    let declared_input = creature.input.unwrap_or(f64::NAN);
    let declared_output = creature.output.unwrap_or(f64::NAN);

    validate_declared_widths(
        creature.neurons.len(),
        declared_input,
        declared_output,
        options,
    )?;

    // Rules 2 and 3 passed, so both widths are integers of at least one.
    let input = declared_input as usize;
    let output = declared_output as usize;

    let views: Vec<NeuronView<'_>> = creature.neurons.iter().map(neuron_view).collect();
    let (from, to, synapse_types) = resolve_endpoints(&creature.synapses, views.len())?;
    let memetic = creature.memetic.as_ref().map(memetic_view);

    validate_prepared(
        &views,
        None,
        input,
        output,
        &from,
        &to,
        &synapse_types,
        options,
        memetic.as_ref(),
    )
}

/// The walk's view of one runtime neuron — no derivation, only reading.
fn neuron_view(neuron: &RuntimeNeuron) -> NeuronView<'_> {
    let declared_type = neuron.neuron_type.as_deref().unwrap_or("");
    let integer_id = neuron.id.filter(|id| is_js_integer(*id));
    NeuronView {
        id: integer_id.map(|id| id as i64),
        non_integer_id: neuron.id.filter(|id| !is_js_integer(*id)),
        kind: NeuronKind::from_declared_runtime(declared_type),
        declared_type,
        uuid: neuron.uuid.as_deref(),
        bias: bias_value(neuron.bias.as_ref()),
        squash: neuron.squash.as_deref(),
    }
}

/// Resolve every synapse's endpoints, refusing a position no neuron occupies.
#[allow(clippy::type_complexity)]
fn resolve_endpoints(
    synapses: &[RuntimeSynapse],
    neuron_count: usize,
) -> Result<(Vec<u32>, Vec<u32>, Vec<SynapseType>), ValidationFailure> {
    let mut from = Vec::with_capacity(synapses.len());
    let mut to = Vec::with_capacity(synapses.len());
    let mut types = Vec::with_capacity(synapses.len());

    for (index, synapse) in synapses.iter().enumerate() {
        let resolve = |endpoint: &str, position: Option<f64>| -> Result<u32, ValidationFailure> {
            position
                .filter(|value| {
                    is_js_integer(*value) && *value >= 0.0 && (*value as usize) < neuron_count
                })
                .map(|value| value as u32)
                .ok_or_else(|| {
                    ValidationFailure::topology(
                        reason::INVALID_SYNAPSE_REFERENCE,
                        format!(
                            "{index}) synapse {endpoint} {} does not name a neuron",
                            position.map_or("undefined".to_string(), |value| value.to_string())
                        ),
                    )
                    .at_synapse(index as u32)
                })
        };

        from.push(resolve("from", synapse.from)?);
        to.push(resolve("to", synapse.to)?);
        types.push(parse_synapse_type(synapse.synapse_type.as_deref()));
    }

    Ok((from, to, types))
}

/// The memetic record as rule 31 reads it, taken verbatim from the host.
///
/// Nothing here refuses a malformed record: a `weights` value that is not an
/// array, an entry that is not an object, a missing `toId` — each is a rule 31
/// failure reported in NEAT-AI's own words, not a parse error that would tell
/// the host nothing about its creature.
pub(crate) fn memetic_view(memetic: &Value) -> crate::creature_validate::MemeticView<'_> {
    let record: Option<&Map<String, Value>> = memetic.as_object();

    let member = |name: &str| -> Vec<(&str, &Value)> {
        record
            .and_then(|record| record.get(name))
            .and_then(Value::as_object)
            .map(|entries| {
                entries
                    .iter()
                    .map(|(key, value)| (key.as_str(), value))
                    .collect()
            })
            .unwrap_or_default()
    };

    crate::creature_validate::MemeticView {
        biases: member("biases").into_iter().map(|(key, _)| key).collect(),
        weights: MemeticWeightsView::ById(
            member("weights")
                .into_iter()
                .map(|(key, value)| (key, weights_view(value)))
                .collect(),
        ),
    }
}

/// One `weights` value: the deltas it lists, or "not an array at all". A host
/// record is always id-keyed, so this is the only shape the runtime form ever
/// carries — see [`MemeticWeightsView::ById`].
fn weights_view(value: &Value) -> MemeticWeightEntries<'_> {
    let Some(entries) = value.as_array() else {
        return MemeticWeightEntries::NotAnArray;
    };

    MemeticWeightEntries::Entries(
        entries
            .iter()
            .map(|entry| {
                let field = |name: &str| entry.as_object().and_then(|entry| entry.get(name));
                let to_id = field("toId");
                let number = to_id.and_then(Value::as_f64);
                MemeticEntry {
                    to_id: number.filter(|id| is_js_integer(*id)).map(|id| id as i64),
                    to_id_text: match to_id {
                        None | Some(Value::Null) => std::borrow::Cow::Borrowed("undefined"),
                        Some(Value::Number(number)) => std::borrow::Cow::Owned(number.to_string()),
                        Some(other) => std::borrow::Cow::Owned(other.to_string()),
                    },
                    to_id_present: !matches!(to_id, None | Some(Value::Null)),
                    weight_present: !matches!(field("weight"), None | Some(Value::Null)),
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature_validate::FailureClass;

    /// `input-0` → `h1` → `output-0`, every neuron listed, wired by position.
    fn healthy() -> Value {
        serde_json::json!({
            "input": 1,
            "output": 1,
            "neurons": [
                { "type": "input", "id": 0, "uuid": "input-0" },
                { "type": "hidden", "id": 1000001, "uuid": "h1", "bias": 0.5, "squash": "TANH" },
                { "type": "output", "id": -1, "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY" }
            ],
            "synapses": [
                { "from": 0, "to": 1, "weight": 1.0 },
                { "from": 1, "to": 2, "weight": 1.0 }
            ]
        })
    }

    /// Validate `creature`, having patched `mutate` into it first.
    fn validate(mutate: impl FnOnce(&mut Value)) -> Result<ValidationStats, ValidationFailure> {
        let mut creature = healthy();
        mutate(&mut creature);
        let creature: RuntimeCreature =
            serde_json::from_value(creature).expect("the runtime shape parses");
        creature_validate_runtime(&creature, &ValidateOptions::default())
    }

    #[test]
    fn a_healthy_runtime_creature_answers_with_its_counters() {
        let stats = validate(|_| {}).expect("the creature breaks no rule");
        assert_eq!(
            (
                stats.input,
                stats.constant,
                stats.hidden,
                stats.output,
                stats.connections
            ),
            (1, 0, 1, 1, 2)
        );
    }

    #[test]
    fn an_input_neuron_whose_id_is_not_its_index_is_rule_7() {
        // The export form derives `id == index` for its implicit inputs, so
        // this defect is unreachable there and reachable here.
        let failure = validate(|creature| creature["neurons"][0]["id"] = serde_json::json!(5))
            .expect_err("an input neuron is identified by its position");
        assert_eq!(failure.class, FailureClass::Validation);
        assert_eq!(failure.reason, "OTHER");
        assert_eq!(failure.message, "5) invalid input neuron id: 5");
        assert_eq!(failure.neuron_index, Some(0));
    }

    #[test]
    fn an_input_neuron_past_the_declared_width_is_rule_10() {
        // An input neuron behind a computational one: the export form lists no
        // input neurons at all, so it cannot describe this creature.
        let failure = validate(|creature| {
            creature["neurons"]
                .as_array_mut()
                .expect("neurons are a list")
                .insert(2, serde_json::json!({ "type": "input", "id": 2 }));
            creature["synapses"] = serde_json::json!([
                { "from": 0, "to": 1, "weight": 1.0 },
                { "from": 1, "to": 3, "weight": 1.0 }
            ]);
        })
        .expect_err("an input neuron may not follow the declared input width");
        assert_eq!(failure.reason, "OTHER");
        assert_eq!(
            failure.message,
            "2) input neuron after the maximum input neurons"
        );
    }

    #[test]
    fn counted_inputs_that_miss_the_declared_width_are_rule_21() {
        let failure = validate(|creature| creature["input"] = serde_json::json!(2))
            .expect_err("two inputs were declared and one was listed");
        assert_eq!(failure.reason, "OTHER");
        assert_eq!(failure.message, "Expected 2 input neurons found: 1");
    }

    #[test]
    fn a_missing_id_is_rule_4_and_a_non_integer_id_is_rule_5() {
        let missing = validate(|creature| creature["neurons"][1]["id"] = Value::Null)
            .expect_err("a neuron with no id");
        assert_eq!(missing.message, "undefined) no id");

        let fractional =
            validate(|creature| creature["neurons"][1]["id"] = serde_json::json!(-1.5))
                .expect_err("a neuron whose id is not an integer");
        assert_eq!(fractional.message, "-1.5) invalid neuron id: -1.5");
    }

    #[test]
    fn the_non_finite_bias_sentinels_reach_rule_8() {
        for (sentinel, printed) in [
            ("NaN", "NaN"),
            ("Infinity", "Infinity"),
            ("-Infinity", "-Infinity"),
        ] {
            let failure =
                validate(|creature| creature["neurons"][1]["bias"] = serde_json::json!(sentinel))
                    .expect_err("a non-finite bias is not a bias");
            assert_eq!(failure.message, format!("1000001) invalid bias: {printed}"));
        }

        let absent = validate(|creature| {
            creature["neurons"][1]
                .as_object_mut()
                .expect("a neuron is an object")
                .remove("bias");
        })
        .expect_err("an absent bias is `undefined`");
        assert_eq!(absent.message, "1000001) invalid bias: undefined");
    }

    #[test]
    fn a_width_that_is_not_an_integer_reaches_rules_2_and_3() {
        let input = validate(|creature| creature["input"] = serde_json::json!(1.5))
            .expect_err("1.5 observations is not a width");
        assert_eq!(
            input.message,
            "Must have at least one input neurons was: 1.5"
        );

        let output = validate(|creature| creature["output"] = serde_json::json!(2.5))
            .expect_err("2.5 targets is not a width");
        assert_eq!(
            output.message,
            "Must have at least one output neurons was: 2.5"
        );
    }

    #[test]
    fn a_synapse_naming_no_neuron_is_refused_rather_than_indexed() {
        let failure = validate(|creature| creature["synapses"][1]["to"] = serde_json::json!(9))
            .expect_err("position 9 holds no neuron");
        assert_eq!(failure.class, FailureClass::Topology);
        assert_eq!(failure.reason, "INVALID_SYNAPSE_REFERENCE");
        assert_eq!(failure.message, "1) synapse to 9 does not name a neuron");
        assert_eq!(failure.synapse_index, Some(1));
    }

    #[test]
    fn memetic_weights_that_are_not_an_array_are_rule_31() {
        let failure = validate(|creature| {
            creature["memetic"] = serde_json::json!({
                "biases": {},
                "weights": { "0": { "toId": 1000001, "weight": 0.5 } }
            });
        })
        .expect_err("a weights entry is a list of deltas or it is nothing");
        assert_eq!(failure.reason, "MEMETIC");
        assert_eq!(failure.message, "Synapse with id 0 has invalid weights.");
    }

    #[test]
    fn a_memetic_entry_missing_its_fields_is_reported_not_refused() {
        let missing_to_id = validate(|creature| {
            creature["memetic"] = serde_json::json!({ "weights": { "0": [{ "weight": 0.5 }] } });
        })
        .expect_err("a delta names the neuron it feeds");
        assert_eq!(
            missing_to_id.message,
            "Memetic from id 0 to id undefined is invalid."
        );

        let missing_weight = validate(|creature| {
            creature["memetic"] = serde_json::json!({ "weights": { "0": [{ "toId": 1000001 }] } });
        })
        .expect_err("a delta carries a weight");
        assert_eq!(
            missing_weight.message,
            "Memetic from id 0 to id 1000001 has invalid weight at index 0."
        );
    }

    #[test]
    fn a_neuron_declaring_an_unknown_type_is_rule_20() {
        let failure =
            validate(|creature| creature["neurons"][1]["type"] = serde_json::json!("banana"))
                .expect_err("a creature has four kinds of neuron");
        assert_eq!(failure.class, FailureClass::Topology);
        assert_eq!(failure.reason, "INVALID_NEURON_TYPE");
        assert_eq!(failure.message, "1000001) Invalid type: banana");
    }

    #[test]
    fn a_host_key_the_rules_do_not_read_is_ignored() {
        let stats = validate(|creature| {
            creature["neurons"][1]["index"] = serde_json::json!(1);
            creature["neurons"][1]["tags"] = serde_json::json!([{ "name": "grafted" }]);
        })
        .expect("a host may carry fields of its own");
        assert_eq!(stats.hidden, 1);
    }
}
