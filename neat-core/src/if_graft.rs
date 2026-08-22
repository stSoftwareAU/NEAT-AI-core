//! Safe construction and grafting of `IF` decision nodes onto a
//! [`CreatureExport`] (Issue #555).
//!
//! A decision-tree node in this fleet is a neuron with [`SquashType::If`] whose
//! inbound synapses carry the three [`SynapseType`] roles: `Condition` inputs
//! are summed, and the neuron emits the `Positive` sum when that condition sum
//! is **strictly** greater than zero, otherwise the `Negative` sum (plus its own
//! bias in either case). Hand-editing neuron/synapse JSON to build one is
//! error-prone: a missing role silently turns the node into a constant, and a
//! source placed after the node reads a stale activation.
//!
//! This module is the single home of that construction rule. Callers describe
//! *what* they want ([`IfNodeSpec`], or [`IfCorrectionSpec`] for the common
//! depth-1 correction) and the helper resolves placement, emits the role
//! strings, and refuses anything malformed with a typed [`GraftError`]. The
//! source creature is never mutated — a graft either returns a new, validated
//! [`CreatureExport`] or it returns an error.
//!
//! What comes back is a creature the shared validator accepts, not merely one
//! that compiles: the constants a graft introduces are listed ahead of every
//! hidden neuron and the whole synapse list is left in canonical
//! `(from, to)` order, which are [`crate::creature_validate()`] rules 11 and 25.
//!
//! ```mermaid
//! flowchart LR
//!     A["IfNodeSpec"] --> B{"names new?<br/>roles present?<br/>edges resolve?"}
//!     B -- no --> E["Err(GraftError)"]
//!     B -- yes --> C{"placement:<br/>after every source,<br/>before every target"}
//!     C -- impossible --> E
//!     C -- ok --> D["build creature"]
//!     D --> F{"validate_topology +<br/>validate_structural_integrity"}
//!     F -- fails --> E
//!     F -- passes --> G["Ok(CreatureExport)"]
//! ```
//!
//! ## Why a grafted node brings its own constant neurons
//!
//! An `IF` condition of the form `x > threshold` needs a constant `1.0` source
//! to carry `-threshold`, and each leaf value needs one too. A creature may not
//! hold two synapses between the same ordered pair of neurons, so the three
//! roles cannot share one constant: [`IfCorrectionSpec`] therefore introduces
//! three (`<uuid>-condition-one`, `<uuid>-positive-one`, `<uuid>-negative-one`),
//! each with bias [`GRAFT_CONSTANT_BIAS`], leaving the thresholds and leaf
//! values in the trainable **weights**.
//!
//! ## Whole trees, and corrections that enter both branches
//!
//! Two shapes need more than one node at a time, so a consumer used to write
//! them out by hand (NEAT-AI-Forests #48):
//!
//! * a **nested tree**, whose child exists only to feed the parent that does
//!   not exist yet. [`graft_if_nodes`] takes the whole post-order batch: a node
//!   may leave its outward edge to a later node in the same batch, and only the
//!   assembled creature is validated. It is still all or nothing — the first
//!   rejection returns a [`GraftError`] and no partial creature escapes.
//! * a correction entering **both branches of an `IF` destination**. An untyped
//!   edge into an `IF` neuron feeds one branch, so the node takes the other with
//!   a typed outward edge ([`IfNodeSpec::with_target_role`]); the second, equal
//!   edge comes through an IDENTITY [`RelaySpec`] ([`graft_relay_node`]),
//!   because a creature may not carry two synapses between the same ordered
//!   pair of neurons.

use std::collections::{HashMap, HashSet};

use crate::creature::{
    CreatureError, CreatureExport, NeuronExport, SynapseExport, parse_squash_name,
    parse_synapse_type, squash_name_from, synapse_type_name_from, validate_creature_width,
};
use crate::squash::SquashType;
use crate::synapse_type::SynapseType;
use crate::topology_ops::{
    STRUCTURAL_VALID, VALID, validate_structural_integrity, validate_topology,
};

/// Bias of every constant neuron a graft introduces.
///
/// Kept at `1.0` so the threshold offset and the leaf values live in the
/// synapse **weights**, which are what training adjusts.
pub const GRAFT_CONSTANT_BIAS: f64 = 1.0;

/// One weighted edge of a grafted node.
///
/// `uuid` names the **source** neuron for an inbound edge
/// ([`IfNodeSpec::with_condition`] / [`with_positive`](IfNodeSpec::with_positive)
/// / [`with_negative`](IfNodeSpec::with_negative) / [`RelaySpec::with_source`])
/// and the **destination** neuron for an outward edge
/// ([`IfNodeSpec::with_target`]).
#[derive(Debug, Clone, PartialEq)]
pub struct GraftEdge {
    /// UUID of the neuron at the other end of the edge.
    pub uuid: String,
    /// Connection weight.
    pub weight: f64,
    /// Role the emitted synapse carries.
    ///
    /// [`SynapseType::Standard`] emits an **untyped** synapse — the additive
    /// edge a point-wise neuron takes. The three `IF` roles emit their role
    /// string, which is how an outward edge reaches one named branch of an `IF`
    /// destination; an untyped edge into an `IF` neuron feeds the positive
    /// branch only (NEAT-AI-Forests #48).
    ///
    /// Only **outward** edges carry a role: an inbound edge takes its role from
    /// the branch it is listed under, so one set here is refused with
    /// [`GraftError::InboundEdgeHasRole`] rather than silently ignored.
    pub role: SynapseType,
}

