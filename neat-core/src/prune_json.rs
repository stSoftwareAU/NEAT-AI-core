//! The JSON ABI the pruning rewrites cross the WASM boundary on (Issue #592).
//!
//! [`crate::prune_neuron::prune_neuron`] and
//! [`crate::prune_synapse::prune_synapse`] are the fleet's one
//! implementation of "remove this and give me back something I can score".
//! NEAT-AI calls them from TypeScript, so they need a wire form — and a second
//! implementation behind that wire form is exactly what Issue #587 exists to
//! prevent. This module is therefore a **translation layer and nothing else**:
//! it parses a request, calls the native function, and writes the result down.
//! No rule, no fold and no cascade is decided here.
//!
//! ```mermaid
//! flowchart LR
//!     TS["NEAT-AI (TypeScript)"] -->|"JSON request"| W["wasm_exports::prune_neuron<br/>wasm_exports::prune_synapse"]
//!     W --> J["prune_json — parse, call, write"]
//!     N["native caller (Rust)"] --> J
//!     J --> P["prune_neuron / prune_synapse<br/>the one implementation"]
//!     P --> J
//!     J -->|"JSON response"| TS
//! ```
//!
//! # Requests
//!
//! ```json
//! { "creature": <CreatureExport>, "uuid": "h-1",
//!   "stats": { "meanActivation": 0.5, "variance": 0.04,
//!              "proxy": { "uuid": "h-2", "meanActivation": 0.4,
//!                         "variance": 0.02, "covariance": 0.01 } } }
//! ```
//!
//! ```json
//! { "creature": <CreatureExport>,
//!   "synapse": { "fromUUID": "h-a", "toUUID": "if-1", "type": "negative" },
//!   "stats": { "meanActivation": 0.5 } }
//! ```
//!
//! `creature` is the [`CreatureExport`] wire shape NEAT-AI already exchanges,
//! so a creature file goes in and a creature file comes out. `stats` is
//! optional and is the **caller's** measurement — this crate only ever uses
//! what it is handed (Issue #587). `type` is optional and defaults to the
//! untyped role; an unknown spelling is a boundary fault rather than a silent
//! `"standard"`, because a request for `"POSITIVE"` that quietly removed the
//! untyped edge would delete structure the caller never named.
//!
//! Every key is `deny_unknown_fields`: a payload writing `"statz"` must not be
//! answered as though no statistics were supplied.
//!
//! # Responses
//!
//! ```json
//! { "ok": true, "creature": <CreatureExport>, "transform": "exact", "passes": 2,
//!   "removedNeuron": "h-1", "removedSynapses": [ … ], "cascadeNeurons": [ … ],
//!   "biasFolds": [ … ], "weightShares": [ … ], "uncompensated": [ … ] }
//! ```
//!
//! ```json
//! { "ok": false, "failure": { "reason": "PROTECTED_NEURON",
//!                             "message": "Neuron output-0 is a output node …",
//!                             "malformed": false } }
//! ```
//!
//! Empty report lists are omitted rather than written as `[]`, so a small
//! rewrite answers with a small payload. `transform` is `"exact"` or
//! `"approximate"` — the same honest label
//! [`PruneResult::transform`](crate::prune_neuron::PruneResult::transform)
//! carries natively, never inferred by the host.
//!
//! # A refusal is not a fault, and neither may panic
//!
//! `ok: false` covers two different things and the wire keeps them apart:
//!
//! | `malformed` | What happened |
//! |---|---|
//! | `false` | the request was understood and **refused** — an unknown UUID, a protected neuron, an unusable statistic. A verdict on the request. |
//! | `true` | the payload never reached the rewrite. Not a verdict on anything. |
//!
//! A panic on wasm aborts the module and takes the host's session with it, and
//! `catch_unwind` is unavailable there, so the boundary must not be able to
//! panic. Malformed input answers with [`MALFORMED_REQUEST`] — the same
//! convention [`mod@crate::creature_validate_json`] uses — and an oversized
//! creature is refused before the walk allocates per neuron.
//!
//! One malformed *message* is target-dependent and deliberately left so: a
//! declared width past `u32` fits `usize` natively but not on wasm32, where
//! serde refuses it while reading rather than the ceiling refusing it after.
//! Both answer `malformed: true` and neither reaches a rewrite, so the contract
//! holds; only the wording differs, which is why no parity case pins it.
//!
//! # Native and WASM answer the same thing
//!
//! There is one implementation and two entry surfaces:
//! [`prune_neuron_json`] here, and the `#[wasm_bindgen]` rename over it in
//! `crate::wasm_exports` (wasm builds only). [`prune_golden_cases`] is the
//! shared record that
//! keeps the two honest — the requests, and the answers native produces for
//! them, committed at [`GOLDEN_PATH`]. `neat-core/tests/prune_json.rs` grades
//! the native side against it and `scripts/check_wasm_prune_parity.ts` drives
//! the built bundle through the same requests in CI.

use serde::{Deserialize, Serialize};

