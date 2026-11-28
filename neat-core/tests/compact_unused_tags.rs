//! Behaviour of the newborn-grace and pass-tag helpers for `compactUnused` (Issue #778).

use neat_core::{
    CompactUnusedPassTags, CreatureExport, NEWBORN_GRACE_TAG, NeuronExport, NewbornGraceAgeing,
    SynapseExport, age_newborn_grace, compact_unused_pass_tags, newborn_grace_remaining,
};

#[test]
fn the_grace_tag_is_named_as_in_the_original() {
    assert_eq!(NEWBORN_GRACE_TAG, "newborn-grace");
}

#[test]
fn newborn_grace_remaining_reads_the_tag_like_parse_int() {
    let long_nines = "9".repeat(400);
    let table: Vec<(Option<&str>, u32, &str)> = vec![
        (None, 0, "no tag"),
        (Some(""), 0, "empty"),
        (Some("1"), 1, "1"),
        (Some("2"), 2, "2"),
        (Some("0"), 0, "0"),
        (Some("-3"), 0, "-3"),
        (Some("-0"), 0, "-0"),
        (Some("abc"), 0, "abc"),
        (Some("3abc"), 3, "3abc"),
        (Some("  4"), 4, "two spaces then 4"),
        (Some("\u{FEFF}5"), 5, "BOM then 5"),
        (Some("\u{00A0}6"), 6, "NBSP then 6"),
        (Some("\t\n7"), 7, "tab newline 7"),
        (Some("\u{0085}7"), 0, "NEL is not JS whitespace"),
        (Some("+8"), 8, "+8"),
        (Some("--1"), 0, "--1"),
        (Some("+-1"), 0, "+-1"),
        (Some("1.9"), 1, "1.9"),
        (Some("1e3"), 1, "1e3"),
        (Some("0x10"), 0, "0x10"),
        (Some("007"), 7, "007"),
        (Some("4294967295"), u32::MAX, "u32::MAX"),
        (Some("4294967296"), u32::MAX, "u32::MAX + 1 saturates"),
        (Some(long_nines.as_str()), 0, "400 nines is Infinity in JS"),
    ];
    for (input, expected, label) in table {
        assert_eq!(newborn_grace_remaining(input), expected, "input: {label}");
    }
}

#[test]
fn age_newborn_grace_decides_what_happens_to_the_tag() {
    use NewbornGraceAgeing::{Keep, Remove, Set};
    let table: Vec<(Option<&str>, NewbornGraceAgeing)> = vec![
        (None, Keep),
        (Some(""), Keep),
        (Some("0"), Remove),
        (Some("junk"), Remove),
        (Some("-2"), Remove),
        (Some("1"), Remove),
        (Some("2"), Set("1".to_string())),
        (Some("10"), Set("9".to_string())),
        (Some(" 3"), Set("2".to_string())),
        (Some("02"), Set("1".to_string())),
    ];
    for (input, expected) in table {
        assert_eq!(age_newborn_grace(input), expected, "input: {input:?}");
    }
}

fn neuron(kind: &str, uuid: &str) -> NeuronExport {
    NeuronExport {
        id: None,
        neuron_type: kind.to_string(),
        uuid: uuid.to_string(),
        bias: 0.0,
        squash: Some("IDENTITY".to_string()),
    }
}

fn synapse(from: &str, to: &str) -> SynapseExport {
    SynapseExport {
        from_uuid: from.to_string(),
        to_uuid: to.to_string(),
        weight: 1.0,
        synapse_type: None,
    }
}

fn creature(
    input: usize,
    output: usize,
    neurons: Vec<NeuronExport>,
    synapses: usize,
) -> CreatureExport {
    CreatureExport {
        input,
        output,
        neurons,
        synapses: (0..synapses)
            .map(|_| synapse("input-0", "output-0"))
            .collect(),
        semantic_version: None,
        forward_only: true,
        memetic: None,
    }
}

#[test]
fn pass_tags_count_listed_neurons_less_outputs() {
    // Inputs are not listed in export neurons, so 3 listed - 1 output = 2.
    let a = creature(
        2,
        1,
        vec![
            neuron("hidden", "hidden-0"),
            neuron("hidden", "hidden-1"),
            neuron("output", "output-0"),
        ],
        4,
    );
    assert_eq!(
        compact_unused_pass_tags(&a),
        CompactUnusedPassTags {
            old_neurons: 2,
            old_synapses: 4
        }
    );
}

#[test]
fn pass_tags_count_a_constant_neuron() {
    // 4 listed (constant, hidden, two outputs) - 2 outputs = 2.
    let b = creature(
        1,
        2,
        vec![
            neuron("constant", "c-0"),
            neuron("hidden", "h-0"),
            neuron("output", "output-0"),
            neuron("output", "output-1"),
        ],
        3,
    );
    assert_eq!(
        compact_unused_pass_tags(&b),
        CompactUnusedPassTags {
            old_neurons: 2,
            old_synapses: 3
        }
    );
}

#[test]
fn pass_tags_ignore_the_declared_input_count() {
    let c = creature(
        5,
        1,
        vec![
            neuron("hidden", "hidden-0"),
            neuron("hidden", "hidden-1"),
            neuron("output", "output-0"),
        ],
        4,
    );
    assert_eq!(
        compact_unused_pass_tags(&c),
        CompactUnusedPassTags {
            old_neurons: 2,
            old_synapses: 4
        }
    );
}
