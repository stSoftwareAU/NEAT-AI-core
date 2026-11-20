//! Issue #766 — the contract serialiser for persisted creature JSON.
//!
//! `docs/CREATURE_JSON_PRESENTATION.md` fixes the bytes of a persisted creature
//! file: the layout of `JSON.stringify(doc, null, 1)`, ECMAScript
//! `Number::toString` number text, `-0` kept as `-0`, a fixed key order per
//! object kind, and no trailing newline. This module is the one Rust writer of
//! that form. Both entry points share a single writer:
//!
//! - [`creature_presentation_json`] takes a `serde_json::Value` document, so
//!   keys the typed structs do not model (the creature `uuid`, `tags`,
//!   `frozen`) survive.
//! - [`creature_export_presentation_json`] takes a typed [`CreatureExport`],
//!   converts it to a document and calls the first.
//!
//! The module is plain Rust: no `cfg`, no file or environment access.
//!
//! # Refusals
//!
//! A creature whose width is refused by [`validate_creature_width`] is never
//! written. A non-finite number is refused too
//! ([`PresentationError::NonFiniteNumber`]): the typed entry names a NaN or
//! infinite `f64` field, and the document entry names a number with no finite
//! `f64` (reachable only when a consumer's build unifies `serde_json`'s
//! `arbitrary_precision` feature and a literal such as `1e400` overflows). The
//! document entry also refuses any `null` ([`PresentationError::NullValue`]),
//! because `serde_json` and `JSON.stringify` both turn NaN and infinities into
//! `null`, so a `null` is a number that is already lost. The contract never
//! writes `null`.

use std::cmp::Ordering;
use std::fmt;

use serde::Deserialize;
use serde_json::Value;

use crate::creature::{
    CreatureError, CreatureExport, MemeticExport, MemeticWeights, validate_creature_width,
};

/// Why a creature could not be written in presentation form.
#[derive(Debug)]
#[non_exhaustive]
pub enum PresentationError {
    /// Not a creature (a serde refusal, as [`CreatureError::Json`]) or a
    /// refused width (the error [`validate_creature_width`] returns).
    Creature(CreatureError),
    /// A number that is not a finite `f64`: a NaN or infinite typed field, or
    /// (with `serde_json`'s `arbitrary_precision` unified in) a literal that
    /// overflows `f64`. `path` is a JSON-pointer-style path such as
    /// `/synapses/3/weight`.
    NonFiniteNumber {
        /// Where the number sits.
        path: String,
    },
    /// Document entry: a `null` anywhere in the document. A `null` is a number
    /// already lost, and the contract never writes `null`.
    NullValue {
        /// Where the `null` sits, as a JSON-pointer-style path.
        path: String,
    },
}

impl fmt::Display for PresentationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Creature(e) => write!(f, "creature refused: {e}"),
            Self::NonFiniteNumber { path } => {
                write!(
                    f,
                    "non-finite number at {path}: JSON cannot hold NaN or infinity"
                )
            }
            Self::NullValue { path } => write!(
                f,
                "null at {path}: the presentation contract never writes null (a non-finite number becomes null)"
            ),
        }
    }
}

impl std::error::Error for PresentationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Creature(e) => Some(e),
            _ => None,
        }
    }
}

impl From<CreatureError> for PresentationError {
    fn from(e: CreatureError) -> Self {
        Self::Creature(e)
    }
}

// ---------------------------------------------------------------------------
// Key-order tables. One const slice per object kind; a swap is a one-line edit.
// ---------------------------------------------------------------------------

const CREATURE_KEYS: &[&str] = &[
    "uuid",
    "semanticVersion",
    "forwardOnly",
    "neurons",
    "synapses",
    "input",
    "output",
    "tags",
    "memetic",
];
const NEURON_KEYS: &[&str] = &["type", "uuid", "bias", "squash", "frozen", "tags", "id"];
const SYNAPSE_KEYS: &[&str] = &["weight", "fromUUID", "toUUID", "type", "frozen", "tags"];
const TAG_KEYS: &[&str] = &["name", "value"];
const MEMETIC_KEYS: &[&str] = &["generation", "weights", "biases", "score", "ancestry"];
const WEIGHT_ROW_KEYS: &[&str] = &["fromUUID", "toUUID", "weight"];
const ID_ENTRY_KEYS: &[&str] = &["toId", "weight"];
const NO_KEYS: &[&str] = &[];

/// What an object is, which decides its key table and what its members are.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Creature,
    Neuron,
    Synapse,
    Tag,
    /// `memetic` and every `ancestry[]` snapshot.
    Memetic,
    /// The value of a memetic record's `weights`: an array of rows or an
    /// id-keyed object, told apart by the JSON shape.
    WeightsField,
    WeightRow,
    /// The id-keyed `weights` object itself: no table, members are arrays of
    /// [`Kind::IdEntry`].
    IdMap,
    IdEntry,
    /// Any other object: no table.
    Plain,
}

