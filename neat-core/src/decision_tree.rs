//! Canonical decision-tree creature fixtures built from the `IF` aggregate
//! (Issue #555).
//!
//! These are the fleet's **authoritative** small decision trees: NEAT-AI-Forests
//! and any other consumer reads its interpretation of `SquashType::If` and the
//! `Condition` / `Negative` / `Positive` synapse roles from here rather than
//! inventing its own.
//!
//! ## The rule the fixtures encode
//!
//! An `IF` neuron sums its `Condition` inputs. When that sum is **strictly**
//! greater than zero it emits the sum of its `Positive` inputs, otherwise the
//! sum of its `Negative` inputs; its bias is added either way. So the split test
//! `x > t` is written as a condition sum of `x * 1.0 + 1.0 * (-t)`, with a
//! constant neuron supplying the `1.0`.
//!
//! A creature may not carry two synapses between the same ordered pair of
//! neurons, so one `IF` node cannot take all three roles from a single constant.
//! Each fixture therefore declares three shared constants — `const-condition`,
//! `const-positive` and `const-negative`, all `1.0` — and every threshold and
//! leaf value lives in a **weight**, which is what training adjusts.
//!
//! ```mermaid
//! flowchart LR
//!     X["input-0"] -->|"condition w=1"| N(("IF node"))
//!     C["const-condition = 1"] -->|"condition w=-t"| N
//!     P["const-positive = 1"] -->|"positive w=leaf⁺"| N
//!     G["const-negative = 1"] -->|"negative w=leaf⁻"| N
//!     N -->|"sum(condition) &gt; 0 ? leaf⁺ : leaf⁻"| O["output"]
//! ```
//!
//! ## Fixtures
//!
//! | Builder | Shape | Covers |
//! | --- | --- | --- |
//! | [`stump_creature`] | `x > 0.5 ? 3.0 : 0.0` | single split, zero default branch |
//! | [`depth2_tree_creature`] | root on `x0 > 0.5`, both children on `x1 > 0.25` | nested depth-2, all four leaves |
//! | [`linear_base_creature`] | `2x`, no `IF` at all | the pre-graft base |
//! | [`residual_correction_creature`] | `2x + (x > 0.75 ? 1.5 : 0)` | non-zero residual leaf grafted onto the base |
//!
//! [`residual_correction_creature`] is exactly what
//! [`crate::if_graft::graft_if_correction`] produces from
//! [`linear_base_creature`], which is how the helper and the fixture keep each
//! other honest.

use crate::creature::{CreatureExport, NeuronExport, SynapseExport, squash_name_from};
use crate::squash::SquashType;
use crate::synapse_type::SynapseType;

/// Split point of the [`stump_creature`] fixture.
pub const STUMP_THRESHOLD: f64 = 0.5;
/// Leaf value the stump emits above [`STUMP_THRESHOLD`].
pub const STUMP_POSITIVE_VALUE: f64 = 3.0;
/// Root split of the [`depth2_tree_creature`] fixture, on `input-0`.
pub const DEPTH2_ROOT_THRESHOLD: f64 = 0.5;
/// Child split of the [`depth2_tree_creature`] fixture, on `input-1`.
pub const DEPTH2_SPLIT_THRESHOLD: f64 = 0.25;
/// Weight of the linear term in [`linear_base_creature`].
pub const RESIDUAL_BASE_WEIGHT: f64 = 2.0;
/// Split point of the correction grafted in [`residual_correction_creature`].
pub const RESIDUAL_THRESHOLD: f64 = 0.75;
/// Non-zero residual the correction adds above [`RESIDUAL_THRESHOLD`].
pub const RESIDUAL_VALUE: f64 = 1.5;

/// UUID of the shared constant feeding every condition offset.
pub const CONST_CONDITION: &str = "const-condition";
/// UUID of the shared constant feeding every positive leaf.
pub const CONST_POSITIVE: &str = "const-positive";
/// UUID of the shared constant feeding every negative leaf.
pub const CONST_NEGATIVE: &str = "const-negative";

/// One documented record and the output the fixture must produce for it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecisionCase {
    /// Input record, in `input-0`, `input-1`, … order.
    pub inputs: &'static [f32],
    /// The single output value the fixture is documented to produce.
    pub expected: f32,
    /// Which branch of the tree the record exercises.
    pub branch: &'static str,
}