use crate::creature::{CreatureExport, synapse_type_name_from};
use crate::creature_validate_json::{MALFORMED_REQUEST, oversized_detail};
use crate::prune_cleanup::{StaticIfRewrite, SynapseKey};
use crate::prune_neuron::{
    BiasFold, ProxyStats, PruneError, PruneResult, PruneStats, TransformClass, UncompensatedReason,
    UncompensatedTarget, WeightShare, prune_neuron,
};
use crate::prune_synapse::prune_synapse;
use crate::synapse_type::SynapseType;

/// The `reason` codes a failure names, as the host reads them.
///
/// One code per [`PruneError`] variant plus the boundary fault, so a host
/// branches on a stable token rather than on message text.
pub mod reason {
    /// No neuron of the creature carries the requested UUID.
    pub const UNKNOWN_NEURON: &str = "UNKNOWN_NEURON";
    /// No synapse of the creature carries the requested `(from, to, role)`.
    pub const UNKNOWN_SYNAPSE: &str = "UNKNOWN_SYNAPSE";
    /// The neuron exists but is an observation, output or constant node.
    pub const PROTECTED_NEURON: &str = "PROTECTED_NEURON";
    /// A neuron declared a type outside `hidden | output | constant`.
    pub const UNKNOWN_NEURON_TYPE: &str = "UNKNOWN_NEURON_TYPE";
    /// A supplied statistic was not a finite number.
    pub const NON_FINITE_STATISTIC: &str = "NON_FINITE_STATISTIC";
    /// A supplied variance was negative.
    pub const NEGATIVE_VARIANCE: &str = "NEGATIVE_VARIANCE";
    /// The creature carries no surviving neuron with the proxy's UUID.
    pub const UNKNOWN_PROXY: &str = "UNKNOWN_PROXY";
    /// The proxy's activation never moved, so its regression slope is undefined.
    pub const DEGENERATE_PROXY: &str = "DEGENERATE_PROXY";
    /// The covariance is larger than the supplied variances allow.
    pub const INCONSISTENT_COVARIANCE: &str = "INCONSISTENT_COVARIANCE";
    /// The proxy does not already feed a target the removal costs.
    pub const MISSING_PROXY_EDGE: &str = "MISSING_PROXY_EDGE";
    /// What remained could not be cleaned into a valid canonical creature.
    pub const CLEANUP_FAILED: &str = "CLEANUP_FAILED";
    /// The payload never reached the rewrite — see [`super::MALFORMED_REQUEST`].
    pub const MALFORMED_REQUEST: &str = "MALFORMED_REQUEST";
}

/// Statistics as they arrive on the wire.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StatsJson {
    mean_activation: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    variance: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    proxy: Option<ProxyStatsJson>,
}

/// A correlated survivor's statistics, as they arrive on the wire.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProxyStatsJson {
    uuid: String,
    mean_activation: f64,
    variance: f64,
    covariance: f64,
}

impl From<StatsJson> for PruneStats {
    fn from(stats: StatsJson) -> Self {
        Self {
            mean_activation: stats.mean_activation,
            variance: stats.variance,
            proxy: stats.proxy.map(|proxy| ProxyStats {
                uuid: proxy.uuid,
                mean_activation: proxy.mean_activation,
                variance: proxy.variance,
                covariance: proxy.covariance,
            }),
        }
    }
}

/// A `(from, to, role)` triple on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SynapseKeyJson {
    /// Wire UUID of the source neuron.
    #[serde(rename = "fromUUID")]
    pub from_uuid: String,
    /// Wire UUID of the target neuron.
    #[serde(rename = "toUUID")]
    pub to_uuid: String,
    /// `"standard"`, `"condition"`, `"negative"` or `"positive"`; absent means
    /// the untyped role.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub synapse_type: Option<String>,
}

impl From<&SynapseKey> for SynapseKeyJson {
    fn from(key: &SynapseKey) -> Self {
        Self {
            from_uuid: key.from_uuid.clone(),
            to_uuid: key.to_uuid.clone(),
            synapse_type: Some(role_name(key.role).to_string()),
        }
    }
}

/// The wire spelling of a role — `"standard"` for the untyped one.
fn role_name(role: SynapseType) -> &'static str {
    synapse_type_name_from(role).unwrap_or("standard")
}

/// Read a role the wire named, refusing a spelling this crate does not carry.
///
/// Deliberately **not** [`crate::creature::parse_synapse_type`], whose unknown
/// arm is `Standard`: a request naming `"POSITIVE"` must be a boundary fault,
/// not a silent request to remove the untyped edge instead.
fn parse_role(spelling: Option<&str>) -> Result<SynapseType, String> {
    match spelling {
        None | Some("standard") => Ok(SynapseType::Standard),
        Some("condition") => Ok(SynapseType::Condition),
        Some("negative") => Ok(SynapseType::Negative),
        Some("positive") => Ok(SynapseType::Positive),
        Some(other) => Err(format!(
            "synapse type '{other}' is not one of standard, condition, negative, positive"
        )),
    }
}

