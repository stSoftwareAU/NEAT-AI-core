//! `IF`-shaped creatures both pruning entry points are graded on (Ockham #198).
//!
//! `neat-core/tests/prune_neuron.rs` and `neat-core/tests/prune_synapse.rs`
//! assert that a neuron removal and the synapse removal taking the same term
//! away answer the same rewrite. That claim is only worth anything while both
//! are asked about the **same** creature, so these two live here rather than
//! once per test file where the copies could drift apart.
//!
//! Included by path (`#[path = "common/prune_if.rs"] mod prune_if;`) rather
//! than through `common/mod.rs`, so neither target compiles a helper it does
//! not use.

/// `if-1`'s condition is decided by the creature itself: `h-c1` and `h-c2` sum
/// nothing, so each is worth `IDENTITY(bias)` on every record and the condition
/// is `1.0 - 0.5 = +0.5` — the positive branch, always. Dropping `h-c2`'s term
/// leaves `+1.0` and the same branch; dropping `h-c1`'s leaves `-0.5` and flips
/// it. `h-n` feeds only the arm the condition never reaches.
pub const IF_STATIC_CONDITION_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":1,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-c1","bias":1.0,"squash":"IDENTITY"},
    {"type":"hidden","uuid":"h-c2","bias":-0.5,"squash":"IDENTITY"},
    {"type":"hidden","uuid":"h-p","bias":0.0,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-n","bias":0.0,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"if-1","bias":0.25,"squash":"IF"},
    {"type":"output","uuid":"output-0","bias":0.0,"squash":"IDENTITY"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-p"},
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-n"},
    {"weight":1.0,"fromUUID":"h-c1","toUUID":"if-1","type":"condition"},
    {"weight":1.0,"fromUUID":"h-c2","toUUID":"if-1","type":"condition"},
    {"weight":2.0,"fromUUID":"h-p","toUUID":"if-1","type":"positive"},
    {"weight":-3.0,"fromUUID":"h-n","toUUID":"if-1","type":"negative"},
    {"weight":1.0,"fromUUID":"if-1","toUUID":"output-0"}
  ]
}"#;

/// The **output** neuron itself carries the `IF` squash (corner case 6): the
/// declared target width means it can never be removed or replaced, so a role
/// it loses has to be repaired in place. `h-cond` varies with the record, so
/// losing it flattens the output and losing a branch source restores that
/// branch instead.
pub const OUTPUT_IF_JSON: &str = r#"{
  "semanticVersion":"4.0.0","forwardOnly":true,"input":2,"output":1,
  "neurons":[
    {"type":"hidden","uuid":"h-cond","bias":0.1,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-p","bias":0.2,"squash":"LOGISTIC"},
    {"type":"hidden","uuid":"h-n","bias":0.3,"squash":"LOGISTIC"},
    {"type":"output","uuid":"output-0","bias":0.05,"squash":"IF"}
  ],
  "synapses":[
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-cond"},
    {"weight":1.0,"fromUUID":"input-1","toUUID":"h-p"},
    {"weight":1.0,"fromUUID":"input-0","toUUID":"h-n"},
    {"weight":1.0,"fromUUID":"h-cond","toUUID":"output-0","type":"condition"},
    {"weight":2.0,"fromUUID":"h-p","toUUID":"output-0","type":"positive"},
    {"weight":-3.0,"fromUUID":"h-n","toUUID":"output-0","type":"negative"}
  ]
}"#;
