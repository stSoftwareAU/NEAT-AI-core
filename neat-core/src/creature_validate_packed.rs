//! The packed ABI `creature_validate` crosses the WASM boundary on when the
//! host is validating a large creature (NEAT-AI#3832).
//!
//! [`mod@crate::creature_validate_json`] is JSON in, JSON out. That is the
//! right shape for a *failure* — a record with a class, a reason and a message
//! — but the wrong shape for a creature: on a 4 272-neuron, 22 928-synapse
//! production creature the request is 850 KB, and the host spends 3.5 ms
//! building the string before the rules have looked at anything.
//! `creatureValidate` runs after every mutation, breed and discovery step, so
//! that cost is paid on the hot path.
//!
//! This module is the same rules over a **packed buffer**: one little-endian
//! byte block the host fills from the typed arrays it already holds
//! (`TypedTopology`), copied into linear memory as a single `memcpy`. Both
//! shapes meet at the same seam — the shared rule walk in
//! [`mod@crate::creature_validate`] — so a creature cannot
//! be judged differently depending on how it was described.
//!
//! # No strings, and what that costs
//!
//! The buffer carries **no text at all**: a neuron's type, activation and UUID
//! travel as codes and flags. That is what makes it fast — the strings are
//! most of the payload and all of the per-neuron allocation — and the rules
//! need none of them to reach a *verdict*. They need them only to write the
//! *message*, so a packed request can say "this creature is healthy, and here
//! are its counters" but cannot say "hidden neuron `neuron-1803232510` has no
//! outward connections".
//!
//! So a broken creature comes back as `{"ok":false,"detailRequired":true}`,
//! and the host asks [`crate::creature_validate_json()`] for the failure. That call is
//! the slow one, and it is the one that never happens on a healthy creature:
//! a failure throws, ending whatever the host was doing.
//!
//! Reporting the failure from the JSON shape rather than reconstructing it
//! here is also what keeps the message single-sourced — there is exactly one
//! place a `creatureValidate` message is written, and it is
//! [`mod@crate::creature_validate`].
//!
//! ```mermaid
//! flowchart LR
//!   H["host: TypedTopology + neuron block"] --> P["creature_validate_packed"]
//!   P -- "ok + stats" --> S["done"]
//!   P -- "detailRequired" --> J["creature_validate (JSON)"] --> F["class, reason, message"]
//!   P -- "buffer is not a request" --> M["malformed — never a verdict"]
//! ```
//!
//! # Request layout
//!
//! One buffer, little-endian throughout, laid out as a header followed by five
//! per-neuron arrays and three per-synapse arrays. `N` is the neuron count and
//! `S` the synapse count; the buffer is exactly [`packed_request_len`] bytes
//! and a request of any other length is refused.
//!
//! | Offset | Type | Field |
//! |--------|------|-------|
//! | 0  | `u32` | [`PACKED_MAGIC`] |
//! | 4  | `u32` | [`PACKED_VERSION`] |
//! | 8  | `u32` | `N`, the neuron count |
//! | 12 | `u32` | `S`, the synapse count |
//! | 16 | `u32` | option flags — see below |
//! | 20 | `u32` | `options.neurons`, read only when its flag is set |
//! | 24 | `u32` | `options.connections`, read only when its flag is set |
//! | 28 | `u32` | reserved, written as `0` |
//! | 32 | `f64` | the declared `input` width |
//! | 40 | `f64` | the declared `output` width |
//! | 48 | `f64 × N` | neuron ids |
//! | 48 + 8N | `f64 × N` | neuron biases |
//! | 48 + 16N | `u8 × N` | neuron kinds |
//! | 48 + 17N | `u8 × N` | neuron flags |
//! | 48 + 18N | `u8 × N` | activation codes |
//! | `E` | `u32 × S` | synapse `from` positions |
//! | `E` + 4S | `u32 × S` | synapse `to` positions |
//! | `E` + 8S | `u8 × S` | synapse role codes |
//!
//! `E` is `48 + 19N` rounded up to the next multiple of four, so the `u32`
//! arrays stay four-aligned however many neurons the creature carries.
//!
//! ## Option flags (offset 16)
//!
//! | Bit | Meaning |
//! |-----|---------|
//! | 0 | `forwardOnly` |
//! | 1 | `feedbackLoop` was given |
//! | 2 | `feedbackLoop`'s value, read only when bit 1 is set |
//! | 3 | `options.neurons` was given |
//! | 4 | `options.connections` was given |
//!
//! ## Neuron flags (offset 48 + 17N)
//!
//! | Bit | Meaning |
//! |-----|---------|
//! | 0 | the neuron carries an id; when clear, rule 4 reports "no id" |
//! | 1 | the neuron carries a bias; when clear, rule 8 reads `undefined` |
//! | 2 | the neuron carries an activation; when clear, it has none |
//!
//! A *present* id or bias is sent verbatim, non-finite values included: `NaN`
//! reaches rule 5 or rule 8 exactly as it would through the JSON shape's
//! sentinel strings. The flags are only for what JavaScript calls `undefined`.
//!
//! ## Neuron kinds (offset 48 + 16N)
//!
//! `0` input, `1` constant, `2` hidden, `3` output, and anything else the
//! type rule 20 rejects — a host sends [`KIND_OTHER`] for a type it does not
//! recognise, and the message naming that type comes from the JSON shape.
//!
//! ## Activation codes (offset 48 + 18N)
//!
//! [`SquashType`] as a `u8`, with [`SQUASH_UNKNOWN`] for a name this crate
//! does not know. Only two things are read off it: whether it is
//! [`SquashType::If`] (rule 12 and the forward-only structural leg), and
//! whether the neuron has one at all (rule 15). Every other code is "some
//! other activation".
//!
//! ## Synapse roles (offset `E` + 8S)
//!
//! [`SynapseType`] as a `u8` — `0` standard, `1` condition, `2` negative,
//! `3` positive, which is the encoding `TypedTopology.synapseTypes` already
//! carries.
//!
//! ## Endpoints
//!
//! A `from` or `to` is the neuron's **array position**, the same integer the
//! host holds. A position no neuron occupies — including the
//! [`ENDPOINT_NONE`] sentinel a host sends for an endpoint that is not an
//! index at all — is a rule failure, reported the same way any other is.
//!
//! # The memetic record
//!
//! `memetic` is irregular, optional, and small — five entries and 618 bytes on
//! the creature that motivated this module — so it stays JSON, passed
//! alongside the buffer. An empty string is a creature with no memetic record.
//!
//! # Malformed input cannot panic
//!
//! Same contract as the JSON shape: a buffer that is not a request comes back
//! as an ordinary structured failure carrying `"malformed": true` and a
//! message leading with [`MALFORMED_REQUEST`], never as a trap. A panic in
//! WASM aborts the module and takes the host's session with it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::creature::parse_squash_name;
use crate::creature_validate::{
    NeuronKind, NeuronView, ValidateOptions, validate_declared_widths, validate_prepared,
};
use crate::creature_validate_json::{
    MALFORMED_REQUEST, MAX_REQUEST_NEURONS, ValidationFailureJson, ValidationStatsJson,
};
use crate::creature_validate_runtime::memetic_view;
use crate::squash::SquashType;
use crate::synapse_type::SynapseType;

