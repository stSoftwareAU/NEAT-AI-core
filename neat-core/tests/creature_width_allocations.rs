//! Allocation regression for the declared observation width (Issue #622).
//!
//! `CreatureExport::input` is a *declared* count with no backing data in the
//! JSON: a payload under 100 bytes can say `"input": 100000000`. Both
//! `compile_creature` and `validate_creature_topology` used to build a UUID map
//! with one owned `String` key per declared input *before* anything bounded the
//! count against [`MAX_NODE_COUNT`], so the declared width — not the payload —
//! set the memory the process spent, and a large enough literal aborted on the
//! allocation instead of returning an error.
//!
//! The cost is observable without touching the implementation: a counting
//! global allocator totals the bytes handed out across one refusal. A width
//! that is walked before it is bounded shows up as a total proportional to the
//! declared count, so quadrupling the declaration quadruples the bill; a width
//! bounded first does not move at all. Comparing the two readings is what makes
//! the assertion hold on any machine — an absolute byte figure would not.
//!
//! `creature_validate` and its standalone synapse half walk the same declared
//! width and were left out of that fix (Issue #639). They owe NEAT-AI's own
//! rule wording rather than a typed `CreatureError`, so they carry the ceiling
//! as a rule of their own; the cost of the refusal is measured here exactly as
//! it is for the two above.
//!
//! `MemeticExport::prune_to` — and so `CreatureExport::prune_memetic` — walked
//! it too, and answers with `()`, so it has nowhere to report a refusal
//! (Issue #650). It is bounded the other way instead: it resolves the implicit
//! input neurons arithmetically rather than materialising one view each, so its
//! cost follows the payload and no refusal is needed. The measurement is the
//! same comparison — quadruple the declaration, the bill must not move — over
//! a prune that *succeeds* rather than a refusal, and the outcome is asserted
//! beside the cost so a prune that silently did nothing cannot read as a pass.
//!
//! Modelled on `tests/topology_ops_allocations.rs`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use neat_core::if_graft::{GraftError, validate_creature_topology};
use neat_core::network::MAX_NODE_COUNT;
use neat_core::{
    CreatureError, CreatureExport, MemeticExport, MemeticWeightRowExport, MemeticWeights,
    NeuronExport, SynapseExport, ValidateOptions, ValidationFailure, ValidationStats,
    compile_creature, creature_validate, validate_synapse_and_memetic_rules,
};
use std::collections::BTreeMap;

thread_local! {
    /// Bytes handed to **this thread** since it last zeroed the counter.
    ///
    /// Per-thread rather than process-wide: `quality.sh` runs the suite with
    /// `--test-threads=2`, so the two tests below execute concurrently and a
    /// global counter bills each of them for the other's allocations — a real
    /// failure this file saw. A thread-local counter needs no lock and is
    /// exact, because every allocation a measured call makes happens on the
    /// thread that made the call.
    ///
    /// `const`-initialised on purpose: a lazily initialised thread-local would
    /// allocate on first touch, and the first touch is *inside* the allocator.
    /// `Cell<usize>` has no destructor either, so there is no TLS-teardown
    /// window in which the access could fail.
    ///
    /// The counter is **monotonic** — `dealloc` does not decrement it — so
    /// zeroing it before a measurement can never leave it out of step with
    /// memory that was already live. A live-bytes counter cannot: freeing
    /// something allocated before the reset underflows it, and the arithmetic
    /// then panics *inside* the allocator, which deadlocks the harness rather
    /// than failing a test.
    static THREAD_BYTES: Cell<usize> = const { Cell::new(0) };
}

/// Bill the calling thread for `bytes`, saturating rather than wrapping so a
/// runaway walk cannot roll the reading back around to a small number.
fn bill(bytes: usize) {
    let _ = THREAD_BYTES.try_with(|counter| counter.set(counter.get().saturating_add(bytes)));
}

/// Global allocator forwarding to the system allocator while totalling the
/// bytes it hands out, per thread.
///
/// `realloc` is deliberately *not* forwarded to `System.realloc` — the default
/// `GlobalAlloc::realloc` allocates, copies and frees, which is the behaviour a
/// growing `HashMap` must be assumed to have. That makes the figure
/// deterministic across platforms instead of depending on whether the system
/// allocator happened to extend a block in place.
struct TrackingAllocator;

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        bill(layout.size());
        // SAFETY: forwarding an unchanged layout to the system allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        bill(layout.size());
        // SAFETY: forwarding an unchanged layout to the system allocator.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr`/`layout` came from an `alloc` call above.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;

/// A declared width far above [`MAX_NODE_COUNT`], kept small enough that an
/// unbounded walk still finishes rather than exhausting the test runner: the
/// point is the *shape* of the cost, not how far it can be pushed.
const OVERSIZED_INPUT: usize = 1_000_000;

