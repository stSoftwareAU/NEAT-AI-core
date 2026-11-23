//! Allocation regression for the capped IDENTITY splice (NEAT-AI#4096).
//!
//! Every hidden `IDENTITY` neuron is a splice candidate, and a candidate that
//! the net-new-synapse cap refuses used to be planned in full first: the whole
//! synapse list cloned, then a linear `plan_edge` lookup per rewired edge. One
//! cleanup pass therefore cost O(identity neurons x synapses) — 18 s on a
//! 66,000-synapse creature — to leave the creature exactly as it was.
//!
//! The answer is unchanged by that waste, so only the memory it costs can pin
//! the fix: a counting allocator totals the bytes handed out across one
//! cleanup, and the total must stay far below what a per-candidate clone of the
//! synapse list necessarily bills. Modelled on
//! `tests/creature_width_allocations.rs`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use neat_core::{
    CleanupOptions, CreatureExport, IfRepair, SynapseExport, cleanup_creature_with,
    parse_creature_json,
};

thread_local! {
    /// Bytes handed to this thread; `const`-initialised, monotonic, no
    /// destructor — see `creature_width_allocations.rs` for why each matters.
    static THREAD_BYTES: Cell<usize> = const { Cell::new(0) };
}

fn bill(bytes: usize) {
    let _ = THREAD_BYTES.try_with(|counter| counter.set(counter.get().saturating_add(bytes)));
}

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

const RELAYS: usize = 200;
const FILLER: usize = 1000;

/// `RELAYS` hidden `IDENTITY` relays, each 5 observations in and 5 outputs out
/// (25 rewired edges against 10 removed: 15 net new, so every splice is
/// refused), beside `FILLER` `TANH` neurons that only add synapses to the list.
fn creature() -> CreatureExport {
    let mut neurons = Vec::new();
    let mut synapses = Vec::new();
    for o in 0..5 {
        neurons.push(format!(
            r#"{{"type":"output","uuid":"output-{o}","bias":0.0,"squash":"IDENTITY"}}"#
        ));
    }
    for r in 0..RELAYS {
        neurons.push(format!(
            r#"{{"type":"hidden","uuid":"relay-{r}","bias":0.0,"squash":"IDENTITY"}}"#
        ));
        for i in 0..5 {
            synapses.push(format!(
                r#"{{"weight":{:?},"fromUUID":"input-{i}","toUUID":"relay-{r}"}}"#,
                1.0 + i as f64
            ));
        }
        for o in 0..5 {
            synapses.push(format!(
                r#"{{"weight":{:?},"fromUUID":"relay-{r}","toUUID":"output-{o}"}}"#,
                0.5 + o as f64
            ));
        }
    }
    for f in 0..FILLER {
        neurons.push(format!(
            r#"{{"type":"hidden","uuid":"filler-{f}","bias":0.1,"squash":"TANH"}}"#
        ));
        for i in 0..5 {
            synapses.push(format!(
                r#"{{"weight":0.5,"fromUUID":"input-{i}","toUUID":"filler-{f}"}}"#
            ));
        }
        synapses.push(format!(
            r#"{{"weight":0.25,"fromUUID":"filler-{f}","toUUID":"output-{}"}}"#,
            f % 5
        ));
    }
    parse_creature_json(&format!(
        r#"{{"semanticVersion":"4.0.0","forwardOnly":true,"input":5,"output":5,
        "neurons":[{}],"synapses":[{}]}}"#,
        neurons.join(","),
        synapses.join(",")
    ))
    .expect("fixture parses")
}

#[test]
fn refused_splice_candidates_do_not_clone_the_synapse_list() {
    let before = creature();
    let synapses = before.synapses.len();
    assert_eq!(synapses, RELAYS * 10 + FILLER * 6);

    THREAD_BYTES.with(|c| c.set(0));
    let outcome = cleanup_creature_with(
        &before,
        CleanupOptions {
            if_repair: IfRepair::Rewrite,
            splice_identity: true,
        },
    )
    .expect("cleanup succeeds");
    let billed = THREAD_BYTES.with(|c| c.get());

    // The answer: every splice refused, nothing else to do.
    // (`changed` is not asserted: the fixture is not in canonical synapse
    // order, so cleanup may legitimately reorder it.)
    assert!(outcome.spliced_neurons.is_empty());
    assert_eq!(outcome.creature.neurons.len(), before.neurons.len());
    assert_eq!(outcome.creature.synapses.len(), synapses);
    let key = |s: &SynapseExport| (s.from_uuid.clone(), s.to_uuid.clone());
    let mut want: Vec<_> = before.synapses.iter().map(|s| (key(s), s.weight)).collect();
    let mut got: Vec<_> = outcome
        .creature
        .synapses
        .iter()
        .map(|s| (key(s), s.weight))
        .collect();
    want.sort_by(|a, b| a.0.cmp(&b.0));
    got.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(got, want, "same edges and weights, whatever the order");

    // A per-candidate clone bills at least `RELAYS x synapses x
    // size_of::<SynapseExport>()` for the list alone, before the two uuid
    // `String`s each clone also allocates. A quarter of that is the ceiling:
    // the pass itself clones the creature a handful of times, which is a small
    // multiple of one list, not `RELAYS` of them.
    let ceiling = RELAYS * synapses * std::mem::size_of::<SynapseExport>() / 4;
    assert!(
        billed < ceiling,
        "cleanup allocated {billed} bytes, ceiling {ceiling}: a refused splice is cloning the list"
    );
}