/// The four bytes a packed request opens with — `"NVAL"` read little-endian.
///
/// A host that sends the wrong buffer entirely (a compiled network, a training
/// batch) is told so, rather than having its bytes read as a creature.
pub const PACKED_MAGIC: u32 = 0x4C41_564E;

/// The layout revision this crate reads.
///
/// Bumped whenever the byte layout changes. A host built against a different
/// revision is refused rather than mis-read: the vendored bundle and the host
/// advance together, so a mismatch is a stale build, not a runtime condition
/// to tolerate.
pub const PACKED_VERSION: u32 = 1;

/// Bytes before the first per-neuron array.
pub const PACKED_HEADER_BYTES: usize = 48;

/// The kind code for a type none of the four rules recognise (rule 20).
pub const KIND_OTHER: u8 = 4;

/// The activation code for a name this crate cannot parse.
///
/// Matches what the JSON shape does with an unknown name: it is "not `IF`",
/// and nothing else about it is a rule here — NEAT-AI checks the name itself
/// host-side in `neuron.validate()`.
pub const SQUASH_UNKNOWN: u8 = u8::MAX;

/// The endpoint a host sends for a `from` or `to` that is not a neuron
/// position at all.
///
/// Any value at or past the neuron count is refused the same way; this is the
/// one a host can always reach for, since a creature can never carry
/// `u32::MAX` neurons.
pub const ENDPOINT_NONE: u32 = u32::MAX;

/// Option flag bits at offset 16 — see the module documentation.
mod option_flag {
    /// `forwardOnly`.
    pub(super) const FORWARD_ONLY: u32 = 1 << 0;
    /// `feedbackLoop` was given at all.
    pub(super) const FEEDBACK_LOOP_SET: u32 = 1 << 1;
    /// `feedbackLoop`'s value, read only when [`FEEDBACK_LOOP_SET`] is set.
    pub(super) const FEEDBACK_LOOP_VALUE: u32 = 1 << 2;
    /// `options.neurons` was given.
    pub(super) const NEURONS_SET: u32 = 1 << 3;
    /// `options.connections` was given.
    pub(super) const CONNECTIONS_SET: u32 = 1 << 4;
}

/// Neuron flag bits — see the module documentation.
mod neuron_flag {
    /// The neuron carries an id.
    pub(super) const HAS_ID: u8 = 1 << 0;
    /// The neuron carries a bias.
    pub(super) const HAS_BIAS: u8 = 1 << 1;
    /// The neuron carries an activation.
    pub(super) const HAS_SQUASH: u8 = 1 << 2;
}

/// Bytes a packed request for `neuron_count` neurons and `synapse_count`
/// synapses occupies.
///
/// The one home of the layout arithmetic: the host sizes its buffer with the
/// same formula, and a request of any other length is refused rather than
/// read short.
///
/// Sized for a request that exists — a creature the caller is holding. The
/// boundary cannot assume that of the counts it reads out of a *buffer*, so it
/// computes the same length in a wider type instead, so a header claiming
/// more synapses than any buffer could hold is refused rather than wrapped.
#[must_use]
pub const fn packed_request_len(neuron_count: usize, synapse_count: usize) -> usize {
    synapse_section_offset(neuron_count) + synapse_count * 9
}