/// A neuron-removal request.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct NeuronRequest {
    creature: CreatureExport,
    uuid: String,
    #[serde(default)]
    stats: Option<StatsJson>,
}

/// A synapse-removal request.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SynapseRequest {
    creature: CreatureExport,
    synapse: SynapseKeyJson,
    #[serde(default)]
    stats: Option<StatsJson>,
}

/// One target's bias fold, on the wire.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BiasFoldJson {
    /// Wire UUID of the target whose bias moved.
    #[serde(rename = "targetUUID")]
    pub target_uuid: String,
    /// Total weight the removal took out of that target.
    pub weight_sum: f64,
    /// What was added to the target's bias.
    pub delta: f64,
    /// True when the folded value is what the creature computed on every record.
    pub exact: bool,
    /// Variance of what the compensation could not carry, when the caller
    /// supplied the variance it derives from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub residual_variance: Option<f64>,
}

impl From<&BiasFold> for BiasFoldJson {
    fn from(fold: &BiasFold) -> Self {
        Self {
            target_uuid: fold.target_uuid.clone(),
            weight_sum: fold.weight_sum,
            delta: fold.delta,
            exact: fold.exact,
            residual_variance: fold.residual_variance,
        }
    }
}

/// Weight moved onto a correlated survivor's edge, on the wire.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct WeightShareJson {
    /// Wire UUID of the survivor carrying the correlated part.
    #[serde(rename = "fromUUID")]
    pub from_uuid: String,
    /// Wire UUID of the target it feeds.
    #[serde(rename = "toUUID")]
    pub to_uuid: String,
    /// What was added to that edge's weight.
    pub delta: f64,
}

impl From<&WeightShare> for WeightShareJson {
    fn from(share: &WeightShare) -> Self {
        Self {
            from_uuid: share.from_uuid.clone(),
            to_uuid: share.to_uuid.clone(),
            delta: share.delta,
        }
    }
}

/// A target left carrying the removal without compensation, on the wire.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UncompensatedJson {
    /// Wire UUID of the target.
    #[serde(rename = "targetUUID")]
    pub target_uuid: String,
    /// Role the removed edges played at that target.
    #[serde(rename = "type")]
    pub synapse_type: String,
    /// Total weight the removal took out of that role of that target.
    pub weight_sum: f64,
    /// The target's squash, which is what makes an aggregate uncompensable.
    pub squash: String,
    /// `"NO_STATISTICS"` or `"AGGREGATE_TARGET"`.
    pub reason: String,
}

impl From<&UncompensatedTarget> for UncompensatedJson {
    fn from(target: &UncompensatedTarget) -> Self {
        Self {
            target_uuid: target.target_uuid.clone(),
            synapse_type: role_name(target.role).to_string(),
            weight_sum: target.weight_sum,
            squash: target.squash.to_string(),
            reason: match target.reason {
                UncompensatedReason::NoStatistics => "NO_STATISTICS",
                UncompensatedReason::AggregateTarget => "AGGREGATE_TARGET",
            }
            .to_string(),
        }
    }
}

/// An `IF` flattened to the branch its condition always takes, on the wire.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct StaticIfJson {
    /// Wire UUID of the `IF` neuron.
    pub uuid: String,
    /// The branch that survived — `"positive"` or `"negative"`.
    pub branch: String,
}

impl From<&StaticIfRewrite> for StaticIfJson {
    fn from(rewrite: &StaticIfRewrite) -> Self {
        Self {
            uuid: rewrite.uuid.clone(),
            branch: role_name(rewrite.branch).to_string(),
        }
    }
}

/// Why a prune produced no creature, on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PruneFailureJson {
    /// One of the [`reason`] codes.
    pub reason: String,
    /// The human-readable message, from the native error's own `Display`.
    pub message: String,
    /// `true` when the payload never reached the rewrite.
    pub malformed: bool,
}

/// What [`prune_neuron_json`] and [`prune_synapse_json`] answer with.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PruneResponse {
    /// `true` when a creature came back.
    pub ok: bool,
    /// The canonical, validated creature — present only when `ok`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creature: Option<CreatureExport>,
    /// `"exact"` or `"approximate"` — present only when `ok`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<String>,
    /// How many cleanup passes the fixed point took — present only when `ok`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passes: Option<usize>,
    /// Wire UUID of the neuron the request named, for a neuron removal.
    #[serde(
        rename = "removedNeuron",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub removed_neuron: Option<String>,
    /// The edges the request itself took.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed_synapses: Vec<SynapseKeyJson>,
    /// Neurons the cleanup cascade removed on top of the request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cascade_neurons: Vec<String>,
    /// Synapses the cascade removed alongside them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cascade_synapses: Vec<SynapseKeyJson>,
    /// Hidden neurons the cascade folded into constant support.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folded_neurons: Vec<String>,
    /// `IF` neurons downgraded to `IDENTITY` because a role went with the
    /// removal — the one rewrite that is not exact.
    #[serde(
        rename = "downgradedIfNeurons",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub downgraded_if_neurons: Vec<String>,
    /// `IF` neurons flattened to the branch their condition always takes.
    #[serde(
        rename = "staticIfNeurons",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub static_if_neurons: Vec<StaticIfJson>,
    /// Zero-weight support edges added to give an `IF` back an emptied role.
    #[serde(
        rename = "restoredIfRoles",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub restored_if_roles: Vec<SynapseKeyJson>,
    /// The mean folds applied, one per compensated target.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bias_folds: Vec<BiasFoldJson>,
    /// The correlated-survivor shares applied.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weight_shares: Vec<WeightShareJson>,
    /// Targets that carried the removal with nothing folded back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uncompensated: Vec<UncompensatedJson>,
    /// Why no creature came back — present only when not `ok`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<PruneFailureJson>,
}