impl Kind {
    fn table(self) -> &'static [&'static str] {
        match self {
            Self::Creature => CREATURE_KEYS,
            Self::Neuron => NEURON_KEYS,
            Self::Synapse => SYNAPSE_KEYS,
            Self::Tag => TAG_KEYS,
            Self::Memetic => MEMETIC_KEYS,
            Self::WeightRow => WEIGHT_ROW_KEYS,
            Self::IdEntry => ID_ENTRY_KEYS,
            Self::WeightsField | Self::IdMap | Self::Plain => NO_KEYS,
        }
    }

    /// The kind of the value under `key` (for an array, of its elements).
    fn member(self, key: &str) -> Self {
        match (self, key) {
            (Self::Creature, "neurons") => Self::Neuron,
            (Self::Creature, "synapses") => Self::Synapse,
            (Self::Creature, "tags") => Self::Tag,
            (Self::Creature, "memetic") => Self::Memetic,
            (Self::Neuron | Self::Synapse, "tags") => Self::Tag,
            (Self::Memetic, "weights") => Self::WeightsField,
            (Self::Memetic, "ancestry") => Self::Memetic,
            (Self::IdMap, _) => Self::IdEntry,
            _ => Self::Plain,
        }
    }
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Write a creature document in the persisted presentation form.
///
/// Order: refuse any `null`, read the document as a creature, check the width
/// with [`validate_creature_width`], then write. Keys the typed creature does
/// not model are kept and placed by the contract's general key rule.
///
/// # Errors
///
/// [`PresentationError::NullValue`] for a `null`;
/// [`PresentationError::NonFiniteNumber`] for a number with no finite `f64`
/// (only under `serde_json`'s `arbitrary_precision`);
/// [`PresentationError::Creature`] when the document is not a creature or its
/// declared width is refused.
pub fn creature_presentation_json(document: &Value) -> Result<String, PresentationError> {
    scan_document(document, &mut String::new())?;
    // `&Value` is a `Deserializer`, so this reads the document without a clone.
    let creature = CreatureExport::deserialize(document)
        .map_err(|e| PresentationError::Creature(CreatureError::Json(e)))?;
    validate_creature_width(&creature)?;

    let mut out = String::new();
    write_value(document, Kind::Creature, 0, &mut out);
    Ok(out)
}

/// Write a typed creature in the persisted presentation form.
///
/// Only the fields the typed structs model are written; use
/// [`creature_presentation_json`] to keep the rest of a document.
///
/// # Errors
///
/// [`PresentationError::Creature`] for a refused width;
/// [`PresentationError::NonFiniteNumber`] for a NaN or infinite `f64`;
/// [`PresentationError::NullValue`] for a `null` inside `memetic.extra`, which
/// the typed entry hands on to the document entry.
pub fn creature_export_presentation_json(
    creature: &CreatureExport,
) -> Result<String, PresentationError> {
    validate_creature_width(creature)?;
    check_finite(creature)?;
    let document = serde_json::to_value(creature)
        .map_err(|e| PresentationError::Creature(CreatureError::Json(e)))?;
    creature_presentation_json(&document)
}

// ---------------------------------------------------------------------------
// Refusal scans
// ---------------------------------------------------------------------------

/// JSON-pointer escaping of one path segment (`~` then `/`).
fn push_segment(path: &mut String, segment: &str) {
    path.push('/');
    path.push_str(&segment.replace('~', "~0").replace('/', "~1"));
}