/// [`packed_request_len`] in a width no target can overflow.
///
/// A header can claim `u32::MAX` synapses, and `u32::MAX * 9` does not fit a
/// `usize` on wasm32 — it would wrap, agree with a short buffer's length, and
/// send the reads below off the end of it. Sixty-four bits hold the largest
/// length the counts can express (about 39 GB) with room to spare, so the
/// comparison against `buffer.len()` is decided before any offset is built.
const fn packed_request_len_wide(neuron_count: u64, synapse_count: u64) -> u64 {
    let unaligned = PACKED_HEADER_BYTES as u64 + neuron_count * 19;
    unaligned.next_multiple_of(4) + synapse_count * 9
}

/// Offset of the first per-synapse array, four-aligned so the `u32` endpoint
/// arrays are readable as words on the host side whatever `N` is.
const fn synapse_section_offset(neuron_count: usize) -> usize {
    let unaligned = PACKED_HEADER_BYTES + neuron_count * 19;
    unaligned.next_multiple_of(4)
}

/// What [`creature_validate_packed`] answers with.
///
/// Deliberately **not** [`crate::ValidateResponse`]: this shape can say
/// "a rule was broken and I cannot name it", which the JSON shape can never
/// say and must never learn to.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackedValidateResponse {
    /// `true` when the creature broke no rule.
    pub ok: bool,
    /// The counters — present only when `ok`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<ValidationStatsJson>,
    /// `true` when a rule was broken and the host must ask
    /// [`crate::creature_validate_json()`] which one, and in what words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_required: Option<bool>,
    /// The boundary fault — present only when the buffer was never a request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<ValidationFailureJson>,
}

impl PackedValidateResponse {
    /// A healthy creature, and what was counted walking it.
    fn healthy(stats: ValidationStatsJson) -> Self {
        Self {
            ok: true,
            stats: Some(stats),
            detail_required: None,
            failure: None,
        }
    }

    /// A rule was broken. Which one, and in what words, is the JSON shape's to
    /// say — see the module documentation.
    fn detail_required() -> Self {
        Self {
            ok: false,
            stats: None,
            detail_required: Some(true),
            failure: None,
        }
    }