/// The same declaration, four times as wide. Both are answered the same way, so
/// the difference between the two costs is what the declared width bought — see
/// [`GROWTH_SLACK_BYTES`].
const QUADRUPLED_INPUT: usize = 4 * OVERSIZED_INPUT;

/// Floor on the heap one *walked* input costs: the `input-N` map entry holds a
/// `String` handle (24 B) beside its `usize` index (8 B), before the name's own
/// heap and the table's load-factor headroom. Deliberately conservative — the
/// figure this file actually measures against the unbounded walk is ~81 B per
/// input — so anything derived from it understates the walk rather than
/// flattering the fix.
const BYTES_PER_WALKED_INPUT: usize = 32;

/// Bytes one call may allocate in total, derived rather than picked: answering
/// on an impossible width must cost less than *accepting* the widest creature
/// the `u16` index space allows, which is the largest walk this crate ever
/// legitimately performs. At [`BYTES_PER_WALKED_INPUT`] that is 2 MiB, and the
/// bounded entry points in fact spend none of it.
const WIDTH_BUDGET_BYTES: usize = MAX_NODE_COUNT * BYTES_PER_WALKED_INPUT;

/// How far the cost of one call may move when the declared width is
/// **quadrupled**. A width that is walked before it is bounded quadruples with
/// it (tens of megabytes here); a width bounded first does not move at all, so
/// a few kilobytes of slack covers allocator noise without covering a walk.
///
/// This is the assertion that survives a change of machine: it compares two
/// readings of the same work rather than one reading against a constant.
const GROWTH_SLACK_BYTES: usize = 4 * 1024;

/// Structurally valid apart from the declared width, so any rejection can only
/// come from the width rule.
fn creature_declaring(input: usize) -> CreatureExport {
    CreatureExport {
        memetic: None,
        input,
        output: 1,
        neurons: vec![NeuronExport {
            id: None,
            neuron_type: "output".to_string(),
            uuid: "output-0".to_string(),
            bias: 0.0,
            squash: Some("IDENTITY".to_string()),
        }],
        synapses: vec![SynapseExport {
            from_uuid: "input-0".to_string(),
            to_uuid: "output-0".to_string(),
            weight: 1.0,
            synapse_type: None,
        }],
        semantic_version: None,
        forward_only: false,
    }
}

/// Bytes this thread allocated while `refuse` turned `input` away.
///
/// The creature is built *before* the counter is zeroed, so the reading covers
/// the refusal alone and nothing that set it up.
fn cost_of_refusing(input: usize, refuse: impl Fn(&CreatureExport) -> bool) -> usize {
    let creature = creature_declaring(input);

    THREAD_BYTES.with(|counter| counter.set(0));
    let refused = refuse(&creature);
    let spent = THREAD_BYTES.with(Cell::get);

    assert!(refused, "a declared input of {input} must be refused");
    spent
}

/// The two readings a bounded width must produce, whatever the entry point did
/// with it: small in absolute terms, and unmoved by quadrupling the declared
/// count. `cost` is billed the bytes one call spends at the width it is given.
fn assert_cost_does_not_follow_the_width(entry_point: &str, cost: impl Fn(usize) -> usize) {
    let at_width = cost(OVERSIZED_INPUT);
    let at_quadruple = cost(QUADRUPLED_INPUT);
    let growth = at_quadruple.saturating_sub(at_width);

    assert!(
        at_width <= WIDTH_BUDGET_BYTES,
        "{entry_point} allocated {at_width} B against a declared input of {OVERSIZED_INPUT} \
         (budget {WIDTH_BUDGET_BYTES} B, ceiling {MAX_NODE_COUNT}); \
         the declared width is being walked before it is bounded"
    );
    assert!(
        growth <= GROWTH_SLACK_BYTES,
        "{entry_point} spent {growth} B more at {QUADRUPLED_INPUT} inputs than at \
         {OVERSIZED_INPUT} ({at_quadruple} B against {at_width} B, slack \
         {GROWTH_SLACK_BYTES} B); the cost still scales with the declared width"
    );
}

/// The same two readings over an entry point that answers by *refusing* the
/// width.
fn assert_width_is_not_walked(entry_point: &str, refuse: impl Fn(&CreatureExport) -> bool) {
    assert_cost_does_not_follow_the_width(entry_point, |input| cost_of_refusing(input, &refuse));
}

#[test]
fn compiling_refuses_an_oversized_declared_input_without_paying_for_it() {
    assert_width_is_not_walked("compile_creature", |creature| {
        matches!(
            compile_creature(creature),
            Err(CreatureError::TooManyNodes { .. })
        )
    });
}