/// One document-order walk refusing the first `null` (`NullValue`) or number
/// with no finite `f64` (`NonFiniteNumber`).
fn scan_document(value: &Value, path: &mut String) -> Result<(), PresentationError> {
    match value {
        Value::Null => Err(PresentationError::NullValue { path: path.clone() }),
        Value::Number(n) => match n.as_f64() {
            Some(x) if x.is_finite() => Ok(()),
            _ => Err(PresentationError::NonFiniteNumber { path: path.clone() }),
        },
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                let len = path.len();
                push_segment(path, &i.to_string());
                scan_document(item, path)?;
                path.truncate(len);
            }
            Ok(())
        }
        Value::Object(map) => {
            for (key, item) in map {
                let len = path.len();
                push_segment(path, key);
                scan_document(item, path)?;
                path.truncate(len);
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn finite(x: f64, path: impl FnOnce() -> String) -> Result<(), PresentationError> {
    if x.is_finite() {
        Ok(())
    } else {
        Err(PresentationError::NonFiniteNumber { path: path() })
    }
}

/// Walk every `f64` the typed structs hold. `memetic.extra` is already a
/// `Value`, which cannot hold a non-finite number.
fn check_finite(creature: &CreatureExport) -> Result<(), PresentationError> {
    for (i, n) in creature.neurons.iter().enumerate() {
        finite(n.bias, || format!("/neurons/{i}/bias"))?;
    }
    for (i, s) in creature.synapses.iter().enumerate() {
        finite(s.weight, || format!("/synapses/{i}/weight"))?;
    }
    if let Some(memetic) = &creature.memetic {
        check_finite_memetic(memetic)?;
    }
    Ok(())
}

fn check_finite_memetic(memetic: &MemeticExport) -> Result<(), PresentationError> {
    for (key, x) in &memetic.biases {
        finite(*x, || {
            let mut p = String::from("/memetic/biases");
            push_segment(&mut p, key);
            p
        })?;
    }
    match &memetic.weights {
        MemeticWeights::Rows(rows) => {
            for (i, row) in rows.iter().enumerate() {
                if let Some(w) = row.weight {
                    finite(w, || format!("/memetic/weights/{i}/weight"))?;
                }
            }
        }
        MemeticWeights::ById(map) => {
            for (key, entries) in map {
                for (j, entry) in entries.iter().enumerate() {
                    if let Some(w) = entry.weight {
                        finite(w, || {
                            let mut p = String::from("/memetic/weights");
                            push_segment(&mut p, key);
                            p.push_str(&format!("/{j}/weight"));
                            p
                        })?;
                    }
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The writer
// ---------------------------------------------------------------------------

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push(' ');
    }
}

fn write_value(value: &Value, kind: Kind, depth: usize, out: &mut String) {
    match value {
        Value::Null => unreachable!("the pre-scan refused every null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&js_number(
            n.as_f64()
                .expect("the pre-scan refused every number without a finite f64"),
        )),
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            // `weights` as an array is the row form.
            let element_kind = if kind == Kind::WeightsField {
                Kind::WeightRow
            } else {
                kind
            };
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(",\n");
                }
                indent(out, depth + 1);
                write_value(item, element_kind, depth + 1, out);
            }
            out.push('\n');
            indent(out, depth);
            out.push(']');
        }
        Value::Object(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            // `weights` as an object is the id-keyed form.
            let kind = if kind == Kind::WeightsField {
                Kind::IdMap
            } else {
                kind
            };
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| compare_keys(a.0, b.0, kind.table()));
            out.push_str("{\n");
            for (i, (key, item)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push_str(",\n");
                }
                indent(out, depth + 1);
                write_string(key, out);
                out.push_str(": ");
                write_value(item, kind.member(key), depth + 1, out);
            }
            out.push('\n');
            indent(out, depth);
            out.push('}');
        }
    }
}

/// `JSON.stringify` string escaping. `serde_json::to_string` on a `str` escapes
/// exactly as it does: `\"`, `\\`, `\b`, `\f`, `\n`, `\r`, `\t`, other
/// U+0000 to U+001F as lowercase `\u00XX`, everything else literal.
fn write_string(s: &str, out: &mut String) {
    out.push_str(&serde_json::to_string(s).expect("a str always serialises"));
}

// ---------------------------------------------------------------------------
// Key order
// ---------------------------------------------------------------------------

/// A canonical decimal integer string `0..=2^32-2`: an ECMAScript array index.
fn array_index(key: &str) -> Option<u64> {
    let b = key.as_bytes();
    if b.is_empty() || b.len() > 10 || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if b.len() > 1 && b[0] == b'0' {
        return None;
    }
    key.parse::<u64>().ok().filter(|n| *n <= 4_294_967_294)
}

/// Index keys ascending numerically, then table keys in table order, then the
/// rest by UTF-16 code unit.
fn compare_keys(a: &str, b: &str, table: &[&str]) -> Ordering {
    let rank = |k: &str| -> (u8, u64) {
        if let Some(n) = array_index(k) {
            (0, n)
        } else if let Some(p) = table.iter().position(|t| *t == k) {
            (1, p as u64)
        } else {
            (2, 0)
        }
    };
    rank(a)
        .cmp(&rank(b))
        .then_with(|| a.encode_utf16().cmp(b.encode_utf16()))
}

// ---------------------------------------------------------------------------
// Numbers
// ---------------------------------------------------------------------------

/// ECMAScript `Number::toString` for a finite `f64`, with `-0` written `-0`.
fn js_number(x: f64) -> String {
    if x == 0.0 {
        return if x.is_sign_negative() { "-0" } else { "0" }.to_string();
    }
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    // Rust's `{:e}` gives the shortest round-trip digits: "d.ddde<exp>".
    let sci = format!("{:e}", x.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp.parse::<i32>().unwrap_or(0) + 1;

    if k <= n && n <= 21 {
        out.push_str(&digits);
        out.push_str(&"0".repeat((n - k) as usize));
    } else if 0 < n && n <= 21 {
        out.push_str(&digits[..n as usize]);
        out.push('.');
        out.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        out.push_str(&"0".repeat((-n) as usize));
        out.push_str(&digits);
    } else {
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        // Exponent is n - 1; it is non-negative exactly when n > 0.
        out.push(if n > 0 { '+' } else { '-' });
        out.push_str(&(n - 1).abs().to_string());
    }
    out
}