    /// A boundary fault: the buffer never reached a rule.
    ///
    /// Reported exactly as the JSON shape reports one — `ValidationError` /
    /// `OTHER`, `malformed` set, and the message led by [`MALFORMED_REQUEST`]
    /// — so neither the host nor a log reader can mistake it for a verdict on
    /// the creature.
    fn malformed(detail: impl std::fmt::Display) -> Self {
        Self {
            ok: false,
            stats: None,
            detail_required: None,
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

/// Validate a creature described by a packed buffer, answering with JSON.
///
/// This is the whole of the WASM export:
/// `wasm_exports::wasm_creature_validate_packed` is a `#[wasm_bindgen]` rename
/// over it, so the ABI is testable natively. The buffer layout, the
/// `detailRequired` contract and the malformed-input contract are in the
/// module documentation.
///
/// Never panics and never returns an error: a buffer that is not a request
/// comes back as a structured failure carrying `"malformed": true`.
///
/// `memetic_json` is the creature's memetic record as JSON, or `""` for a
/// creature that carries none.
///
/// ```
/// use neat_core::{
///     RuntimeCreature, ValidateOptions, creature_validate_packed, encode_packed_request,
/// };
///
/// let creature: RuntimeCreature = serde_json::from_str(
///     r#"{ "input": 1, "output": 1,
///          "neurons": [ { "type": "input",  "id": 0,  "uuid": "input-0" },
///                       { "type": "output", "id": -1, "uuid": "output-0",
///                         "bias": 0.0, "squash": "IDENTITY" } ],
///          "synapses": [ { "from": 0, "to": 1 } ] }"#,
/// )?;
///
/// let buffer = encode_packed_request(&creature, &ValidateOptions::default());
/// let answer = creature_validate_packed(&buffer, "");
///
/// assert_eq!(
///     answer,
///     r#"{"ok":true,"stats":{"input":1,"constant":0,"hidden":0,"output":1,"connections":1}}"#
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn creature_validate_packed(buffer: &[u8], memetic_json: &str) -> String {
    let response = validate_packed_request(buffer, memetic_json);
    // The response is derived `Serialize` over owned `String`s and plain
    // numbers, so this cannot fail — but a fallback that says so beats an
    // `unwrap` that would abort the module if it ever did.
    serde_json::to_string(&response).unwrap_or_else(|error| {
        format!(
            r#"{{"ok":false,"failure":{{"class":"ValidationError","reason":"OTHER","message":"{MALFORMED_REQUEST} response could not be serialised: {error}","neuronIndex":null,"synapseIndex":null,"malformed":true}}}}"#
        )
    })
}

/// The typed half of [`creature_validate_packed`], kept separate so the tests
/// read the answer as a value rather than as text.
fn validate_packed_request(buffer: &[u8], memetic_json: &str) -> PackedValidateResponse {
    let request = match PackedRequest::decode(buffer) {
        Ok(request) => request,
        Err(detail) => return PackedValidateResponse::malformed(detail),
    };

    let memetic: Option<Value> = if memetic_json.is_empty() {
        None
    } else {
        match serde_json::from_str(memetic_json) {
            Ok(value) => Some(value),
            Err(error) => {
                return PackedValidateResponse::malformed(format!(
                    "the memetic record is not JSON: {error}"
                ));
            }
        }
    };

    if validate_declared_widths(
        request.neuron_count,
        request.declared_input,
        request.declared_output,
        &request.options,
    )
    .is_err()
    {
        return PackedValidateResponse::detail_required();
    }

    // Rules 2 and 3 passed, so both widths are integers of at least one.
    let input = request.declared_input as usize;
    let output = request.declared_output as usize;

    let views = request.neuron_views();
    // An endpoint naming no neuron is a rule failure in the runtime shape
    // (`INVALID_SYNAPSE_REFERENCE`), so it is one here — reported by the JSON
    // shape rather than indexed out of bounds.
    if request
        .from_indices
        .iter()
        .chain(request.to_indices.iter())
        .any(|endpoint| *endpoint as usize >= request.neuron_count)
    {
        return PackedValidateResponse::detail_required();
    }

    let memetic = memetic.as_ref().map(memetic_view);

    match validate_prepared(
        &views,
        None,
        input,
        output,
        &request.from_indices,
        &request.to_indices,
        &request.synapse_types,
        &request.options,
        memetic.as_ref(),
    ) {
        Ok(stats) => PackedValidateResponse::healthy(stats.into()),
        Err(_) => PackedValidateResponse::detail_required(),
    }
}

/// A decoded packed request — the buffer, read once into the shapes the rules
/// take.
struct PackedRequest {
    neuron_count: usize,
    declared_input: f64,
    declared_output: f64,
    options: ValidateOptions,
    ids: Vec<f64>,
    biases: Vec<f64>,
    kinds: Vec<u8>,
    flags: Vec<u8>,
    squash_codes: Vec<u8>,
    from_indices: Vec<u32>,
    to_indices: Vec<u32>,
    synapse_types: Vec<SynapseType>,
}

impl PackedRequest {
    /// Read a buffer, or say why it is not a request.
    ///
    /// Every read is bounds-checked against a length the header agreed to
    /// first: a host that miscounted its own creature is told so, rather than
    /// trapping the module on a short slice.
    fn decode(buffer: &[u8]) -> Result<Self, String> {
        if buffer.len() < PACKED_HEADER_BYTES {
            return Err(format!(
                "a packed request is at least {PACKED_HEADER_BYTES} bytes, this one is {}",
                buffer.len()
            ));
        }

        let magic = read_u32(buffer, 0);
        if magic != PACKED_MAGIC {
            return Err(format!(
                "buffer does not open with the packed-request magic {PACKED_MAGIC:#010x} (read {magic:#010x})"
            ));
        }

        let version = read_u32(buffer, 4);
        if version != PACKED_VERSION {
            return Err(format!(
                "packed request layout version {version}, this bundle reads version {PACKED_VERSION}"
            ));
        }

        let declared_neurons = u64::from(read_u32(buffer, 8));
        let declared_synapses = u64::from(read_u32(buffer, 12));
        if declared_neurons > MAX_REQUEST_NEURONS as u64 {
            return Err(format!(
                "creature declares {declared_neurons} neurons, exceeding the maximum of {MAX_REQUEST_NEURONS}"
            ));
        }

        // Decided in 64 bits, before a single offset is built from the counts:
        // a buffer of the agreed length bounds every read below.
        let expected = packed_request_len_wide(declared_neurons, declared_synapses);
        if buffer.len() as u64 != expected {
            return Err(format!(
                "a request for {declared_neurons} neurons and {declared_synapses} synapses is {expected} bytes, this one is {}",
                buffer.len()
            ));
        }

        // The buffer is that long, so both counts fit the address space.
        let neuron_count = declared_neurons as usize;
        let synapse_count = declared_synapses as usize;

        let option_bits = read_u32(buffer, 16);
        let options = ValidateOptions {
            neurons: (option_bits & option_flag::NEURONS_SET != 0)
                .then(|| read_u32(buffer, 20) as usize),
            connections: (option_bits & option_flag::CONNECTIONS_SET != 0)
                .then(|| read_u32(buffer, 24) as usize),
            feedback_loop: (option_bits & option_flag::FEEDBACK_LOOP_SET != 0)
                .then_some(option_bits & option_flag::FEEDBACK_LOOP_VALUE != 0),
            forward_only: option_bits & option_flag::FORWARD_ONLY != 0,
        };

        let ids_at = PACKED_HEADER_BYTES;
        let biases_at = ids_at + neuron_count * 8;
        let kinds_at = biases_at + neuron_count * 8;
        let flags_at = kinds_at + neuron_count;
        let squash_at = flags_at + neuron_count;
        let from_at = synapse_section_offset(neuron_count);
        let to_at = from_at + synapse_count * 4;
        let roles_at = to_at + synapse_count * 4;

        Ok(Self {
            neuron_count,
            declared_input: read_f64(buffer, 32),
            declared_output: read_f64(buffer, 40),
            options,
            ids: (0..neuron_count)
                .map(|i| read_f64(buffer, ids_at + i * 8))
                .collect(),
            biases: (0..neuron_count)
                .map(|i| read_f64(buffer, biases_at + i * 8))
                .collect(),
            kinds: buffer[kinds_at..kinds_at + neuron_count].to_vec(),
            flags: buffer[flags_at..flags_at + neuron_count].to_vec(),
            squash_codes: buffer[squash_at..squash_at + neuron_count].to_vec(),
            from_indices: (0..synapse_count)
                .map(|i| read_u32(buffer, from_at + i * 4))
                .collect(),
            to_indices: (0..synapse_count)
                .map(|i| read_u32(buffer, to_at + i * 4))
                .collect(),
            synapse_types: buffer[roles_at..roles_at + synapse_count]
                .iter()
                .map(|role| SynapseType::from(*role))
                .collect(),
        })
    }

    /// The walk's view of every neuron.
    ///
    /// The text fields are reconstructed from the codes, not carried: a
    /// `declared_type` and an activation name that round-trip, and no UUID at
    /// all. They reach only the messages, and a packed request never answers
    /// with one — see the module documentation.
    fn neuron_views(&self) -> Vec<NeuronView<'static>> {
        (0..self.neuron_count)
            .map(|index| {
                let flags = self.flags[index];
                let declared = self.ids[index];
                let has_id = flags & neuron_flag::HAS_ID != 0;
                let kind = kind_from_code(self.kinds[index]);

                NeuronView {
                    id: has_id
                        .then_some(declared)
                        .filter(|id| crate::creature_validate::is_js_integer(*id))
                        .map(|id| id as i64),
                    non_integer_id: has_id
                        .then_some(declared)
                        .filter(|id| !crate::creature_validate::is_js_integer(*id)),
                    kind,
                    declared_type: declared_type_for(kind),
                    uuid: None,
                    bias: (flags & neuron_flag::HAS_BIAS != 0).then_some(self.biases[index]),
                    squash: (flags & neuron_flag::HAS_SQUASH != 0)
                        .then(|| squash_name_for(self.squash_codes[index])),
                }
            })
            .collect()
    }
}

/// A `u32` at `offset`, which the caller has already bounds-checked.
fn read_u32(buffer: &[u8], offset: usize) -> u32 {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&buffer[offset..offset + 4]);
    u32::from_le_bytes(bytes)
}

/// An `f64` at `offset`, read byte-wise so the buffer needs no alignment
/// beyond the one byte a `&[u8]` guarantees.
fn read_f64(buffer: &[u8], offset: usize) -> f64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&buffer[offset..offset + 8]);
    f64::from_le_bytes(bytes)
}

