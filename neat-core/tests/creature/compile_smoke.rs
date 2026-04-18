use neat_core::{compile_creature, parse_creature_json};

#[test]
fn compile_minimal_creature_using_common_fixture() {
    let creature = parse_creature_json(crate::common::minimal_creature_json()).expect("parse");
    let net = compile_creature(&creature).expect("compile");
    assert_eq!(net.num_inputs(), 2);
    assert!(net.num_synapses() >= 3);
}
