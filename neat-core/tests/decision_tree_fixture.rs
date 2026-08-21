//! Canonical decision-tree fixture semantics (Issue #555).
//!
//! Every expectation here is checked against a **hand-written reference tree**
//! declared in this file — plain `if x > t` Rust that shares no code with
//! `compile_creature` / `CompiledNetwork::activate`. A fault in the IF kernel
//! moves the network side only, so the assertion goes red (AGENTS.md oracle
//! rule 1). The documented `*_CASES` tables are checked against the same
//! reference, so a wrong constant in the library is caught too.

use neat_core::decision_tree::{
    DEPTH2_CASES, DEPTH2_ROOT_THRESHOLD, DEPTH2_SPLIT_THRESHOLD, DecisionCase, RESIDUAL_CASES,
    RESIDUAL_THRESHOLD, RESIDUAL_VALUE, STUMP_CASES, STUMP_POSITIVE_VALUE, STUMP_THRESHOLD,
    depth2_tree_creature, linear_base_creature, residual_correction_creature, stump_creature,
};
use neat_core::{
    CreatureExport, SynapseType, compile_creature, creature_to_json, mse_sum_batch_packed,
    parse_creature_json,
};

/// f32 tolerance for a handful of adds — the fixtures are exact in f32, but the
/// assertions stay within "normal f32 tolerance" as the issue specifies.
const TOL: f32 = 1e-6;

// ---------------------------------------------------------------------------
// Independent reference trees — no shared code path with the network kernel.
// ---------------------------------------------------------------------------

/// `x > 0.5 ? 3.0 : 0.0` — the stump the fixture is documented to encode.
fn stump_reference(x: f32) -> f32 {
    if x > STUMP_THRESHOLD as f32 {
        STUMP_POSITIVE_VALUE as f32
    } else {
        0.0
    }
}

/// Depth-2 tree: root splits on `x0 > 0.5`, both children split on `x1 > 0.25`.
fn depth2_reference(x0: f32, x1: f32) -> f32 {
    let root = DEPTH2_ROOT_THRESHOLD as f32;
    let split = DEPTH2_SPLIT_THRESHOLD as f32;
    if x0 > root {
        if x1 > split { 4.0 } else { 0.0 }
    } else if x1 > split {
        1.0
    } else {
        -2.0
    }
}