/// The kind a code names — anything past the four is rule 20's to reject.
fn kind_from_code(code: u8) -> NeuronKind {
    match code {
        0 => NeuronKind::Input,
        1 => NeuronKind::Constant,
        2 => NeuronKind::Hidden,
        3 => NeuronKind::Output,
        _ => NeuronKind::Invalid,
    }
}

/// The code a kind travels as — the inverse of [`kind_from_code`].
fn code_from_kind(kind: NeuronKind) -> u8 {
    match kind {
        NeuronKind::Input => 0,
        NeuronKind::Constant => 1,
        NeuronKind::Hidden => 2,
        NeuronKind::Output => 3,
        NeuronKind::Invalid => KIND_OTHER,
    }
}

/// The `type` string a kind was declared with, as far as a code can say.
///
/// Reaches only the messages the packed shape does not answer with, so the
/// unrecognised case names no type: the host's own word for it is in the JSON
/// request it falls back to.
fn declared_type_for(kind: NeuronKind) -> &'static str {
    match kind {
        NeuronKind::Input => "input",
        NeuronKind::Constant => "constant",
        NeuronKind::Hidden => "hidden",
        NeuronKind::Output => "output",
        NeuronKind::Invalid => "",
    }
}

/// The activation name a code travels as.
///
/// A code this crate cannot name still has to read as *an* activation — rule
/// 15 asks whether a constant carries one at all, and JavaScript's truthiness
/// test makes an empty string no activation — so an unknown code takes a name
/// that is deliberately not one [`parse_squash_name`] accepts.
fn squash_name_for(code: u8) -> &'static str {
    if code <= SquashType::Mean as u8 {
        crate::creature::squash_name_from(SquashType::from(code))
    } else {
        "UNKNOWN_SQUASH"
    }
}

/// The code an activation name travels as, or [`SQUASH_UNKNOWN`] for a name
/// this crate does not know.
#[must_use]
pub fn squash_code_for(name: &str) -> u8 {
    parse_squash_name(name).map_or(SQUASH_UNKNOWN, |squash| squash as u8)
}