impl GraftEdge {
    /// Build an untyped edge from a UUID and a weight.
    pub fn new(uuid: impl Into<String>, weight: f64) -> Self {
        Self {
            uuid: uuid.into(),
            weight,
            role: SynapseType::Standard,
        }
    }

    /// Build an outward edge that carries `role`.
    pub fn with_role(uuid: impl Into<String>, weight: f64, role: SynapseType) -> Self {
        Self {
            uuid: uuid.into(),
            weight,
            role,
        }
    }
}

/// A constant neuron introduced alongside a grafted node.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstantSpec {
    /// UUID for the new constant neuron; must not already exist.
    pub uuid: String,
    /// The constant's value — a constant neuron activates to its own bias.
    pub bias: f64,
}

impl ConstantSpec {
    /// Build a constant specification from a UUID and a value.
    pub fn new(uuid: impl Into<String>, bias: f64) -> Self {
        Self {
            uuid: uuid.into(),
            bias,
        }
    }
}

/// Description of one `IF` node to graft onto a creature.
///
/// The grafted neuron is always **hidden**: grafting an output would change the
/// declared output width and move the output block, which the fleet's
/// structural gate requires to stay last.
#[derive(Debug, Clone, PartialEq)]
pub struct IfNodeSpec {
    /// UUID for the new node; must not already exist in the creature.
    pub uuid: String,
    /// Bias added to whichever branch the condition selects.
    pub bias: f64,
    /// Constant neurons introduced by this graft, listed ahead of every hidden
    /// neuron — where `creature_validate` rule 11 requires them — so the node's
    /// own edges may reference them.
    pub constants: Vec<ConstantSpec>,
    /// Inbound edges summed to decide the branch (`> 0` selects positive).
    pub condition: Vec<GraftEdge>,
    /// Inbound edges summed when the condition sum is strictly positive.
    pub positive: Vec<GraftEdge>,
    /// Inbound edges summed when the condition sum is zero or negative.
    pub negative: Vec<GraftEdge>,
    /// Outward edges from the new node to existing non-input, non-constant
    /// neurons.
    pub targets: Vec<GraftEdge>,
}

impl IfNodeSpec {
    /// Start a specification for a node with the given UUID and bias.
    pub fn new(uuid: impl Into<String>, bias: f64) -> Self {
        Self {
            uuid: uuid.into(),
            bias,
            constants: Vec::new(),
            condition: Vec::new(),
            positive: Vec::new(),
            negative: Vec::new(),
            targets: Vec::new(),
        }
    }

    /// Introduce a constant neuron alongside the node.
    #[must_use]
    pub fn with_constant(mut self, uuid: impl Into<String>, bias: f64) -> Self {
        self.constants.push(ConstantSpec::new(uuid, bias));
        self
    }

    /// Add a condition edge.
    #[must_use]
    pub fn with_condition(mut self, uuid: impl Into<String>, weight: f64) -> Self {
        self.condition.push(GraftEdge::new(uuid, weight));
        self
    }

    /// Add a positive-branch edge.
    #[must_use]
    pub fn with_positive(mut self, uuid: impl Into<String>, weight: f64) -> Self {
        self.positive.push(GraftEdge::new(uuid, weight));
        self
    }

    /// Add a negative-branch edge.
    #[must_use]
    pub fn with_negative(mut self, uuid: impl Into<String>, weight: f64) -> Self {
        self.negative.push(GraftEdge::new(uuid, weight));
        self
    }

    /// Add an untyped outward edge from the node to an existing neuron.
    #[must_use]
    pub fn with_target(mut self, uuid: impl Into<String>, weight: f64) -> Self {
        self.targets.push(GraftEdge::new(uuid, weight));
        self
    }

    /// Add an outward edge carrying `role` — the way into one named branch of
    /// an `IF` destination (NEAT-AI-Forests #48).
    #[must_use]
    pub fn with_target_role(
        mut self,
        uuid: impl Into<String>,
        weight: f64,
        role: SynapseType,
    ) -> Self {
        self.targets.push(GraftEdge::with_role(uuid, weight, role));
        self
    }
}

/// Description of an **IDENTITY relay** to graft onto a creature: a hidden
/// neuron that passes its inbound sum on unchanged.
///
/// A creature may not carry two synapses between the same ordered pair of
/// neurons, so a node whose value must reach *two* branches of one `IF`
/// destination needs a second source. The relay is that source: the node feeds
/// one branch directly and the relay carries the same value into the other
/// (NEAT-AI-Forests #48).
#[derive(Debug, Clone, PartialEq)]
pub struct RelaySpec {
    /// UUID for the new relay; must not already exist in the creature.
    pub uuid: String,
    /// Bias added to the relayed sum — `0.0` for a pass-through.
    pub bias: f64,
    /// Untyped inbound edges, summed into the relay.
    pub sources: Vec<GraftEdge>,
    /// Outward edges from the relay to existing non-input, non-constant
    /// neurons, each optionally carrying a role.
    pub targets: Vec<GraftEdge>,
}

