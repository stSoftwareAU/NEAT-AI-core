//! Issue #759: input-to-output reachability diagnostics.
//!
//! Every expected class below is derived by hand from the drawn graph, not
//! produced by the code under test.

use neat_core::reachability::{
    ClassCounts, ReachabilityClass as C, ReachabilityError as E, reachability_diagnostics,
};
use neat_core::{cleanup_creature, parse_creature_json};

/// Splits `(from, to)` pairs into the parallel arrays the API takes.
fn split(edges: &[(u32, u32)]) -> (Vec<u32>, Vec<u32>) {
    (
        edges.iter().map(|e| e.0).collect(),
        edges.iter().map(|e| e.1).collect(),
    )
}

/// Ten neurons: inputs 0,1; interior 2..=7; outputs 8,9.
///
/// Edges: 0->2, 2->8 (0 reaches 2, 2 reaches 8: Live)
///        4->3, 3->8 (3 reads constant 4 only; reaches 8: BiasDriven; 4 too)
///        0->5       (5 reached from input, no outward edge: DeadEnd)
///        6 has no edges at all (Isolated); 7 has no edges (Isolated)
///        1->9       (input 1 feeds output 9)
/// Input 0 reaches 8 via 2 so Live; input 1 reaches 9 so Live.
/// Output 8 is reached from input 0; output 9 from input 1: both Live.
fn four_class_edges() -> (Vec<u32>, Vec<u32>) {
    split(&[(0, 2), (2, 8), (4, 3), (3, 8), (0, 5), (1, 9)])
}

#[test]
fn all_four_classes_are_assigned_by_hand_derivation() {
    let (f, t) = four_class_edges();
    let r = reachability_diagnostics(&f, &t, 10, 2, 2).unwrap();
    assert_eq!(
        r.classes(),
        &[
            C::Live,       // 0 input -> 2 -> 8
            C::Live,       // 1 input -> 9
            C::Live,       // 2
            C::BiasDriven, // 3 fed by constant 4, reaches 8
            C::BiasDriven, // 4 feeds 3, nothing input-reached
            C::DeadEnd,    // 5 fed by input 0, no outward edge
            C::Isolated,   // 6
            C::Isolated,   // 7
            C::Live,       // 8 output
            C::Live,       // 9 output
        ]
    );
    assert_eq!(
        r.interior_counts(),
        ClassCounts {
            live: 1,
            bias_driven: 2,
            dead_end: 1,
            isolated: 2
        }
    );
    assert_eq!(r.class_of(5), Some(C::DeadEnd));
    assert_eq!(r.class_of(10), None);
}

#[test]
fn unused_input_and_input_independent_output_are_listed() {
    // 4 neurons: inputs 0,1; outputs 2,3. Edges: 0->2 only.
    // Input 1 reaches nothing: DeadEnd. Output 3 has no inward edge:
    // reaches an output (itself) but no input reaches it: BiasDriven.
    let (f, t) = split(&[(0, 2)]);
    let r = reachability_diagnostics(&f, &t, 4, 2, 2).unwrap();
    assert_eq!(r.classes(), &[C::Live, C::DeadEnd, C::Live, C::BiasDriven]);
    assert_eq!(r.unused_inputs(), vec![1]);
    assert_eq!(r.input_independent_outputs(), vec![3]);
}

#[test]
fn recurrent_cycles_terminate_and_classify() {
    // 6 neurons: input 0; interior 1,2,3,4; output 5.
    // 0->1, 1->2, 2->1 (back edge), 2->5: 1 and 2 are Live.
    // 3<->4 is an island feeding only itself: Isolated.
    let (f, t) = split(&[(0, 1), (1, 2), (2, 1), (2, 5), (3, 4), (4, 3)]);
    let r = reachability_diagnostics(&f, &t, 6, 1, 1).unwrap();
    assert_eq!(
        r.classes(),
        &[C::Live, C::Live, C::Live, C::Isolated, C::Isolated, C::Live]
    );
}

#[test]
fn duplicate_and_unsorted_edges_match_the_clean_list() {
    let (cf, ct) = four_class_edges();
    let clean = reachability_diagnostics(&cf, &ct, 10, 2, 2).unwrap();
    let (f, t) = split(&[
        (1, 9),
        (3, 8),
        (0, 5),
        (0, 2),
        (4, 3),
        (2, 8),
        (0, 2),
        (1, 9),
        (4, 3),
    ]);
    assert_eq!(reachability_diagnostics(&f, &t, 10, 2, 2).unwrap(), clean);
}

#[test]
fn mismatched_edge_lengths_are_refused() {
    assert_eq!(
        reachability_diagnostics(&[0, 0], &[1], 2, 1, 1),
        Err(E::MismatchedEdgeLengths {
            from_len: 2,
            to_len: 1
        })
    );
    assert!(reachability_diagnostics(&[0], &[1], 2, 1, 1).is_ok());
}