/// Documented outputs of [`stump_creature`].
///
/// `x > 0.5` selects the positive leaf `3.0`; everything at or below the split
/// — the strict `>` includes the threshold itself — takes the zero-valued
/// default branch.
pub const STUMP_CASES: &[DecisionCase] = &[
    DecisionCase {
        inputs: &[0.75],
        expected: 3.0,
        branch: "positive",
    },
    DecisionCase {
        inputs: &[0.25],
        expected: 0.0,
        branch: "negative",
    },
    DecisionCase {
        inputs: &[0.5],
        expected: 0.0,
        branch: "negative (boundary)",
    },
];

/// Documented outputs of [`depth2_tree_creature`].
///
/// Root `x0 > 0.5` picks the `high` subtree, otherwise `low`; each child then
/// splits on `x1 > 0.25`. Leaves are `high/positive = 4.0`,
/// `high/negative = 0.0` (the zero default), `low/positive = 1.0` and
/// `low/negative = -2.0` (the non-zero residual leaf).
pub const DEPTH2_CASES: &[DecisionCase] = &[
    DecisionCase {
        inputs: &[0.9, 0.9],
        expected: 4.0,
        branch: "high/positive",
    },
    DecisionCase {
        inputs: &[0.9, 0.1],
        expected: 0.0,
        branch: "high/negative",
    },
    DecisionCase {
        inputs: &[0.1, 0.9],
        expected: 1.0,
        branch: "low/positive",
    },
    DecisionCase {
        inputs: &[0.1, 0.1],
        expected: -2.0,
        branch: "low/negative",
    },
    DecisionCase {
        inputs: &[0.5, 0.25],
        expected: -2.0,
        branch: "low/negative",
    },
];

/// Documented outputs of [`residual_correction_creature`]: `2x`, plus `1.5`
/// once `x > 0.75`.
pub const RESIDUAL_CASES: &[DecisionCase] = &[
    DecisionCase {
        inputs: &[1.0],
        expected: 3.5,
        branch: "corrected",
    },
    DecisionCase {
        inputs: &[0.75],
        expected: 1.5,
        branch: "uncorrected (boundary)",
    },
    DecisionCase {
        inputs: &[0.25],
        expected: 0.5,
        branch: "uncorrected",
    },
    DecisionCase {
        inputs: &[0.0],
        expected: 0.0,
        branch: "uncorrected",
    },
];

fn constant_neuron(uuid: &str) -> NeuronExport {
    NeuronExport {
        id: None,
        neuron_type: "constant".to_string(),
        uuid: uuid.to_string(),
        bias: 1.0,
        squash: None,
    }
}

fn if_neuron(uuid: &str, neuron_type: &str) -> NeuronExport {
    NeuronExport {
        id: None,
        neuron_type: neuron_type.to_string(),
        uuid: uuid.to_string(),
        bias: 0.0,
        squash: Some(squash_name_from(SquashType::If).to_string()),
    }
}

fn edge(from: &str, to: &str, weight: f64, role: SynapseType) -> SynapseExport {
    SynapseExport {
        from_uuid: from.to_string(),
        to_uuid: to.to_string(),
        weight,
        synapse_type: crate::creature::synapse_type_name_from(role).map(str::to_string),
    }
}

/// The three shared `1.0` constants every fixture tree stands on.
fn shared_constants() -> Vec<NeuronExport> {
    vec![
        constant_neuron(CONST_CONDITION),
        constant_neuron(CONST_POSITIVE),
        constant_neuron(CONST_NEGATIVE),
    ]
}

/// Emit the four synapses of one leaf `IF` node: `feature > threshold ?
/// positive_value : negative_value`.
fn leaf_synapses(
    node: &str,
    feature: &str,
    threshold: f64,
    positive_value: f64,
    negative_value: f64,
) -> Vec<SynapseExport> {
    vec![
        edge(feature, node, 1.0, SynapseType::Condition),
        edge(CONST_CONDITION, node, -threshold, SynapseType::Condition),
        edge(CONST_POSITIVE, node, positive_value, SynapseType::Positive),
        edge(CONST_NEGATIVE, node, negative_value, SynapseType::Negative),
    ]
}

/// Single-split decision stump: `input-0 > 0.5 ? 3.0 : 0.0`.
///
/// The negative leaf weight is `0.0`, so the default branch is the documented
/// zero output. Expected outputs are in [`STUMP_CASES`].
pub fn stump_creature() -> CreatureExport {
    let mut neurons = shared_constants();
    neurons.push(if_neuron("output-0", "output"));

    CreatureExport {
        memetic: None,
        input: 1,
        output: 1,
        neurons,
        synapses: leaf_synapses(
            "output-0",
            "input-0",
            STUMP_THRESHOLD,
            STUMP_POSITIVE_VALUE,
            0.0,
        ),
        semantic_version: None,
        forward_only: true,
    }
}