impl RelaySpec {
    /// Start a specification for a relay with the given UUID and bias.
    pub fn new(uuid: impl Into<String>, bias: f64) -> Self {
        Self {
            uuid: uuid.into(),
            bias,
            sources: Vec::new(),
            targets: Vec::new(),
        }
    }

    /// Add an inbound edge the relay sums.
    #[must_use]
    pub fn with_source(mut self, uuid: impl Into<String>, weight: f64) -> Self {
        self.sources.push(GraftEdge::new(uuid, weight));
        self
    }

    /// Add an untyped outward edge from the relay to an existing neuron.
    #[must_use]
    pub fn with_target(mut self, uuid: impl Into<String>, weight: f64) -> Self {
        self.targets.push(GraftEdge::new(uuid, weight));
        self
    }

    /// Add an outward edge carrying `role`.
    #[must_use]
    pub fn with_target_role(
        mut self,
        uuid: impl Into<String>,
        weight: f64,
        role: SynapseType,
    ) -> Self {
        self.targets.push(GraftEdge::with_role(uuid, weight, role));
        self
    }
}

/// The common depth-1 case: `feature > threshold ? positive : negative`, added
/// into one existing neuron.
///
/// Expands into a full [`IfNodeSpec`] via [`IfCorrectionSpec::to_node_spec`], so
/// a caller adding a residual correction never restates the synapse-role rule.
#[derive(Debug, Clone, PartialEq)]
pub struct IfCorrectionSpec {
    /// UUID for the new `IF` node; also the prefix of its three constants.
    pub uuid: String,
    /// UUID of the neuron whose activation is compared against `threshold`.
    pub feature_uuid: String,
    /// Split point — the node fires the positive leaf when the feature is
    /// **strictly** greater than this.
    pub threshold: f64,
    /// Leaf value emitted above the threshold.
    pub positive_value: f64,
    /// Leaf value emitted at or below the threshold (often `0.0`).
    pub negative_value: f64,
    /// UUID of the neuron the correction is added into.
    pub target_uuid: String,
    /// Weight of the edge into `target_uuid`.
    pub target_weight: f64,
}

impl IfCorrectionSpec {
    /// Expand into the equivalent low-level [`IfNodeSpec`].
    pub fn to_node_spec(&self) -> IfNodeSpec {
        let condition_one = format!("{}-condition-one", self.uuid);
        let positive_one = format!("{}-positive-one", self.uuid);
        let negative_one = format!("{}-negative-one", self.uuid);
        IfNodeSpec::new(self.uuid.clone(), 0.0)
            .with_constant(condition_one.clone(), GRAFT_CONSTANT_BIAS)
            .with_constant(positive_one.clone(), GRAFT_CONSTANT_BIAS)
            .with_constant(negative_one.clone(), GRAFT_CONSTANT_BIAS)
            .with_condition(self.feature_uuid.clone(), 1.0)
            .with_condition(condition_one, -self.threshold)
            .with_positive(positive_one, self.positive_value)
            .with_negative(negative_one, self.negative_value)
            .with_target(self.target_uuid.clone(), self.target_weight)
    }
}