#[test]
fn topology_validation_refuses_an_oversized_declared_input_without_paying_for_it() {
    assert_width_is_not_walked("validate_creature_topology", |creature| {
        matches!(
            validate_creature_topology(creature),
            Err(GraftError::Creature(CreatureError::TooManyNodes { .. }))
        )
    });
}

/// Whether a validator answered with the ceiling refusal (Issue #639).
///
/// Any *other* answer fails loud naming what came back, rather than returning
/// `false` and leaving the caller's "must be refused" assertion to guess: a
/// verdict on a creature this wide is a different fault from refusing it for
/// the wrong reason, and the two must not read alike.
fn refused_for_being_oversized<T: std::fmt::Debug>(answer: Result<T, ValidationFailure>) -> bool {
    match answer {
        Err(failure) if failure.message.contains("exceeding the maximum") => true,
        Err(failure) => panic!("refused, but not for its declared width: {failure}"),
        Ok(accepted) => panic!("a declared width past the ceiling was accepted: {accepted:?}"),
    }
}

#[test]
fn creature_validate_refuses_an_oversized_declared_input_without_paying_for_it() {
    // Issue #639 — the third entry point that walks the declared width. It
    // owes NEAT-AI's own rule wording rather than a typed `CreatureError`, so
    // the refusal is a `ValidationFailure`; what is measured here is the same
    // thing as above, that the refusal is decided before the walk pays for it.
    assert_width_is_not_walked("creature_validate", |creature| {
        refused_for_being_oversized(creature_validate(creature, &ValidateOptions::default()))
    });
}

#[test]
fn the_synapse_half_refuses_an_oversized_declared_input_without_paying_for_it() {
    // Rules 23–31 are callable on their own and derive the same views from the
    // same declared width, so the ceiling is theirs too (Issue #639).
    assert_width_is_not_walked("validate_synapse_and_memetic_rules", |creature| {
        let mut stats = ValidationStats::default();
        refused_for_being_oversized(validate_synapse_and_memetic_rules(
            creature,
            &ValidateOptions::default(),
            &mut stats,
        ))
    });
}

/// One UUID-keyed memetic weight row.
fn row(from: &str, to: &str, weight: f64) -> MemeticWeightRowExport {
    MemeticWeightRowExport {
        from_uuid: Some(from.to_string()),
        to_uuid: Some(to.to_string()),
        weight: Some(weight),
    }
}

/// Bytes this thread allocated pruning a memetic record against a creature
/// declaring `input` observations — and the assertion that the prune actually
/// pruned.
///
/// The record names one live reference and one dangling one in each half, so a
/// prune that dropped everything, or nothing, fails here rather than reading as
/// a cheap pass. The creature and the record are built *before* the counter is
/// zeroed, so the reading covers the prune alone.
fn cost_of_pruning(input: usize) -> usize {
    let mut creature = creature_declaring(input);
    creature.memetic = Some(MemeticExport {
        biases: BTreeMap::from([
            // The implicit input neuron `input-0` — resolvable, and by both
            // vocabularies: the wire UUID here, its runtime id below.
            ("input-0".to_string(), 0.5),
            ("0".to_string(), 0.25),
            ("no-such-neuron".to_string(), 0.75),
        ]),
        weights: MemeticWeights::Rows(vec![
            row("input-0", "output-0", 0.9),
            row("input-0", "no-such-neuron", 0.1),
        ]),
        extra: serde_json::Map::new(),
    });

    THREAD_BYTES.with(|counter| counter.set(0));
    creature.prune_memetic();
    let spent = THREAD_BYTES.with(Cell::get);

    let memetic = creature
        .memetic
        .as_ref()
        .expect("the record itself survives");
    assert_eq!(
        memetic.biases.keys().collect::<Vec<_>>(),
        vec!["0", "input-0"],
        "both vocabularies still resolve the implicit input; the dangling bias goes"
    );
    let surviving: Vec<(&str, &str)> = match &memetic.weights {
        MemeticWeights::Rows(rows) => rows
            .iter()
            .map(|r| {
                (
                    r.from_uuid.as_deref().unwrap_or_default(),
                    r.to_uuid.as_deref().unwrap_or_default(),
                )
            })
            .collect(),
        MemeticWeights::ById(_) => panic!("the fixture is the row form"),
    };
    assert_eq!(
        surviving,
        vec![("input-0", "output-0")],
        "only the row naming a neuron the creature does not carry is dropped"
    );

    spent
}

#[test]
fn pruning_a_memetic_record_does_not_walk_the_declared_input_width() {
    // Issue #650 — the one entry point that cannot report a refusal: it answers
    // with `()`. It is bounded by deriving the implicit input neurons
    // arithmetically instead, so the cost follows the payload; `cost_of_pruning`
    // asserts the prune still resolved both vocabularies while spending it.
    assert_cost_does_not_follow_the_width("CreatureExport::prune_memetic", cost_of_pruning);
}