impl PruneResponse {
    /// The answer for a successful rewrite.
    ///
    /// The result is **destructured**, not read field by field: a field added
    /// to [`PruneResult`] then fails to compile here rather than silently
    /// never reaching the wire.
    fn from_result(result: &PruneResult) -> Self {
        let PruneResult {
            creature,
            removed_neuron,
            removed_synapses,
            cascade_neurons,
            cascade_synapses,
            folded_neurons,
            downgraded_if_neurons,
            static_if_neurons,
            restored_if_roles,
            bias_folds,
            weight_shares,
            uncompensated,
            transform,
            passes,
        } = result;

        Self {
            ok: true,
            creature: Some(creature.clone()),
            transform: Some(
                match transform {
                    TransformClass::Exact => "exact",
                    TransformClass::Approximate => "approximate",
                }
                .to_string(),
            ),
            passes: Some(*passes),
            removed_neuron: removed_neuron.clone(),
            removed_synapses: removed_synapses.iter().map(Into::into).collect(),
            cascade_neurons: cascade_neurons.clone(),
            cascade_synapses: cascade_synapses.iter().map(Into::into).collect(),
            folded_neurons: folded_neurons.clone(),
            downgraded_if_neurons: downgraded_if_neurons.clone(),
            static_if_neurons: static_if_neurons.iter().map(Into::into).collect(),
            restored_if_roles: restored_if_roles.iter().map(Into::into).collect(),
            bias_folds: bias_folds.iter().map(Into::into).collect(),
            weight_shares: weight_shares.iter().map(Into::into).collect(),
            uncompensated: uncompensated.iter().map(Into::into).collect(),
            failure: None,
        }
    }

    /// The answer for a request that was understood and refused.
    fn from_error(error: &PruneError) -> Self {
        let code = match error {
            PruneError::UnknownNeuron { .. } => reason::UNKNOWN_NEURON,
            PruneError::UnknownSynapse { .. } => reason::UNKNOWN_SYNAPSE,
            PruneError::Protected { .. } => reason::PROTECTED_NEURON,
            PruneError::UnknownNeuronType { .. } => reason::UNKNOWN_NEURON_TYPE,
            PruneError::NonFiniteStatistic { .. } => reason::NON_FINITE_STATISTIC,
            PruneError::NegativeVariance { .. } => reason::NEGATIVE_VARIANCE,
            PruneError::UnknownProxy { .. } => reason::UNKNOWN_PROXY,
            PruneError::DegenerateProxy { .. } => reason::DEGENERATE_PROXY,
            PruneError::InconsistentCovariance { .. } => reason::INCONSISTENT_COVARIANCE,
            PruneError::MissingProxyEdge { .. } => reason::MISSING_PROXY_EDGE,
            PruneError::Cleanup(_) => reason::CLEANUP_FAILED,
        };
        Self {
            ok: false,
            failure: Some(PruneFailureJson {
                reason: code.to_string(),
                message: error.to_string(),
                malformed: false,
            }),
            ..Self::default()
        }
    }

    /// The answer for a payload that never reached the rewrite.
    fn malformed(detail: impl std::fmt::Display) -> Self {
        Self {
            ok: false,
            failure: Some(PruneFailureJson {
                reason: reason::MALFORMED_REQUEST.to_string(),
                message: format!("{MALFORMED_REQUEST} {detail}"),
                malformed: true,
            }),
            ..Self::default()
        }
    }
}

/// Serialise an answer, never panicking on the way out.
fn write(response: &PruneResponse) -> String {
    // The response is derived `Serialize` over owned `String`s and plain
    // numbers, so this cannot fail — but a fallback that says so beats an
    // `unwrap` that would abort the module if it ever did.
    serde_json::to_string(response).unwrap_or_else(|error| {
        format!(
            r#"{{"ok":false,"failure":{{"reason":"MALFORMED_REQUEST","message":"{MALFORMED_REQUEST} response could not be serialised: {error}","malformed":true}}}}"#
        )
    })
}