/// Reasons a graft was refused, or a creature failed the shared gates.
///
/// Every variant means **no creature was produced** — the helper fails closed.
#[derive(Debug)]
pub enum GraftError {
    /// The creature failed a [`crate::creature`] rule, e.g. the observation
    /// width contract or an unknown squash name.
    Creature(CreatureError),
    /// A UUID the graft introduces is already taken, or repeated within the
    /// same specification.
    DuplicateUuid(String),
    /// A branch edge named a source neuron that does not exist.
    UnknownSourceUuid(String),
    /// An outward edge named a destination neuron that does not exist.
    UnknownTargetUuid(String),
    /// An outward edge targeted an input neuron — inputs take no synapses.
    TargetIsInput(String),
    /// An outward edge targeted a constant neuron — constants take no inbound
    /// connections.
    TargetIsConstant(String),
    /// An edge named the grafted node itself.
    SelfEdge(String),
    /// No `condition` edge was supplied, so the node could never branch.
    MissingConditionSynapse,
    /// No `positive` edge was supplied.
    MissingPositiveSynapse,
    /// No `negative` edge was supplied.
    MissingNegativeSynapse,
    /// No outward edge was supplied, so the node could not influence anything.
    ///
    /// In a batched graft ([`graft_if_nodes`]) a node may leave `targets` empty
    /// **provided** a later node in the same batch names it as a branch source;
    /// this is what a node nothing ever reads returns.
    NoTargets,
    /// No inbound edge was supplied, so the node would emit only its bias.
    NoSources,
    /// An inbound edge carried an explicit role. An inbound edge takes its role
    /// from the branch it is listed under, so an explicit one is refused rather
    /// than ignored.
    InboundEdgeHasRole {
        /// Source neuron UUID.
        from: String,
        /// Destination neuron UUID — the grafted node.
        to: String,
    },
    /// The creature lists a constant after the latest position the node can
    /// take, so no placement satisfies both the forward-only reading order and
    /// the `input, constant, hidden, output` neuron order
    /// ([`crate::creature_validate()`] rule 11).
    ConstantAfterPosition {
        /// The constant that cannot precede the node.
        constant: String,
        /// The target that forced the earliest possible position.
        target: String,
    },
    /// Two synapses would connect the same ordered pair of neurons.
    DuplicateEdge {
        /// Source neuron UUID.
        from: String,
        /// Destination neuron UUID.
        to: String,
    },
    /// A weight was `NaN` or infinite.
    NonFiniteWeight {
        /// Source neuron UUID.
        from: String,
        /// Destination neuron UUID.
        to: String,
        /// The offending weight.
        weight: f64,
    },
    /// A bias was `NaN` or infinite.
    NonFiniteBias {
        /// UUID of the neuron carrying the offending bias.
        uuid: String,
    },
    /// No position exists that puts the node after every source and before
    /// every target, so at least one edge would read a stale activation.
    ForwardOrderViolation {
        /// The source that forced the latest possible position.
        source: String,
        /// The target that forced the earliest possible position.
        target: String,
    },
    /// The creature failed [`validate_topology`]; `code` is one of the
    /// `topology_ops` topology codes.
    ///
    /// The creature synapse list carries no ordering contract (compilation
    /// groups by destination), so pairs are sorted before the gate runs and
    /// `index` refers to that sorted order.
    MalformedTopology {
        /// `topology_ops` error code.
        code: i32,
        /// Index of the offending synapse in sorted order.
        index: i32,
    },
    /// The creature failed [`validate_structural_integrity`]; `code` is one of
    /// the `topology_ops` `STRUCTURAL_*` codes and `index` the neuron index.
    MalformedStructure {
        /// `topology_ops` structural error code.
        code: i32,
        /// Index of the offending neuron.
        index: i32,
    },
    /// The graft itself was well formed but the resulting creature failed a
    /// shared gate — reported instead of returning the malformed creature.
    MalformedResult {
        /// `topology_ops` error code.
        code: i32,
        /// Index of the offending synapse or neuron.
        index: i32,
        /// `true` when the failure came from the structural gate.
        structural: bool,
    },
}

impl std::fmt::Display for GraftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraftError::Creature(e) => write!(f, "Creature error: {e}"),
            GraftError::DuplicateUuid(u) => write!(f, "UUID already in use: {u}"),
            GraftError::UnknownSourceUuid(u) => write!(f, "Unknown source neuron UUID: {u}"),
            GraftError::UnknownTargetUuid(u) => write!(f, "Unknown target neuron UUID: {u}"),
            GraftError::TargetIsInput(u) => write!(f, "Synapse targets an input neuron: {u}"),
            GraftError::TargetIsConstant(u) => write!(f, "Synapse targets a constant neuron: {u}"),
            GraftError::SelfEdge(u) => write!(f, "Synapse connects {u} to itself"),
            GraftError::MissingConditionSynapse => {
                write!(f, "IF node has no condition synapse")
            }
            GraftError::MissingPositiveSynapse => write!(f, "IF node has no positive synapse"),
            GraftError::MissingNegativeSynapse => write!(f, "IF node has no negative synapse"),
            GraftError::NoTargets => write!(f, "Grafted node has no outward connection"),
            GraftError::NoSources => write!(f, "Grafted node has no inward connection"),
            GraftError::InboundEdgeHasRole { from, to } => write!(
                f,
                "Inbound synapse from {from} to {to} sets a role; an inbound edge takes its role from the branch it is listed under"
            ),
            GraftError::ConstantAfterPosition { constant, target } => write!(
                f,
                "No position leaves constant {constant} ahead of the node and the node ahead of target {target}"
            ),
            GraftError::DuplicateEdge { from, to } => {
                write!(f, "Duplicate synapse from {from} to {to}")
            }
            GraftError::NonFiniteWeight { from, to, weight } => {
                write!(f, "Non-finite weight {weight} from {from} to {to}")
            }
            GraftError::NonFiniteBias { uuid } => write!(f, "Non-finite bias on neuron {uuid}"),
            GraftError::ForwardOrderViolation { source, target } => write!(
                f,
                "No forward-only position: source {source} is evaluated after target {target}"
            ),
            GraftError::MalformedTopology { code, index } => {
                write!(f, "Topology validation failed (code {code}, index {index})")
            }
            GraftError::MalformedStructure { code, index } => write!(
                f,
                "Structural validation failed (code {code}, index {index})"
            ),
            GraftError::MalformedResult {
                code,
                index,
                structural,
            } => {
                let gate = if *structural {
                    "structural"
                } else {
                    "topology"
                };
                write!(
                    f,
                    "Grafted creature failed {gate} validation (code {code}, index {index})"
                )
            }
        }
    }
}

impl std::error::Error for GraftError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GraftError::Creature(e) => Some(e),
            _ => None,
        }
    }
}

impl From<CreatureError> for GraftError {
    fn from(e: CreatureError) -> Self {
        GraftError::Creature(e)
    }
}