/// Pack a runtime creature into the buffer [`creature_validate_packed`] reads.
///
/// The mirror of NEAT-AI's `CreatureValidatePack.ts`, and the reason the
/// layout is testable from both ends: a Rust consumer holding a
/// [`RuntimeCreature`] can reach the packed path without hand-rolling the
/// offsets, and the conformance replay encodes the corpus through this rather
/// than restating the layout in a test.
///
/// [`RuntimeCreature`]: crate::RuntimeCreature
///
/// ```
/// use neat_core::{RuntimeCreature, ValidateOptions, encode_packed_request, packed_request_len};
///
/// let creature = RuntimeCreature::default();
/// let buffer = encode_packed_request(&creature, &ValidateOptions::default());
/// assert_eq!(buffer.len(), packed_request_len(0, 0));
/// ```
#[must_use]
pub fn encode_packed_request(
    creature: &crate::RuntimeCreature,
    options: &ValidateOptions,
) -> Vec<u8> {
    let neuron_count = creature.neurons.len();
    let synapse_count = creature.synapses.len();
    let mut buffer = vec![0u8; packed_request_len(neuron_count, synapse_count)];

    let mut option_bits = 0u32;
    if options.forward_only {
        option_bits |= option_flag::FORWARD_ONLY;
    }
    if let Some(feedback_loop) = options.feedback_loop {
        option_bits |= option_flag::FEEDBACK_LOOP_SET;
        if feedback_loop {
            option_bits |= option_flag::FEEDBACK_LOOP_VALUE;
        }
    }
    if let Some(neurons) = options.neurons {
        option_bits |= option_flag::NEURONS_SET;
        write_u32(&mut buffer, 20, neurons as u32);
    }
    if let Some(connections) = options.connections {
        option_bits |= option_flag::CONNECTIONS_SET;
        write_u32(&mut buffer, 24, connections as u32);
    }

    write_u32(&mut buffer, 0, PACKED_MAGIC);
    write_u32(&mut buffer, 4, PACKED_VERSION);
    write_u32(&mut buffer, 8, neuron_count as u32);
    write_u32(&mut buffer, 12, synapse_count as u32);
    write_u32(&mut buffer, 16, option_bits);
    write_f64(&mut buffer, 32, creature.input.unwrap_or(f64::NAN));
    write_f64(&mut buffer, 40, creature.output.unwrap_or(f64::NAN));

    let ids_at = PACKED_HEADER_BYTES;
    let biases_at = ids_at + neuron_count * 8;
    let kinds_at = biases_at + neuron_count * 8;
    let flags_at = kinds_at + neuron_count;
    let squash_at = flags_at + neuron_count;

    for (index, neuron) in creature.neurons.iter().enumerate() {
        let mut flags = 0u8;
        if let Some(id) = neuron.id {
            flags |= neuron_flag::HAS_ID;
            write_f64(&mut buffer, ids_at + index * 8, id);
        }
        if let Some(bias) = crate::creature_validate_runtime::bias_value(neuron.bias.as_ref()) {
            flags |= neuron_flag::HAS_BIAS;
            write_f64(&mut buffer, biases_at + index * 8, bias);
        }
        if let Some(squash) = neuron.squash.as_deref() {
            flags |= neuron_flag::HAS_SQUASH;
            buffer[squash_at + index] = squash_code_for(squash);
        }
        buffer[kinds_at + index] = code_from_kind(NeuronKind::from_declared_runtime(
            neuron.neuron_type.as_deref().unwrap_or(""),
        ));
        buffer[flags_at + index] = flags;
    }

    let from_at = synapse_section_offset(neuron_count);
    let to_at = from_at + synapse_count * 4;
    let roles_at = to_at + synapse_count * 4;

    for (index, synapse) in creature.synapses.iter().enumerate() {
        write_u32(&mut buffer, from_at + index * 4, endpoint(synapse.from));
        write_u32(&mut buffer, to_at + index * 4, endpoint(synapse.to));
        buffer[roles_at + index] =
            crate::creature::parse_synapse_type(synapse.synapse_type.as_deref()) as u8;
    }

    buffer
}

/// A synapse endpoint as the buffer carries it — the position itself, or
/// [`ENDPOINT_NONE`] for anything that is not one.
fn endpoint(position: Option<f64>) -> u32 {
    position
        .filter(|value| {
            crate::creature_validate::is_js_integer(*value)
                && *value >= 0.0
                && *value < f64::from(ENDPOINT_NONE)
        })
        .map_or(ENDPOINT_NONE, |value| value as u32)
}

