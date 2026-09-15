//! The creature a named golden case sends, for the native pruning suites
//! (Ockham #201).
//!
//! `prune_json::prune_golden_cases` is the one home of every fixture the JSON
//! entry points are graded on, and several of those shapes are graded
//! natively too — by `neat-core/tests/prune_neuron.rs` and
//! `neat-core/tests/prune_synapse.rs`. The claim the two halves make together
//! ("the wire answers what the native call answers for this shape") is only
//! worth anything while both are asked about the **same** creature, so the
//! native side reads it out of the golden case rather than keeping a second
//! copy that could drift.
//!
//! Included by path (`#[path = "common/golden_fixture.rs"] mod golden_fixture;`)
//! rather than through `common/mod.rs`, so neither target compiles a helper it
//! does not use.

use neat_core::{CreatureExport, prune_golden_cases};

/// The creature the golden case `name` sends.
///
/// # Panics
///
/// Panics when no case carries that name, or when its request carries no
/// parseable creature — a fixture that has been renamed or broken must fail
/// loudly here rather than leave the native and wire halves grading different
/// creatures in silence.
pub fn golden_creature(name: &str) -> CreatureExport {
    let case = prune_golden_cases()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no golden case named {name}"));
    serde_json::from_value(case.request["creature"].clone())
        .unwrap_or_else(|e| panic!("golden case {name} carries no parseable creature: {e}"))
}