/// Map every neuron UUID to the index compilation will give it.
///
/// Mirrors [`crate::creature::compile_creature`] exactly: inputs take
/// `0..input` under their `input-N` names, then the listed neurons follow in
/// order (a listed neuron reusing an `input-N` name wins, as it does there).
fn index_map(creature: &CreatureExport) -> HashMap<String, usize> {
    let mut map = HashMap::with_capacity(creature.input + creature.neurons.len());
    for i in 0..creature.input {
        map.insert(format!("input-{i}"), i);
    }
    for (j, neuron) in creature.neurons.iter().enumerate() {
        map.insert(neuron.uuid.clone(), creature.input + j);
    }
    map
}

/// Run the fleet's shared width, topology and structural gates over a creature.
///
/// Reuses [`validate_creature_width`], [`validate_topology`] and
/// [`validate_structural_integrity`] rather than restating their rules, so a
/// creature that passes here is one the compiler and the WASM consumers accept.
///
/// The ordering gate only runs for `forwardOnly` creatures: a recurrent creature
/// legitimately carries backward edges, which that gate rejects by design.
pub fn validate_creature_topology(creature: &CreatureExport) -> Result<(), GraftError> {
    validate_creature_width(creature)?;

    let map = index_map(creature);
    let num_neurons = creature.input + creature.neurons.len();

    let mut biases = vec![0.0f64; num_neurons];
    let mut is_constant = vec![0u8; num_neurons];
    let mut squash_types = vec![SquashType::Identity as u8; num_neurons];
    for (j, neuron) in creature.neurons.iter().enumerate() {
        let idx = creature.input + j;
        biases[idx] = neuron.bias;
        is_constant[idx] = u8::from(neuron.neuron_type == "constant");
        squash_types[idx] =
            parse_squash_name(neuron.squash.as_deref().unwrap_or("IDENTITY"))? as u8;
    }

    let mut edges: Vec<(u32, u32, u8)> = Vec::with_capacity(creature.synapses.len());
    for synapse in &creature.synapses {
        let from = *map
            .get(&synapse.from_uuid)
            .ok_or_else(|| GraftError::UnknownSourceUuid(synapse.from_uuid.clone()))?;
        let to = *map
            .get(&synapse.to_uuid)
            .ok_or_else(|| GraftError::UnknownTargetUuid(synapse.to_uuid.clone()))?;
        edges.push((
            from as u32,
            to as u32,
            parse_synapse_type(synapse.synapse_type.as_deref()) as u8,
        ));
    }
    edges.sort_unstable();

    let from_indices: Vec<u32> = edges.iter().map(|e| e.0).collect();
    let to_indices: Vec<u32> = edges.iter().map(|e| e.1).collect();
    let synapse_types: Vec<u8> = edges.iter().map(|e| e.2).collect();

    if creature.forward_only {
        let codes = validate_topology(&from_indices, &to_indices);
        if codes[0] != VALID {
            return Err(GraftError::MalformedTopology {
                code: codes[0],
                index: codes[1],
            });
        }
    }

    let codes = validate_structural_integrity(
        &from_indices,
        &to_indices,
        &is_constant,
        &squash_types,
        &biases,
        creature.input as u32,
        creature.output as u32,
        &synapse_types,
    );
    if codes[0] != STRUCTURAL_VALID {
        return Err(GraftError::MalformedStructure {
            code: codes[0],
            index: codes[1],
        });
    }

    Ok(())
}

/// One node to place: everything the placement rules and the builder need,
/// whatever kind of node the caller described.
///
/// Both an `IF` node ([`IfNodeSpec`]) and an IDENTITY relay ([`RelaySpec`])
/// reduce to this, so the name, finiteness, duplicate-edge, placement and
/// ordering rules have one home rather than one copy per node kind.
struct NodePlan<'a> {
    /// UUID of the new neuron.
    uuid: &'a str,
    /// Bias of the new neuron.
    bias: f64,
    /// Activation of the new neuron.
    squash: SquashType,
    /// Constants the node introduces alongside itself.
    constants: &'a [ConstantSpec],
    /// Inbound edges paired with the role each will carry.
    inbound: Vec<(&'a GraftEdge, SynapseType)>,
    /// Outward edges; each carries its own [`GraftEdge::role`].
    targets: &'a [GraftEdge],
}

/// Reduce an [`IfNodeSpec`] to a [`NodePlan`], applying the `IF`-only rules:
/// all three roles present, and no inbound edge naming a role of its own.
fn if_plan(spec: &IfNodeSpec) -> Result<NodePlan<'_>, GraftError> {
    if spec.condition.is_empty() {
        return Err(GraftError::MissingConditionSynapse);
    }
    if spec.positive.is_empty() {
        return Err(GraftError::MissingPositiveSynapse);
    }
    if spec.negative.is_empty() {
        return Err(GraftError::MissingNegativeSynapse);
    }
    let mut inbound =
        Vec::with_capacity(spec.condition.len() + spec.positive.len() + spec.negative.len());
    for (role, edges) in [
        (SynapseType::Condition, &spec.condition),
        (SynapseType::Positive, &spec.positive),
        (SynapseType::Negative, &spec.negative),
    ] {
        for edge in edges {
            inbound.push((edge, role));
        }
    }
    Ok(NodePlan {
        uuid: &spec.uuid,
        bias: spec.bias,
        squash: SquashType::If,
        constants: &spec.constants,
        inbound,
        targets: &spec.targets,
    })
}