/// Refuse a creature bigger than the boundary walks, before it allocates.
///
/// The ceiling and its wording live once, on
/// [`crate::creature_validate_json::oversized_detail`]; this boundary asks
/// there rather than re-inlining the comparison.
fn refuse_oversized(creature: &CreatureExport) -> Option<PruneResponse> {
    oversized_detail(creature.input.saturating_add(creature.neurons.len()))
        .map(PruneResponse::malformed)
}

/// Remove one hidden neuron, described by a JSON request, answering with JSON.
///
/// The whole of the WASM export: `wasm_exports::wasm_prune_neuron` is a
/// `#[wasm_bindgen]` rename over this, so the ABI is testable natively. The
/// request and response shapes, and the malformed-input contract, are in the
/// module documentation.
///
/// Never panics and never returns an error: a payload that is not a request
/// comes back as a structured failure carrying `"malformed": true`.
///
/// ```
/// use neat_core::prune_neuron_json;
///
/// let answer = prune_neuron_json(r#"{ "creature": "not a creature", "uuid": "h-1" }"#);
/// assert!(answer.contains("MALFORMED_REQUEST"));
/// ```
#[must_use]
pub fn prune_neuron_json(request: &str) -> String {
    write(&neuron_response(request))
}

/// The typed half of [`prune_neuron_json`].
fn neuron_response(request: &str) -> PruneResponse {
    let request: NeuronRequest = match serde_json::from_str(request) {
        Ok(request) => request,
        Err(error) => return PruneResponse::malformed(error),
    };
    if let Some(refusal) = refuse_oversized(&request.creature) {
        return refusal;
    }

    let stats = request.stats.map(PruneStats::from);
    match prune_neuron(&request.creature, &request.uuid, stats.as_ref()) {
        Ok(result) => PruneResponse::from_result(&result),
        Err(error) => PruneResponse::from_error(&error),
    }
}

/// Remove one typed synapse, described by a JSON request, answering with JSON.
///
/// The synapse half of the same ABI; see [`prune_neuron_json`] and the module
/// documentation for the contract both halves share.
///
/// ```
/// use neat_core::prune_synapse_json;
///
/// let answer = prune_synapse_json(r#"{ "creature": {}, "synapse": {} }"#);
/// assert!(answer.contains("MALFORMED_REQUEST"));
/// ```
#[must_use]
pub fn prune_synapse_json(request: &str) -> String {
    write(&synapse_response(request))
}

/// The typed half of [`prune_synapse_json`].
fn synapse_response(request: &str) -> PruneResponse {
    let request: SynapseRequest = match serde_json::from_str(request) {
        Ok(request) => request,
        Err(error) => return PruneResponse::malformed(error),
    };
    if let Some(refusal) = refuse_oversized(&request.creature) {
        return refusal;
    }

    let role = match parse_role(request.synapse.synapse_type.as_deref()) {
        Ok(role) => role,
        Err(detail) => return PruneResponse::malformed(detail),
    };
    let key = SynapseKey {
        from_uuid: request.synapse.from_uuid,
        to_uuid: request.synapse.to_uuid,
        role,
    };

    let stats = request.stats.map(PruneStats::from);
    match prune_synapse(&request.creature, &key, stats.as_ref()) {
        Ok(result) => PruneResponse::from_result(&result),
        Err(error) => PruneResponse::from_error(&error),
    }
}

// ---------------------------------------------------------------------------
// The golden record native and WASM are both graded against.
//
// Native only. The record is test scaffolding — the requests, and the answers
// native gives them — that a host never asks for, and this crate ships its
// wasm side as a published bundle, so none of it belongs in those bytes. The
// wasm surface is the two shims over the ABI above and nothing else.
// ---------------------------------------------------------------------------

#[cfg(not(target_family = "wasm"))]
mod golden {
    use super::{prune_neuron_json, prune_synapse_json, role_name};
    use serde::{Deserialize, Serialize};

    /// Which entry point a golden case drives.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
    #[serde(rename_all = "lowercase")]
    pub enum PruneOp {
        /// [`prune_neuron_json`], `prune_neuron` on the wasm surface.
        Neuron,
        /// [`prune_synapse_json`], `prune_synapse` on the wasm surface.
        Synapse,
    }

    /// One request in the golden record, with the answer native gives it.
    #[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
    pub struct PruneGoldenCase {
        /// Stable name, used in the parity report and in test failures.
        pub name: String,
        /// What the case pins, in one line.
        pub note: String,
        /// Which entry point to call.
        pub op: PruneOp,
        /// The request, as a JSON value so a diff shows a changed weight.
        pub request: serde_json::Value,
        /// The answer the **native** implementation gives — filled in when the
        /// record is written, and empty on the cases [`prune_golden_cases`] builds.
        #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
        pub response: serde_json::Value,
    }

    /// Where the committed golden record lives, relative to the repository root.
    pub const GOLDEN_PATH: &str = "neat-core/tests/golden/prune_wasm_parity.json";

