//! Newborn-grace and pass tags for `compactUnused`, ported from NEAT-AI (Issue #778).
//!
//! `NeuronExport` and `CreatureExport` carry no tags, so these functions compute
//! the values and the caller reads and writes the tags. [`newborn_grace_remaining`]
//! turns the `newborn-grace` tag into the count that feeds
//! [`UnusedNeuronCandidate::newborn_grace_remaining`](crate::compact_unused_choice::UnusedNeuronCandidate::newborn_grace_remaining).
//!
//! Ageing and the pass tags apply only on a pass that changed the creature. That
//! is TypeScript's uuid comparison, which stays with the caller. On such a pass
//! the caller also writes `approach` = `compact`, writes `unused` = the removed
//! uuid when the removal succeeded, removes `approach-logged`, and drops the
//! memetic record.
//!
//! ```
//! use neat_core::{NewbornGraceAgeing, age_newborn_grace, newborn_grace_remaining};
//!
//! assert_eq!(newborn_grace_remaining(Some("2")), 2);
//! assert_eq!(age_newborn_grace(Some("2")), NewbornGraceAgeing::Set("1".to_string()));
//! assert_eq!(age_newborn_grace(Some("1")), NewbornGraceAgeing::Remove);
//! ```

use crate::creature::CreatureExport;

/// The tag that holds a neuron's remaining newborn grace (TypeScript
/// `NEWBORN_GRACE_TAG`).
pub const NEWBORN_GRACE_TAG: &str = "newborn-grace";

/// Whether `c` is a JavaScript StrWhiteSpaceChar, which `parseInt` skips.
fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'..='\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{FEFF}'
    )
}

/// The rounds of newborn grace a tag value holds (TypeScript
/// `newbornGraceRemaining`). `tag` is the value `getTag` returns.
///
/// This emulates `Number.parseInt(raw, 10)`: leading whitespace, an optional
/// sign, then the longest run of digits. No digits, a non-positive value, or a
/// value too large for a finite number gives `0`.
pub fn newborn_grace_remaining(tag: Option<&str>) -> u32 {
    let Some(raw) = tag else { return 0 };
    let trimmed = raw.trim_start_matches(is_js_whitespace);
    let (negative, rest) = match trimmed.chars().next() {
        Some('+') => (false, &trimmed[1..]),
        Some('-') => (true, &trimmed[1..]),
        _ => (false, trimmed),
    };
    let digits_end = rest
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map_or(rest.len(), |(i, _)| i);
    let digits = &rest[..digits_end];
    if digits.is_empty() {
        return 0;
    }
    let Ok(value) = digits.parse::<f64>() else {
        return 0;
    };
    if !value.is_finite() || negative || value == 0.0 {
        return 0;
    }
    // SIMPLE-ON-PURPOSE: grace above u32::MAX saturates there, where TypeScript keeps the exact count — upgrade when a grace budget exceeds 4,294,967,295 rounds
    if value >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        value as u32
    }
}

/// What `ageNewbornGrace` does to one neuron's `newborn-grace` tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewbornGraceAgeing {
    /// Leave the tag as it is, including when there is no tag.
    Keep,
    /// Delete the tag.
    Remove,
    /// Write this value as the tag.
    Set(String),
}

/// Decides how `ageNewbornGrace` treats one neuron's tag. `tag` is the value
/// `getTag` returns. The caller applies the result to the neuron.
pub fn age_newborn_grace(tag: Option<&str>) -> NewbornGraceAgeing {
    let Some(raw) = tag else {
        return NewbornGraceAgeing::Keep;
    };
    match newborn_grace_remaining(tag) {
        0 if raw.is_empty() => NewbornGraceAgeing::Keep,
        0 | 1 => NewbornGraceAgeing::Remove,
        remaining => NewbornGraceAgeing::Set((remaining - 1).to_string()),
    }
}

/// The counts `compactUnused` writes as the `old-neurons` and `old-synapses`
/// tags. The caller writes each with `to_string()`, which matches JavaScript's
/// `toString` for these integers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactUnusedPassTags {
    /// The `old-neurons` value: the listed (non-input) neurons less the
    /// outputs, that is the hidden and constant neurons before the pass.
    pub old_neurons: i128,
    /// The `old-synapses` value: the synapses before the pass.
    pub old_synapses: usize,
}

/// Computes the `old-neurons` and `old-synapses` values from `clean`, the
/// creature before the pass (TypeScript `CompactUnused.ts`). The export lists no
/// input neurons, so TypeScript's `neurons.length - input - output` is
/// `neurons.len() - output` here.
///
/// `compactCreature` (`CompactCreature.ts`) writes the same `old-neurons` count
/// from its starting export, and writes no `old-synapses` tag.
pub fn compact_unused_pass_tags(clean: &CreatureExport) -> CompactUnusedPassTags {
    CompactUnusedPassTags {
        // `usize` widens losslessly into `i128`, so the difference is exact and
        // may be negative for a malformed creature, as in TypeScript.
        old_neurons: clean.neurons.len() as i128 - clean.output as i128,
        old_synapses: clean.synapses.len(),
    }
}