/// Write a `u32` at `offset`, which the caller has sized the buffer for.
fn write_u32(buffer: &mut [u8], offset: usize, value: u32) {
    buffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// Write an `f64` at `offset`, which the caller has sized the buffer for.
fn write_f64(buffer: &mut [u8], offset: usize, value: f64) {
    buffer[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RuntimeCreature;

    fn runtime(json: &str) -> RuntimeCreature {
        serde_json::from_str(json).expect("a readable runtime creature")
    }

    /// The two-neuron creature every case below starts from.
    fn healthy() -> RuntimeCreature {
        runtime(
            r#"{ "input": 1, "output": 1,
                 "neurons": [ { "type": "input",  "id": 0,  "uuid": "input-0" },
                              { "type": "output", "id": -1, "uuid": "output-0",
                                "bias": 0.25, "squash": "IDENTITY" } ],
                 "synapses": [ { "from": 0, "to": 1 } ] }"#,
        )
    }

    fn answer(creature: &RuntimeCreature, options: &ValidateOptions) -> PackedValidateResponse {
        validate_packed_request(&encode_packed_request(creature, options), "")
    }

    #[test]
    fn a_healthy_creature_answers_with_its_counters() {
        let answer = creature_validate_packed(
            &encode_packed_request(&healthy(), &ValidateOptions::default()),
            "",
        );

        assert_eq!(
            answer,
            r#"{"ok":true,"stats":{"input":1,"constant":0,"hidden":0,"output":1,"connections":1}}"#
        );
    }

    #[test]
    fn a_broken_creature_sends_the_host_to_the_json_shape() {
        // A non-input neuron with no bias at all is rule 8.
        let mut creature = healthy();
        creature.neurons[1].bias = None;

        let answer = creature_validate_packed(
            &encode_packed_request(&creature, &ValidateOptions::default()),
            "",
        );

        assert_eq!(answer, r#"{"ok":false,"detailRequired":true}"#);
    }

    /// The flag, not the value in the slot, is what says a neuron carries an
    /// id.
    ///
    /// The host reuses one scratch buffer across calls, so the id slot of a
    /// neuron carrying none holds whatever the *previous* creature left there.
    /// A decoder that read the slot regardless of the flag would hand that
    /// stale id to rule 4 and call the creature healthy — which is exactly the
    /// buffer this builds: a valid creature with the id flag cleared and the
    /// id left in place.
    #[test]
    fn a_cleared_id_flag_beats_whatever_the_slot_still_holds() {
        let creature = healthy();
        let neuron_count = creature.neurons.len();
        let mut buffer = encode_packed_request(&creature, &ValidateOptions::default());
        assert!(validate_packed_request(&buffer, "").ok);

        // The output neuron's id stays `-1` in the slot; only its flag goes.
        let flags_at = PACKED_HEADER_BYTES + neuron_count * 17;
        let output = neuron_count - 1;
        assert_eq!(read_f64(&buffer, PACKED_HEADER_BYTES + output * 8), -1.0);
        buffer[flags_at + output] &= !neuron_flag::HAS_ID;

        assert_eq!(
            validate_packed_request(&buffer, ""),
            PackedValidateResponse::detail_required(),
            "a neuron with no id is rule 4, whatever the slot still holds"
        );
    }

    #[test]
    fn a_declared_width_rule_also_sends_the_host_to_the_json_shape() {
        let mut creature = healthy();
        creature.input = Some(0.0);

        assert_eq!(
            answer(&creature, &ValidateOptions::default()),
            PackedValidateResponse::detail_required()
        );
    }

    #[test]
    fn an_endpoint_naming_no_neuron_is_a_rule_failure_not_a_trap() {
        let mut creature = healthy();
        creature.synapses[0].to = Some(99.0);

        assert_eq!(
            answer(&creature, &ValidateOptions::default()),
            PackedValidateResponse::detail_required()
        );
    }

    #[test]
    fn the_memetic_record_travels_alongside_the_buffer() {
        let creature = healthy();
        let buffer = encode_packed_request(&creature, &ValidateOptions::default());

        // `-1` is the output neuron's id, and `0 -> 1` is the only synapse.
        let healthy_record = r#"{"biases":{"0":0.5},"weights":{"0":[{"toId":-1,"weight":0.5}]}}"#;
        assert!(validate_packed_request(&buffer, healthy_record).ok);

        // A bias naming a neuron the creature does not carry is rule 31.
        let broken = r#"{"biases":{"404":0.5},"weights":{}}"#;
        assert_eq!(
            validate_packed_request(&buffer, broken),
            PackedValidateResponse::detail_required()
        );
    }

    #[test]
    fn a_memetic_record_that_is_not_json_is_a_boundary_fault() {
        let buffer = encode_packed_request(&healthy(), &ValidateOptions::default());
        let answer = creature_validate_packed(&buffer, "{not json");

        assert!(answer.contains(MALFORMED_REQUEST));
        assert!(answer.contains("\"malformed\":true"));
    }

    #[test]
    fn a_buffer_that_is_not_a_request_is_refused_rather_than_read() {
        for (label, buffer) in [
            ("empty", Vec::new()),
            ("short", vec![0u8; PACKED_HEADER_BYTES - 1]),
            ("wrong magic", vec![0u8; PACKED_HEADER_BYTES]),
        ] {
            let answer = creature_validate_packed(&buffer, "");
            assert!(
                answer.contains(MALFORMED_REQUEST),
                "{label}: expected a boundary fault, got {answer}"
            );
        }
    }

    #[test]
    fn a_request_from_another_layout_revision_is_refused() {
        let mut buffer = encode_packed_request(&healthy(), &ValidateOptions::default());
        write_u32(&mut buffer, 4, PACKED_VERSION + 1);

        let answer = creature_validate_packed(&buffer, "");

        assert!(answer.contains(MALFORMED_REQUEST));
        assert!(answer.contains("layout version"));
    }

    #[test]
    fn a_buffer_whose_length_disagrees_with_its_header_is_refused() {
        let mut buffer = encode_packed_request(&healthy(), &ValidateOptions::default());
        buffer.push(0);

        let answer = creature_validate_packed(&buffer, "");

        assert!(answer.contains(MALFORMED_REQUEST));
        assert!(answer.contains("bytes, this one is"));
    }

    /// A synapse count no buffer could hold is refused on its arithmetic, not
    /// on a wrapped length.
    ///
    /// `u32::MAX * 9` does not fit a `usize` on wasm32. Computed there it
    /// wraps to 4 294 967 247 — 48 bytes short of nothing — and a header-only
    /// buffer padded to that would have been *accepted*, sending every read
    /// below off the end of it. The length is settled in 64 bits, so the count
    /// is refused whatever the target's pointer width.
    #[test]
    fn a_synapse_count_no_buffer_could_hold_is_refused() {
        let mut buffer = vec![0u8; PACKED_HEADER_BYTES];
        write_u32(&mut buffer, 0, PACKED_MAGIC);
        write_u32(&mut buffer, 4, PACKED_VERSION);
        write_u32(&mut buffer, 8, 0);
        write_u32(&mut buffer, 12, u32::MAX);

        let answer = creature_validate_packed(&buffer, "");

        assert!(answer.contains(MALFORMED_REQUEST), "{answer}");
        assert!(answer.contains("38654705703 bytes"), "{answer}");

        // The length itself, asserted in a value no 32-bit width can hold:
        // 48 header bytes plus nine per synapse. This is the assertion that
        // stays honest on a wasm32 host, where the `usize` form wraps and the
        // check above would agree with a buffer that is nowhere near this long.
        assert_eq!(
            packed_request_len_wide(0, u64::from(u32::MAX)),
            38_654_705_703
        );
        assert!(packed_request_len_wide(0, u64::from(u32::MAX)) > u64::from(u32::MAX));
    }

    #[test]
    fn a_creature_past_the_boundary_ceiling_is_refused_before_any_allocation() {
        let mut buffer = vec![0u8; PACKED_HEADER_BYTES];
        write_u32(&mut buffer, 0, PACKED_MAGIC);
        write_u32(&mut buffer, 4, PACKED_VERSION);
        write_u32(&mut buffer, 8, u32::MAX);

        let answer = creature_validate_packed(&buffer, "");

        assert!(answer.contains(MALFORMED_REQUEST));
        assert!(answer.contains("exceeding the maximum"));
    }

    #[test]
    fn the_options_survive_the_round_trip() {
        let creature = healthy();
        let options = ValidateOptions {
            neurons: Some(2),
            connections: Some(1),
            feedback_loop: Some(false),
            forward_only: true,
        };

        let buffer = encode_packed_request(&creature, &options);
        let decoded = PackedRequest::decode(&buffer).expect("a request this crate wrote");

        assert_eq!(decoded.options, options);
        assert!(answer(&creature, &options).ok);
    }

    /// Every option travels, including the two that are only distinguishable
    /// from absent by a flag: `feedbackLoop: true` is not the same request as
    /// no `feedbackLoop` at all, and neither is `connections: 0`.
    #[test]
    fn an_option_that_is_present_and_falsy_is_not_an_absent_option() {
        let creature = healthy();

        for options in [
            ValidateOptions {
                feedback_loop: Some(true),
                ..ValidateOptions::default()
            },
            ValidateOptions {
                connections: Some(0),
                ..ValidateOptions::default()
            },
            ValidateOptions {
                neurons: Some(0),
                ..ValidateOptions::default()
            },
        ] {
            let buffer = encode_packed_request(&creature, &options);
            let decoded = PackedRequest::decode(&buffer).expect("a request this crate wrote");
            assert_eq!(decoded.options, options);
        }
    }

    /// The layout arithmetic the host repeats: the `u32` endpoint arrays land
    /// four-aligned for every neuron count, not just the even ones.
    #[test]
    fn the_synapse_section_is_four_aligned_for_every_neuron_count() {
        for neuron_count in 0..64 {
            let offset = synapse_section_offset(neuron_count);
            assert_eq!(offset % 4, 0, "{neuron_count} neurons");
            assert!(offset >= PACKED_HEADER_BYTES + neuron_count * 19);
            assert!(offset < PACKED_HEADER_BYTES + neuron_count * 19 + 4);
        }
    }

    /// A kind code and the kind it names are one another's inverse, so a host
    /// writing `code_from_kind`'s answer is read back as the same kind.
    #[test]
    fn the_kind_codes_round_trip() {
        for kind in [
            NeuronKind::Input,
            NeuronKind::Constant,
            NeuronKind::Hidden,
            NeuronKind::Output,
            NeuronKind::Invalid,
        ] {
            assert_eq!(kind_from_code(code_from_kind(kind)), kind);
        }
        // Anything past the four is the type rule 20 rejects.
        assert_eq!(kind_from_code(KIND_OTHER), NeuronKind::Invalid);
        assert_eq!(kind_from_code(200), NeuronKind::Invalid);
    }

    /// An activation code has to read back as a name the rules treat the same
    /// way — `IF` above all, since rule 12 and the forward-only structural leg
    /// both branch on it.
    #[test]
    fn the_activation_codes_round_trip_through_their_names() {
        assert_eq!(squash_name_for(squash_code_for("IF")), "IF");
        assert_eq!(squash_code_for("IF"), SquashType::If as u8);
        assert_eq!(squash_name_for(squash_code_for("TANH")), "TANH");

        // A name this crate does not know is "not IF", and still an activation.
        assert_eq!(squash_code_for("NOT_A_REAL_ACTIVATION"), SQUASH_UNKNOWN);
        let unknown = squash_name_for(SQUASH_UNKNOWN);
        assert!(!unknown.is_empty());
        assert!(parse_squash_name(unknown).is_err());
    }

    /// An endpoint that is not an array position at all becomes the sentinel,
    /// which the walk refuses the same way it refuses any out-of-range one.
    #[test]
    fn an_endpoint_that_is_not_a_position_becomes_the_sentinel() {
        assert_eq!(endpoint(None), ENDPOINT_NONE);
        assert_eq!(endpoint(Some(-1.0)), ENDPOINT_NONE);
        assert_eq!(endpoint(Some(1.5)), ENDPOINT_NONE);
        assert_eq!(endpoint(Some(f64::NAN)), ENDPOINT_NONE);
        assert_eq!(endpoint(Some(7.0)), 7);
    }
}