    /// Build the request half of the golden record.
    ///
    /// The cases are the Issue #588 captures — the same fixtures Issues #589-#591
    /// were graded on, so the wire surface is exercised on the creatures the
    /// semantics were proven on — plus the request and payload shapes only a
    /// boundary has: a refusal, a malformed payload, an `IF` whose condition the
    /// removal makes static, and a correlated-survivor statistics payload.
    #[must_use]
    pub fn prune_golden_cases() -> Vec<PruneGoldenCase> {
        let mut cases = Vec::new();

        for case in crate::prune_fixtures::PRUNE_PARITY_CASES {
            let creature = case.before();
            let (op, target) = match case.request {
                crate::prune_fixtures::PruneRequest::RemoveNeuron { uuid } => {
                    (PruneOp::Neuron, serde_json::json!({ "uuid": uuid }))
                }
                crate::prune_fixtures::PruneRequest::RemoveSynapse {
                    from_uuid,
                    to_uuid,
                    role,
                } => (
                    PruneOp::Synapse,
                    serde_json::json!({ "synapse": {
                        "fromUUID": from_uuid,
                        "toUUID": to_uuid,
                        "type": role_name(role),
                    }}),
                ),
            };
            let mut request = serde_json::json!({ "creature": creature });
            merge(&mut request, target);
            if let Some(mean) = case.mean_activation {
                merge(
                    &mut request,
                    serde_json::json!({ "stats": { "meanActivation": mean } }),
                );
            }
            cases.push(PruneGoldenCase {
                name: case.name.to_string(),
                note: case.rule.to_string(),
                op,
                request,
                response: serde_json::Value::Null,
            });
        }

        // An `IF` whose condition the removal leaves structurally fixed: the
        // condition is `input-0` plus a constant, so cutting the varying half
        // flattens the `IF` onto the branch it always took (Issue #591).
        let typed = crate::prune_fixtures::EDGE_ROLE_IDENTITY.before();
        cases.push(PruneGoldenCase {
            name: "static_if_rewrite".to_string(),
            note: "an IF whose condition the removal fixes is flattened to the surviving branch"
                .to_string(),
            op: PruneOp::Synapse,
            request: serde_json::json!({
                "creature": typed,
                "synapse": { "fromUUID": "input-0", "toUUID": "if-1", "type": "condition" },
            }),
            response: serde_json::Value::Null,
        });

        // An `IF` branch the removal empties while its condition still varies:
        // cleanup gives the role back a zero-weight support edge, which is the
        // one response payload no captured fixture reaches (Issue #591).
        let branch = crate::creature::parse_creature_json(EMPTIED_IF_BRANCH)
            .expect("the golden fixture parses");
        cases.push(PruneGoldenCase {
            name: "restored_if_role".to_string(),
            note: "an IF branch the removal empties is given back a zero-weight support edge"
                .to_string(),
            op: PruneOp::Synapse,
            request: serde_json::json!({
                "creature": branch,
                "synapse": { "fromUUID": "h-p", "toUUID": "if-1", "type": "positive" },
            }),
            response: serde_json::Value::Null,
        });

        // A source the creature itself fixes: the removal folds `w · a` into the
        // target's bias with no statistic, so the rewrite is *exact* — the label a
        // host must be able to read off the wire.
        cases.push(PruneGoldenCase {
            name: "constant_edge_folds_exactly".to_string(),
            note: "an edge from a constant folds into the target's bias and preserves the output"
                .to_string(),
            op: PruneOp::Synapse,
            request: serde_json::json!({
                "creature": crate::prune_fixtures::CONSTANT_BIAS_FOLD.before(),
                "synapse": { "fromUUID": "c-1", "toUUID": "output-0" },
            }),
            response: serde_json::Value::Null,
        });

        // The correlated-survivor remedy, which is the widest statistics payload
        // the wire carries.
        let correlated = crate::creature::parse_creature_json(CORRELATED_SURVIVOR)
            .expect("the golden fixture parses");
        cases.push(PruneGoldenCase {
            name: "proxy_compensation".to_string(),
            note: "the correlated-survivor remedy: a weight share and the residual it leaves"
                .to_string(),
            op: PruneOp::Neuron,
            request: serde_json::json!({
                "creature": correlated,
                "uuid": "h-x",
                "stats": {
                    "meanActivation": 0.5,
                    "variance": 0.04,
                    "proxy": {
                        "uuid": "h-p", "meanActivation": 0.4,
                        "variance": 0.02, "covariance": 0.01,
                    },
                },
            }),
            response: serde_json::Value::Null,
        });

        // A refusal — understood, and answered `malformed: false`.
        let protected = crate::prune_fixtures::CASCADE_ORPHAN_FEEDERS.before();
        cases.push(PruneGoldenCase {
            name: "protected_neuron_refused".to_string(),
            note: "an output neuron is not a caller's to delete".to_string(),
            op: PruneOp::Neuron,
            request: serde_json::json!({ "creature": protected, "uuid": "output-0" }),
            response: serde_json::Value::Null,
        });

        // A triple the creature does not carry: the role is part of the identity.
        let roles = crate::prune_fixtures::EDGE_ROLE_IDENTITY.before();
        cases.push(PruneGoldenCase {
            name: "unknown_role_refused".to_string(),
            note: "asking for a role a pair does not carry names no edge".to_string(),
            op: PruneOp::Synapse,
            request: serde_json::json!({
                "creature": roles,
                "synapse": { "fromUUID": "input-0", "toUUID": "if-1", "type": "negative" },
            }),
            response: serde_json::Value::Null,
        });

        // A payload that never reaches the rewrite.
        cases.push(PruneGoldenCase {
            name: "malformed_missing_creature".to_string(),
            note: "a request naming no creature is a boundary fault, not a verdict".to_string(),
            op: PruneOp::Neuron,
            request: serde_json::json!({ "uuid": "h-1" }),
            response: serde_json::Value::Null,
        });
        cases.push(PruneGoldenCase {
            name: "malformed_unknown_role".to_string(),
            note: "a role spelling the wire does not carry is refused, never defaulted".to_string(),
            op: PruneOp::Synapse,
            request: serde_json::json!({
                "creature": crate::prune_fixtures::EDGE_ROLE_IDENTITY.before(),
                "synapse": { "fromUUID": "h-a", "toUUID": "if-1", "type": "POSITIVE" },
            }),
            response: serde_json::Value::Null,
        });

        cases
    }