/// Reduce a [`RelaySpec`] to a [`NodePlan`]: an IDENTITY neuron whose inbound
/// edges are all untyped.
fn relay_plan(spec: &RelaySpec) -> NodePlan<'_> {
    NodePlan {
        uuid: &spec.uuid,
        bias: spec.bias,
        squash: SquashType::Identity,
        constants: &[],
        inbound: spec
            .sources
            .iter()
            .map(|edge| (edge, SynapseType::Standard))
            .collect(),
        targets: &spec.targets,
    }
}

/// Check a plan against the creature, choose the node's position, and build the
/// resulting creature — everything but the final validation.
///
/// `require_targets` is `false` only inside [`graft_if_nodes`], where a node may
/// leave its outward edge to a later node in the same batch; a node nothing
/// reads is still refused, by that later node's absence.
fn place_and_build(
    creature: &CreatureExport,
    plan: &NodePlan<'_>,
    require_targets: bool,
) -> Result<CreatureExport, GraftError> {
    let map = index_map(creature);

    // Names this graft introduces must be new, and unique among themselves.
    let mut introduced: HashSet<&str> = HashSet::with_capacity(plan.constants.len() + 1);
    let new_names =
        std::iter::once(plan.uuid).chain(plan.constants.iter().map(|c| c.uuid.as_str()));
    for uuid in new_names {
        if map.contains_key(uuid) || !introduced.insert(uuid) {
            return Err(GraftError::DuplicateUuid(uuid.to_string()));
        }
    }

    if !plan.bias.is_finite() {
        return Err(GraftError::NonFiniteBias {
            uuid: plan.uuid.to_string(),
        });
    }
    for constant in plan.constants {
        if !constant.bias.is_finite() {
            return Err(GraftError::NonFiniteBias {
                uuid: constant.uuid.clone(),
            });
        }
    }

    if plan.inbound.is_empty() {
        return Err(GraftError::NoSources);
    }
    if require_targets && plan.targets.is_empty() {
        return Err(GraftError::NoTargets);
    }

    // Earliest position that still leaves every source before the new node.
    let mut earliest = 0usize;
    let mut latest_source: Option<&str> = None;
    let mut seen_sources: HashSet<&str> = HashSet::new();
    for (edge, _) in &plan.inbound {
        if edge.role != SynapseType::Standard {
            return Err(GraftError::InboundEdgeHasRole {
                from: edge.uuid.clone(),
                to: plan.uuid.to_string(),
            });
        }
        if edge.uuid == plan.uuid {
            return Err(GraftError::SelfEdge(plan.uuid.to_string()));
        }
        if !edge.weight.is_finite() {
            return Err(GraftError::NonFiniteWeight {
                from: edge.uuid.clone(),
                to: plan.uuid.to_string(),
                weight: edge.weight,
            });
        }
        if !seen_sources.insert(edge.uuid.as_str()) {
            return Err(GraftError::DuplicateEdge {
                from: edge.uuid.clone(),
                to: plan.uuid.to_string(),
            });
        }
        if introduced.contains(edge.uuid.as_str()) {
            // A constant this graft introduces is placed before the node.
            continue;
        }
        let index = *map
            .get(edge.uuid.as_str())
            .ok_or_else(|| GraftError::UnknownSourceUuid(edge.uuid.clone()))?;
        if index >= creature.input && index - creature.input + 1 > earliest {
            earliest = index - creature.input + 1;
            latest_source = Some(edge.uuid.as_str());
        }
    }

    // Latest position that still leaves every target after the new node.
    let mut latest = creature.neurons.len();
    let mut earliest_target: Option<&str> = None;
    let mut seen_targets: HashSet<&str> = HashSet::new();
    for edge in plan.targets {
        if edge.uuid == plan.uuid {
            return Err(GraftError::SelfEdge(plan.uuid.to_string()));
        }
        if !edge.weight.is_finite() {
            return Err(GraftError::NonFiniteWeight {
                from: plan.uuid.to_string(),
                to: edge.uuid.clone(),
                weight: edge.weight,
            });
        }
        if !seen_targets.insert(edge.uuid.as_str()) {
            return Err(GraftError::DuplicateEdge {
                from: plan.uuid.to_string(),
                to: edge.uuid.clone(),
            });
        }
        if introduced.contains(edge.uuid.as_str()) {
            return Err(GraftError::TargetIsConstant(edge.uuid.clone()));
        }
        let index = *map
            .get(edge.uuid.as_str())
            .ok_or_else(|| GraftError::UnknownTargetUuid(edge.uuid.clone()))?;
        if index < creature.input {
            return Err(GraftError::TargetIsInput(edge.uuid.clone()));
        }
        let position = index - creature.input;
        if creature.neurons[position].neuron_type == "constant" {
            return Err(GraftError::TargetIsConstant(edge.uuid.clone()));
        }
        if position < latest {
            latest = position;
            earliest_target = Some(edge.uuid.as_str());
        }
    }

    // A constant may never follow a hidden neuron, so the node is listed after
    // every constant the creature already carries — not merely after the last
    // one it reads, which would leave a later constant behind a hidden neuron
    // ([`crate::creature_validate()`] rule 11).
    let last_constant = creature
        .neurons
        .iter()
        .rposition(|n| n.neuron_type == "constant");
    if let Some(last) = last_constant {
        earliest = earliest.max(last + 1);
        if last + 1 > latest {
            return Err(GraftError::ConstantAfterPosition {
                constant: creature.neurons[last].uuid.clone(),
                target: earliest_target.unwrap_or_default().to_string(),
            });
        }
    }

    if earliest > latest {
        return Err(GraftError::ForwardOrderViolation {
            source: latest_source.unwrap_or_default().to_string(),
            target: earliest_target.unwrap_or_default().to_string(),
        });
    }

    Ok(build_grafted(creature, plan, earliest))
}