/// Depth-2 binary decision tree over two observations.
///
/// The root splits on `input-0 > 0.5` and routes to one of two child `IF`
/// nodes, each splitting on `input-1 > 0.25`. Leaves cover both a zero default
/// (`high/negative`) and a non-zero negative residual (`low/negative = -2.0`).
/// Expected outputs are in [`DEPTH2_CASES`].
pub fn depth2_tree_creature() -> CreatureExport {
    let mut neurons = shared_constants();
    neurons.push(if_neuron("low", "hidden"));
    neurons.push(if_neuron("high", "hidden"));
    neurons.push(if_neuron("output-0", "output"));

    let mut synapses = leaf_synapses("low", "input-1", DEPTH2_SPLIT_THRESHOLD, 1.0, -2.0);
    synapses.extend(leaf_synapses(
        "high",
        "input-1",
        DEPTH2_SPLIT_THRESHOLD,
        4.0,
        0.0,
    ));
    // The root's leaves are the two child nodes rather than constants.
    synapses.push(edge("input-0", "output-0", 1.0, SynapseType::Condition));
    synapses.push(edge(
        CONST_CONDITION,
        "output-0",
        -DEPTH2_ROOT_THRESHOLD,
        SynapseType::Condition,
    ));
    synapses.push(edge("high", "output-0", 1.0, SynapseType::Positive));
    synapses.push(edge("low", "output-0", 1.0, SynapseType::Negative));

    CreatureExport {
        memetic: None,
        input: 2,
        output: 1,
        neurons,
        synapses,
        semantic_version: None,
        forward_only: true,
    }
}

/// Purely linear creature — `output-0 = 2 * input-0`, no `IF` node at all.
///
/// The base that [`residual_correction_creature`] grafts a correction onto.
pub fn linear_base_creature() -> CreatureExport {
    CreatureExport {
        memetic: None,
        input: 1,
        output: 1,
        neurons: vec![NeuronExport {
            id: None,
            neuron_type: "output".to_string(),
            uuid: "output-0".to_string(),
            bias: 0.0,
            squash: Some("IDENTITY".to_string()),
        }],
        synapses: vec![SynapseExport {
            from_uuid: "input-0".to_string(),
            to_uuid: "output-0".to_string(),
            weight: RESIDUAL_BASE_WEIGHT,
            synapse_type: None,
        }],
        semantic_version: None,
        forward_only: true,
    }
}

/// [`linear_base_creature`] with a depth-1 `IF` correction of `+1.5` above
/// `input-0 > 0.75`.
///
/// Written out longhand so it can serve as the expected value for
/// [`crate::if_graft::graft_if_correction`] — the helper must reproduce this
/// creature exactly, which is what stops either drifting from the other.
/// Expected outputs are in [`RESIDUAL_CASES`].
pub fn residual_correction_creature() -> CreatureExport {
    let node = "residual-0";
    let condition_one = format!("{node}-condition-one");
    let positive_one = format!("{node}-positive-one");
    let negative_one = format!("{node}-negative-one");

    let mut neurons: Vec<NeuronExport> = [&condition_one, &positive_one, &negative_one]
        .iter()
        .map(|uuid| NeuronExport {
            id: None,
            neuron_type: "constant".to_string(),
            uuid: (*uuid).clone(),
            bias: crate::if_graft::GRAFT_CONSTANT_BIAS,
            squash: None,
        })
        .collect();
    neurons.push(if_neuron(node, "hidden"));
    neurons.push(NeuronExport {
        id: None,
        neuron_type: "output".to_string(),
        uuid: "output-0".to_string(),
        bias: 0.0,
        squash: Some("IDENTITY".to_string()),
    });

    let synapses = vec![
        SynapseExport {
            from_uuid: "input-0".to_string(),
            to_uuid: "output-0".to_string(),
            weight: RESIDUAL_BASE_WEIGHT,
            synapse_type: None,
        },
        edge("input-0", node, 1.0, SynapseType::Condition),
        edge(
            &condition_one,
            node,
            -RESIDUAL_THRESHOLD,
            SynapseType::Condition,
        ),
        edge(&positive_one, node, RESIDUAL_VALUE, SynapseType::Positive),
        edge(&negative_one, node, 0.0, SynapseType::Negative),
        SynapseExport {
            from_uuid: node.to_string(),
            to_uuid: "output-0".to_string(),
            weight: 1.0,
            synapse_type: None,
        },
    ];

    CreatureExport {
        memetic: None,
        input: 1,
        output: 1,
        neurons,
        synapses,
        semantic_version: None,
        forward_only: true,
    }
}