    /// `if-1` reads a varying condition and one source per branch, so removing
    /// either branch's only edge leaves that role empty with the condition
    /// still undecided.
    const EMPTIED_IF_BRANCH: &str = r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-c","bias":0.1,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"h-p","bias":0.2,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"h-n","bias":0.3,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"if-1","bias":0.0,"squash":"IF"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"h-c"},
        {"weight":1.0,"fromUUID":"input-1","toUUID":"h-p"},
        {"weight":1.0,"fromUUID":"input-1","toUUID":"h-n"},
        {"weight":1.0,"fromUUID":"h-c","toUUID":"if-1","type":"condition"},
        {"weight":2.0,"fromUUID":"h-p","toUUID":"if-1","type":"positive"},
        {"weight":-2.0,"fromUUID":"h-n","toUUID":"if-1","type":"negative"},
        {"weight":1.0,"fromUUID":"if-1","toUUID":"output-0"}
      ]
    }"#;

    /// `h-x` and `h-p` both feed the output, so `h-p` can carry the correlated
    /// part of what `h-x` did.
    const CORRELATED_SURVIVOR: &str = r#"{
      "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
      "neurons":[
        {"type":"hidden","uuid":"h-x","bias":0.1,"squash":"LOGISTIC"},
        {"type":"hidden","uuid":"h-p","bias":0.2,"squash":"LOGISTIC"},
        {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
      ],
      "synapses":[
        {"weight":1.0,"fromUUID":"input-0","toUUID":"h-x"},
        {"weight":1.0,"fromUUID":"input-0","toUUID":"h-p"},
        {"weight":2.0,"fromUUID":"h-x","toUUID":"output-0"},
        {"weight":1.0,"fromUUID":"h-p","toUUID":"output-0"}
      ]
    }"#;

    /// Copy the keys of `extra` into `target`, which is always a JSON object here.
    fn merge(target: &mut serde_json::Value, extra: serde_json::Value) {
        let (Some(target), Some(extra)) = (target.as_object_mut(), extra.as_object()) else {
            return;
        };
        for (key, value) in extra {
            target.insert(key.clone(), value.clone());
        }
    }

    /// Answer one golden case through the native ABI.
    ///
    /// # Panics
    ///
    /// Panics when the case's request cannot be re-serialised. The requests are
    /// built here from `serde_json::json!`, so that is a defect in the record
    /// rather than a runtime condition, and it must fail loudly rather than be
    /// answered as a malformed payload.
    #[must_use]
    pub fn run_golden_case(case: &PruneGoldenCase) -> String {
        let request = serde_json::to_string(&case.request).unwrap_or_else(|e| {
            panic!(
                "golden case {} has an unserialisable request: {e}",
                case.name
            )
        });
        match case.op {
            PruneOp::Neuron => prune_neuron_json(&request),
            PruneOp::Synapse => prune_synapse_json(&request),
        }
    }

    /// Read and write the committed golden record.
    ///
    /// Native only: the record is test scaffolding a host never asks for, and the
    /// wasm build has no filesystem to read it from.
    #[cfg(not(target_family = "wasm"))]
    mod golden_io {
        use super::{GOLDEN_PATH, PruneGoldenCase, prune_golden_cases, run_golden_case};

        /// Absolute path of the committed golden record.
        ///
        /// Derived from [`GOLDEN_PATH`] — which is repository-root relative, and is
        /// the same string `scripts/check_wasm_prune_parity.ts` reads — so moving
        /// the record cannot leave the constant naming one file and this reader
        /// opening another.
        fn golden_file() -> std::path::PathBuf {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join(GOLDEN_PATH)
        }

        /// Load the committed golden record.
        ///
        /// # Errors
        ///
        /// Returns the read or parse failure. A missing or unreadable record is a
        /// defect, not a condition to skip over: regenerate it with
        /// [`write_golden`].
        pub fn read_golden() -> Result<Vec<PruneGoldenCase>, String> {
            let path = golden_file();
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("{GOLDEN_PATH} could not be read: {e}"))?;
            serde_json::from_str(&text).map_err(|e| format!("{GOLDEN_PATH} is not the record: {e}"))
        }

        /// Regenerate the committed golden record from the native answers.
        ///
        /// # Errors
        ///
        /// Returns the serialisation or write failure.
        pub fn write_golden() -> Result<(), String> {
            let mut cases = prune_golden_cases();
            for case in &mut cases {
                // An answer this crate cannot read back is a defect in the ABI, not
                // a record to write down as `null`.
                case.response = serde_json::from_str(&run_golden_case(case))
                    .map_err(|e| format!("{}: the native answer is not JSON: {e}", case.name))?;
            }
            let mut text = serde_json::to_string_pretty(&cases)
                .map_err(|e| format!("the golden record could not be serialised: {e}"))?;
            text.push('\n');
            let path = golden_file();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{} could not be created: {e}", parent.display()))?;
            }
            std::fs::write(&path, text)
                .map_err(|e| format!("{GOLDEN_PATH} could not be written: {e}"))
        }
    }

    #[cfg(not(target_family = "wasm"))]
    pub use golden_io::{read_golden, write_golden};
}