/// Report a creature that failed a shared gate as [`GraftError::MalformedResult`]
/// — the graft was well formed but what it assembled was not, so the creature is
/// never returned.
fn validated(grafted: CreatureExport) -> Result<CreatureExport, GraftError> {
    match validate_creature_topology(&grafted) {
        Ok(()) => Ok(grafted),
        Err(GraftError::MalformedTopology { code, index }) => Err(GraftError::MalformedResult {
            code,
            index,
            structural: false,
        }),
        Err(GraftError::MalformedStructure { code, index }) => Err(GraftError::MalformedResult {
            code,
            index,
            structural: true,
        }),
        Err(other) => Err(other),
    }
}

/// Graft one `IF` node (and any constants it declares) onto a creature.
///
/// Returns a **new** creature; the input is never modified. Placement is chosen
/// so the node is evaluated after every source and before every target, which
/// preserves the `forwardOnly` reading order the compiled forward pass relies
/// on; any constants the spec declares are listed ahead of every hidden neuron,
/// and the synapse list comes back in canonical `(from, to)` order, so the
/// result satisfies [`crate::creature_validate()`] as well. The creature is put
/// through [`validate_creature_topology`] before it is returned, so a malformed
/// creature is never emitted.
///
/// # Errors
///
/// Returns a [`GraftError`] when the base creature is already malformed, when
/// the specification names an unknown or duplicate neuron, when any `IF` role
/// is missing, when a weight or bias is not finite, or when no position exists
/// that keeps every edge pointing forwards.
pub fn graft_if_node(
    creature: &CreatureExport,
    spec: &IfNodeSpec,
) -> Result<CreatureExport, GraftError> {
    validate_creature_topology(creature)?;
    let plan = if_plan(spec)?;
    validated(place_and_build(creature, &plan, true)?)
}

/// Graft an IDENTITY relay — a hidden neuron that passes its inbound sum on.
///
/// The way to reach a second branch of a destination the source already feeds:
/// a creature may not carry two synapses between the same ordered pair, so the
/// relay supplies the second, distinct source (NEAT-AI-Forests #48). Same
/// placement, ordering and validation contract as [`graft_if_node`].
///
/// # Errors
///
/// Returns a [`GraftError`] when the relay names an unknown or duplicate
/// neuron, when it has no inbound or no outward edge, when a weight or bias is
/// not finite, or when no position exists that keeps every edge pointing
/// forwards.
pub fn graft_relay_node(
    creature: &CreatureExport,
    spec: &RelaySpec,
) -> Result<CreatureExport, GraftError> {
    validate_creature_topology(creature)?;
    let plan = relay_plan(spec);
    validated(place_and_build(creature, &plan, true)?)
}

/// Graft a batch of `IF` nodes as one all-or-nothing change, where a node may
/// feed another node in the same batch.
///
/// Unlike [`graft_if_tree`], which validates after every node and so needs each
/// one to be complete on its own, a node here may leave `targets` empty
/// **provided** a later node in the batch names it as a branch source — the
/// post-order shape a nested decision tree has, where the child exists to feed
/// the parent that does not exist yet (NEAT-AI-Forests #48). Only the assembled
/// creature is validated, and it is validated once.
///
/// All or nothing: the first failure returns its [`GraftError`] and no partial
/// creature escapes. An empty batch returns the validated base creature.
///
/// # Errors
///
/// Returns the first [`GraftError`] any node produces, [`GraftError::NoTargets`]
/// for a node nothing in the batch ever reads, or the base creature's own
/// validation error.
pub fn graft_if_nodes(
    creature: &CreatureExport,
    specs: &[IfNodeSpec],
) -> Result<CreatureExport, GraftError> {
    validate_creature_topology(creature)?;
    let mut current = creature.clone();
    for (i, spec) in specs.iter().enumerate() {
        // A later node naming this one as a branch source supplies its outward
        // edge, so this node may carry none of its own.
        let fed_to_a_later_node = specs[i + 1..].iter().any(|later| {
            later
                .condition
                .iter()
                .chain(&later.positive)
                .chain(&later.negative)
                .any(|edge| edge.uuid == spec.uuid)
        });
        let plan = if_plan(spec)?;
        current = place_and_build(&current, &plan, !fed_to_a_later_node)?;
    }
    validated(current)
}