/// Linear base `2x` plus a depth-1 IF correction of `+1.5` when `x > 0.75`.
fn residual_reference(x: f32) -> f32 {
    let correction = if x > RESIDUAL_THRESHOLD as f32 {
        RESIDUAL_VALUE as f32
    } else {
        0.0
    };
    2.0 * x + correction
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn activate_once(creature: &CreatureExport, inputs: &[f32]) -> f32 {
    let mut net = compile_creature(creature).expect("fixture compiles");
    let out = net.activate(inputs, creature.output);
    assert_eq!(out.len(), creature.output);
    out[0]
}

fn assert_close(actual: f32, expected: f32, what: &str) {
    assert!(
        (actual - expected).abs() <= TOL,
        "{what}: expected {expected}, got {actual}"
    );
}

/// Drive every documented case through the compiled network *and* the
/// independent reference.
fn check_cases(
    creature: &CreatureExport,
    cases: &[DecisionCase],
    reference: impl Fn(&[f32]) -> f32,
) {
    assert!(!cases.is_empty(), "case table must not be empty");
    for case in cases {
        let expected = reference(case.inputs);
        assert_close(
            case.expected,
            expected,
            &format!("documented expected value for branch {}", case.branch),
        );
        let actual = activate_once(creature, case.inputs);
        assert_close(
            actual,
            expected,
            &format!("activation for branch {} at {:?}", case.branch, case.inputs),
        );
    }
}

// ---------------------------------------------------------------------------
// Stump
// ---------------------------------------------------------------------------

#[test]
fn stump_fixture_matches_reference_on_every_documented_case() {
    check_cases(&stump_creature(), STUMP_CASES, |i| stump_reference(i[0]));
}

#[test]
fn stump_takes_the_zero_default_branch_at_and_below_the_threshold() {
    let creature = stump_creature();
    // `condition_sum > 0.0` is strict, so the threshold itself is the default.
    assert_close(
        activate_once(&creature, &[STUMP_THRESHOLD as f32]),
        0.0,
        "x == threshold",
    );
    assert_close(activate_once(&creature, &[0.0]), 0.0, "x below threshold");
    assert_close(
        activate_once(&creature, &[STUMP_THRESHOLD as f32 + 0.25]),
        STUMP_POSITIVE_VALUE as f32,
        "x above threshold",
    );
}

// ---------------------------------------------------------------------------
// Depth-2 tree
// ---------------------------------------------------------------------------

#[test]
fn depth2_fixture_matches_reference_on_every_documented_case() {
    check_cases(&depth2_tree_creature(), DEPTH2_CASES, |i| {
        depth2_reference(i[0], i[1])
    });
}

#[test]
fn depth2_cases_cover_all_four_leaves() {
    let mut leaves: Vec<&str> = DEPTH2_CASES.iter().map(|c| c.branch).collect();
    leaves.sort_unstable();
    leaves.dedup();
    // Four leaves plus the boundary case that pins the strict `>` comparison.
    assert!(
        leaves.len() >= 4,
        "expected every leaf to be exercised, saw {leaves:?}"
    );
    // Every leaf value the reference can produce must appear in the table.
    for expected in [4.0f32, 0.0, 1.0, -2.0] {
        assert!(
            DEPTH2_CASES
                .iter()
                .any(|c| (c.expected - expected).abs() <= TOL),
            "no documented case yields leaf {expected}"
        );
    }
}

// ---------------------------------------------------------------------------
// Residual / correction leaf
// ---------------------------------------------------------------------------

#[test]
fn residual_fixture_matches_reference_on_every_documented_case() {
    check_cases(&residual_correction_creature(), RESIDUAL_CASES, |i| {
        residual_reference(i[0])
    });
}

#[test]
fn residual_fixture_adds_a_non_zero_correction_over_the_linear_base() {
    let base = linear_base_creature();
    let corrected = residual_correction_creature();
    let x = RESIDUAL_THRESHOLD as f32 + 0.25;

    let base_out = activate_once(&base, &[x]);
    let corrected_out = activate_once(&corrected, &[x]);
    assert_close(base_out, 2.0 * x, "linear base");
    assert_close(
        corrected_out - base_out,
        RESIDUAL_VALUE as f32,
        "correction above threshold",
    );

    // Below the threshold the correction leaf contributes nothing.
    let low = RESIDUAL_THRESHOLD as f32 - 0.25;
    assert_close(
        activate_once(&corrected, &[low]) - activate_once(&base, &[low]),
        0.0,
        "correction below threshold",
    );
}

// ---------------------------------------------------------------------------
// Round trip: export -> JSON -> parse -> compile keeps every synapse role
// ---------------------------------------------------------------------------

/// Count compiled synapses per role, so the assertion is on the compiled
/// structure rather than on the JSON text.
fn role_histogram(creature: &CreatureExport) -> [usize; 4] {
    let net = compile_creature(creature).expect("compiles");
    let mut counts = [0usize; 4];
    for s in &net.synapses {
        counts[SynapseType::from(s.synapse_type) as usize] += 1;
    }
    counts
}

#[test]
fn round_trip_preserves_every_synapse_role() {
    for creature in [
        stump_creature(),
        depth2_tree_creature(),
        residual_correction_creature(),
    ] {
        let before = role_histogram(&creature);
        assert!(
            before[SynapseType::Condition as usize] > 0
                && before[SynapseType::Positive as usize] > 0
                && before[SynapseType::Negative as usize] > 0,
            "fixture must exercise all three IF roles, saw {before:?}"
        );

        let json = creature_to_json(&creature).expect("serialise");
        let reparsed = parse_creature_json(&json).expect("parse");
        assert_eq!(reparsed, creature, "round trip changed the export");
        assert_eq!(role_histogram(&reparsed), before, "round trip lost a role");
    }
}

#[test]
fn round_trip_preserves_branch_semantics() {
    for (creature, cases) in [
        (stump_creature(), STUMP_CASES),
        (depth2_tree_creature(), DEPTH2_CASES),
        (residual_correction_creature(), RESIDUAL_CASES),
    ] {
        let json = creature_to_json(&creature).expect("serialise");
        let reparsed = parse_creature_json(&json).expect("parse");
        for case in cases {
            assert_close(
                activate_once(&reparsed, case.inputs),
                activate_once(&creature, case.inputs),
                "round-tripped activation",
            );
        }
    }
}

/// The batched scoring kernels drop to one record at a time for aggregate
/// squashes, so a record must not score differently for landing in an 8-record
/// group, the 4-record remainder or the scalar tail. Thirteen records
/// (`8 + 4 + 1`) reach all three tiers; the packed targets are the documented
/// branch outputs, so a disagreeing tier shows up as non-zero MSE.
#[test]
fn batched_scoring_reproduces_the_documented_branch_outputs() {
    for (creature, cases) in [
        (stump_creature(), STUMP_CASES),
        (depth2_tree_creature(), DEPTH2_CASES),
        (residual_correction_creature(), RESIDUAL_CASES),
    ] {
        let mut packed: Vec<f32> = Vec::new();
        for i in 0..13 {
            let case = &cases[i % cases.len()];
            packed.extend_from_slice(case.inputs);
            packed.push(case.expected);
        }
        let mut net = compile_creature(&creature).expect("compiles");
        let sum = mse_sum_batch_packed(&mut net, &packed, creature.input, creature.output, true);
        assert!(
            sum.abs() <= 1e-9,
            "batched scoring disagreed with the documented branches: summed MSE {sum}"
        );
    }
}

/// A dropped `type` key must change the answer — this is the mutation guard
/// that proves the role assertions above are not vacuous.
#[test]
fn stripping_synapse_roles_changes_the_branch_output() {
    let mut stripped = stump_creature();
    for s in &mut stripped.synapses {
        s.synapse_type = None;
    }
    // Without roles every inbound synapse becomes "positive", so the condition
    // sum is zero and the IF neuron always takes the (now empty) negative
    // branch: the above-threshold case no longer returns the positive leaf.
    let x = STUMP_THRESHOLD as f32 + 0.25;
    let intact = activate_once(&stump_creature(), &[x]);
    let broken = activate_once(&stripped, &[x]);
    assert!(
        (intact - broken).abs() > TOL,
        "dropping synapse roles must change the output ({intact} vs {broken})"
    );
}