#[test]
fn invalid_widths_are_refused() {
    let w = |n, i, o| E::InvalidWidths {
        num_neurons: n,
        num_inputs: i,
        num_outputs: o,
    };
    assert_eq!(reachability_diagnostics(&[], &[], 2, 2, 1), Err(w(2, 2, 1)));
    assert!(reachability_diagnostics(&[], &[], 3, 2, 1).is_ok());
    assert_eq!(reachability_diagnostics(&[], &[], 2, 1, 0), Err(w(2, 1, 0)));
    assert!(reachability_diagnostics(&[], &[], 2, 1, 1).is_ok());
    assert_eq!(
        reachability_diagnostics(&[], &[], 5, usize::MAX, 2),
        Err(w(5, usize::MAX, 2))
    );
}

#[test]
fn out_of_range_edges_are_refused() {
    assert_eq!(
        reachability_diagnostics(&[0, 0], &[1, 3], 3, 1, 1),
        Err(E::EdgeOutOfRange {
            edge_index: 1,
            from: 0,
            to: 3,
            num_neurons: 3
        })
    );
    assert!(reachability_diagnostics(&[0, 0], &[1, 2], 3, 1, 1).is_ok());
    assert_eq!(
        reachability_diagnostics(&[7], &[1], 3, 1, 1),
        Err(E::EdgeOutOfRange {
            edge_index: 0,
            from: 7,
            to: 1,
            num_neurons: 3
        })
    );
}

#[test]
fn cleaned_forward_only_creature_has_only_live_or_bias_driven_interior() {
    // Indices: inputs 0,1; then listed neurons: h-live 2, h-dead 3,
    // h-chain 4, output 5. h-dead and h-chain feed nothing useful, so
    // cleanup removes them (h-chain only feeds h-dead).
    let creature = parse_creature_json(
        r#"{
          "input":2,"output":1,"forwardOnly":true,
          "neurons":[
            {"type":"hidden","uuid":"h-live","bias":0.1,"squash":"LOGISTIC"},
            {"type":"hidden","uuid":"h-dead","bias":0.0,"squash":"IDENTITY"},
            {"type":"hidden","uuid":"h-chain","bias":0.0,"squash":"IDENTITY"},
            {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
          ],
          "synapses":[
            {"weight":1.0,"fromUUID":"input-0","toUUID":"h-live"},
            {"weight":1.0,"fromUUID":"input-1","toUUID":"h-live"},
            {"weight":1.0,"fromUUID":"h-live","toUUID":"output-0"},
            {"weight":1.0,"fromUUID":"input-1","toUUID":"h-chain"},
            {"weight":1.0,"fromUUID":"h-chain","toUUID":"h-dead"}
          ]
        }"#,
    )
    .unwrap();

    let index_of = |c: &neat_core::creature::CreatureExport, uuid: &str| -> u32 {
        if let Some(n) = uuid.strip_prefix("input-") {
            return n.parse().unwrap();
        }
        let pos = c.neurons.iter().position(|n| n.uuid == uuid).unwrap();
        (c.input as usize + pos) as u32
    };

    // Before cleanup the dead structure is present and flagged.
    let (f, t): (Vec<u32>, Vec<u32>) = creature
        .synapses
        .iter()
        .map(|s| {
            (
                index_of(&creature, &s.from_uuid),
                index_of(&creature, &s.to_uuid),
            )
        })
        .unzip();
    let before = reachability_diagnostics(&f, &t, 2 + 4, 2, 1).unwrap();
    assert_eq!(before.interior_counts().dead_end, 2);

    let cleaned = cleanup_creature(&creature).unwrap().creature;
    let (f, t): (Vec<u32>, Vec<u32>) = cleaned
        .synapses
        .iter()
        .map(|s| {
            (
                index_of(&cleaned, &s.from_uuid),
                index_of(&cleaned, &s.to_uuid),
            )
        })
        .unzip();
    let n = cleaned.input as usize + cleaned.neurons.len();
    let after = reachability_diagnostics(&f, &t, n, 2, 1).unwrap();
    assert_eq!(cleaned.neurons.len(), 2, "h-live and output survive");
    let c = after.interior_counts();
    assert_eq!((c.dead_end, c.isolated), (0, 0));
    assert_eq!(c.live, 1);
}

#[cfg(target_pointer_width = "64")]
#[test]
fn a_neuron_count_beyond_u32_is_refused_before_any_allocation() {
    // A per-neuron vector for this count would be tens of gigabytes; reaching
    // the error at all (rather than aborting) shows the check precedes it.
    let too_many = u32::MAX as usize + 1;
    assert_eq!(
        reachability_diagnostics(&[], &[], too_many, 1, 1),
        Err(E::InvalidWidths {
            num_neurons: too_many,
            num_inputs: 1,
            num_outputs: 1,
        })
    );
}

#[test]
fn a_small_legal_neuron_count_is_still_accepted() {
    // 2 neurons: input 0, output 1, edge 0 -> 1: both Live.
    let r = reachability_diagnostics(&[0], &[1], 2, 1, 1).unwrap();
    assert_eq!(r.classes(), &[C::Live, C::Live]);
}