/// Assemble the grafted creature: the constants ahead of every hidden neuron,
/// the node at `position`, and the whole synapse list left in canonical
/// `(from, to)` order.
///
/// Both placements are what [`crate::creature_validate()`] requires of a valid
/// creature — neurons listed `input, constant, hidden, output` (rule 11) and
/// synapses sorted ascending by resolved index (rule 25) — so a grafted
/// creature satisfies the shared validator, not merely the compiler.
fn build_grafted(
    creature: &CreatureExport,
    plan: &NodePlan<'_>,
    position: usize,
) -> CreatureExport {
    // A constant may never follow a hidden neuron, so the constants go in front
    // of the first non-constant rather than beside the node, which can legally
    // sit after several hidden neurons.
    let constants_at = creature
        .neurons
        .iter()
        .position(|n| n.neuron_type != "constant")
        .unwrap_or(creature.neurons.len())
        .min(position);
    let mut neurons = Vec::with_capacity(creature.neurons.len() + plan.constants.len() + 1);
    neurons.extend_from_slice(&creature.neurons[..constants_at]);
    for constant in plan.constants {
        neurons.push(NeuronExport {
            id: None,
            neuron_type: "constant".to_string(),
            uuid: constant.uuid.clone(),
            bias: constant.bias,
            squash: None,
        });
    }
    neurons.extend_from_slice(&creature.neurons[constants_at..position]);
    neurons.push(NeuronExport {
        id: None,
        neuron_type: "hidden".to_string(),
        uuid: plan.uuid.to_string(),
        bias: plan.bias,
        squash: Some(squash_name_from(plan.squash).to_string()),
    });
    neurons.extend_from_slice(&creature.neurons[position..]);

    let mut synapses = creature.synapses.clone();
    synapses.reserve(plan.inbound.len() + plan.targets.len());
    for (edge, role) in &plan.inbound {
        synapses.push(SynapseExport {
            from_uuid: edge.uuid.clone(),
            to_uuid: plan.uuid.to_string(),
            weight: edge.weight,
            synapse_type: synapse_type_name_from(*role).map(str::to_string),
        });
    }
    for edge in plan.targets {
        synapses.push(SynapseExport {
            from_uuid: plan.uuid.to_string(),
            to_uuid: edge.uuid.clone(),
            weight: edge.weight,
            synapse_type: synapse_type_name_from(edge.role).map(str::to_string),
        });
    }

    let mut grafted = CreatureExport {
        memetic: None,
        input: creature.input,
        output: creature.output,
        neurons,
        synapses,
        semantic_version: creature.semantic_version.clone(),
        forward_only: creature.forward_only,
    };
    sort_synapses_canonically(&mut grafted);
    grafted
}

/// Leave a creature's synapse list in canonical wire order — ascending by
/// `(from index, to index)`, the order [`crate::creature_validate()`] rule 25
/// requires.
///
/// Appending a graft's synapses to the base creature's list leaves them out of
/// order, so the assembled creature is re-sorted rather than emitted piecemeal.
/// An endpoint naming no neuron sorts last (`u32::MAX`) instead of being
/// dropped, so the validator still reports it rather than the graft quietly
/// reordering a broken creature. The sort is stable, so synapses sharing a pair
/// keep their relative order and remain reportable as duplicates.
pub(crate) fn sort_synapses_canonically(creature: &mut CreatureExport) {
    let map = index_map(creature);
    let resolve = |uuid: &str| -> usize { map.get(uuid).copied().unwrap_or(usize::MAX) };
    creature
        .synapses
        .sort_by_key(|s| (resolve(&s.from_uuid), resolve(&s.to_uuid)));
}

/// Graft a sequence of `IF` nodes, each able to reference the ones before it.
///
/// All or nothing: the first failure returns its [`GraftError`] and no partial
/// creature escapes, because each step builds on the previous step's result and
/// only the final creature is returned.
///
/// # Errors
///
/// Returns the first [`GraftError`] any node produces, or the base creature's
/// own validation error when `specs` is empty.
pub fn graft_if_tree(
    creature: &CreatureExport,
    specs: &[IfNodeSpec],
) -> Result<CreatureExport, GraftError> {
    validate_creature_topology(creature)?;
    let mut current = creature.clone();
    for spec in specs {
        current = graft_if_node(&current, spec)?;
    }
    Ok(current)
}

/// Graft a depth-1 `IF` correction — `feature > threshold ? positive : negative`
/// added into one existing neuron.
///
/// Thin wrapper over [`graft_if_node`] and [`IfCorrectionSpec::to_node_spec`],
/// so callers adding a residual correction do not restate the synapse-role rule.
///
/// # Errors
///
/// Same conditions as [`graft_if_node`].
pub fn graft_if_correction(
    creature: &CreatureExport,
    spec: &IfCorrectionSpec,
) -> Result<CreatureExport, GraftError> {
    graft_if_node(creature, &spec.to_node_spec())
}