#[cfg(not(target_family = "wasm"))]
pub use golden::{
    GOLDEN_PATH, PruneGoldenCase, PruneOp, prune_golden_cases, read_golden, run_golden_case,
    write_golden,
};

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_HIDDEN: &str = r#"{"input":1,"output":1,"forwardOnly":true,
        "neurons":[
          {"type":"hidden","uuid":"h-1","bias":0.1,"squash":"LOGISTIC"},
          {"type":"output","uuid":"output-0","bias":0.25,"squash":"IDENTITY"}],
        "synapses":[
          {"weight":1.0,"fromUUID":"input-0","toUUID":"h-1"},
          {"weight":1.0,"fromUUID":"input-0","toUUID":"output-0"},
          {"weight":0.2,"fromUUID":"h-1","toUUID":"output-0"}]}"#;

    fn request(target: &str) -> String {
        format!(r#"{{"creature":{ONE_HIDDEN},{target}}}"#)
    }

    #[test]
    fn the_ok_response_omits_every_empty_report_list() {
        let answer = prune_synapse_json(&request(
            r#""synapse":{"fromUUID":"h-1","toUUID":"output-0"}"#,
        ));
        assert!(answer.starts_with(r#"{"ok":true,"creature":"#), "{answer}");
        assert!(!answer.contains("[]"), "empty lists are omitted: {answer}");
        assert!(answer.contains(r#""transform":"#), "{answer}");
    }

    #[test]
    fn a_role_the_wire_does_not_carry_never_defaults_to_standard() {
        assert_eq!(parse_role(None), Ok(SynapseType::Standard));
        assert_eq!(parse_role(Some("standard")), Ok(SynapseType::Standard));
        assert_eq!(parse_role(Some("condition")), Ok(SynapseType::Condition));
        assert_eq!(parse_role(Some("negative")), Ok(SynapseType::Negative));
        assert_eq!(parse_role(Some("positive")), Ok(SynapseType::Positive));
        assert!(parse_role(Some("Positive")).is_err());
        assert!(parse_role(Some("")).is_err());
    }

    #[test]
    fn every_role_round_trips_through_its_wire_spelling() {
        for role in [
            SynapseType::Standard,
            SynapseType::Condition,
            SynapseType::Negative,
            SynapseType::Positive,
        ] {
            assert_eq!(parse_role(Some(role_name(role))), Ok(role));
        }
    }

    #[test]
    fn a_refusal_and_a_boundary_fault_are_told_apart_on_the_wire() {
        let refused: PruneResponse =
            serde_json::from_str(&prune_neuron_json(&request(r#""uuid":"output-0""#)))
                .expect("a response");
        let failure = refused.failure.expect("a refusal names its reason");
        assert_eq!(failure.reason, reason::PROTECTED_NEURON);
        assert!(!failure.malformed);

        let faulted: PruneResponse =
            serde_json::from_str(&prune_neuron_json("{")).expect("a response");
        assert!(faulted.failure.expect("named").malformed);
    }

    #[test]
    fn every_golden_case_has_a_unique_name_and_a_note() {
        let cases = prune_golden_cases();
        let mut names: Vec<&str> = cases.iter().map(|c| c.name.as_str()).collect();
        names.sort_unstable();
        let unique = names.len();
        names.dedup();
        assert_eq!(names.len(), unique, "golden case names must be unique");
        assert!(cases.iter().all(|c| !c.note.is_empty()));
    }
}
