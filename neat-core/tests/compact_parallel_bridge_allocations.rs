//! Allocation regression for `merge_parallel_bridges` and the declared input
//! width (Issue #777, AGENTS.md "One width check for creature JSON").
//!
//! `CreatureExport::input` is a declared count that the payload never backs. The
//! first port of `mergeParallelBridges` built one owned `input-N` `String` and
//! one map entry per declared input before it looked at a single candidate. So
//! the declaration, not the payload, set what a merge cost.
//!
//! The pass answers with a merged creature, not a refusal, so it is bounded the
//! way `MemeticExport::prune_to` is (Issue #650): an `input-N` UUID is resolved
//! when it is looked up, and the declared range is never materialised. The
//! measurement is the one `creature_width_allocations.rs` uses. Quadruple the
//! declaration and the bill must not move. Each reading also asserts that the
//! merge actually happened, so a pass that silently did nothing cannot pass.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use neat_core::network::MAX_NODE_COUNT;
use neat_core::{CreatureExport, NeuronExport, SynapseExport, merge_parallel_bridges};

thread_local! {
    /// Bytes handed to this thread since it last zeroed the counter. It is
    /// per-thread, `const`-initialised and monotonic for the reasons spelled
    /// out in `creature_width_allocations.rs`.
    static THREAD_BYTES: Cell<usize> = const { Cell::new(0) };
}

fn bill(bytes: usize) {
    let _ = THREAD_BYTES.try_with(|counter| counter.set(counter.get().saturating_add(bytes)));
}

/// Forwards to the system allocator while totalling the bytes it hands out.
/// It does not forward `realloc`, so a growing map is billed the same way on
/// every platform.
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

/// A declared width far above [`MAX_NODE_COUNT`]. It is still small enough for
/// a walk over it to finish, so a regression fails the test instead of
/// exhausting the runner.
const OVERSIZED_INPUT: usize = 1_000_000;
const QUADRUPLED_INPUT: usize = 4 * OVERSIZED_INPUT;

/// A conservative floor on what one walked input costs: a `String` handle
/// (24 B) plus its `usize` position (8 B). The name's own heap and the map's
/// headroom are left out.
const BYTES_PER_WALKED_INPUT: usize = 32;

/// The budget for one call: less than walking the widest creature the index
/// space allows.
const WIDTH_BUDGET_BYTES: usize = MAX_NODE_COUNT * BYTES_PER_WALKED_INPUT;

/// How far one call's cost may move when the declaration is quadrupled. A walk
/// would move by tens of megabytes. A few kilobytes covers allocator noise.
const GROWTH_SLACK_BYTES: usize = 4 * 1024;

fn neuron(kind: &str, uuid: &str) -> NeuronExport {
    NeuronExport {
        id: None,
        neuron_type: kind.to_string(),
        uuid: uuid.to_string(),
        bias: 0.0,
        squash: Some("IDENTITY".to_string()),
    }
}

fn edge(from: &str, to: &str) -> SynapseExport {
    SynapseExport {
        from_uuid: from.to_string(),
        to_uuid: to.to_string(),
        weight: 1.0,
        synapse_type: None,
    }
}

/// Two parallel IDENTITY bridges from `input-0` and `input-1`, which merge into
/// one. Only the declared width varies.
fn bridges_declaring(input: usize) -> CreatureExport {
    CreatureExport {
        input,
        output: 1,
        neurons: vec![
            neuron("hidden", "h0"),
            neuron("hidden", "h1"),
            neuron("output", "output-0"),
        ],
        synapses: vec![
            edge("input-0", "h0"),
            edge("h0", "output-0"),
            edge("input-1", "h1"),
            edge("h1", "output-0"),
        ],
        semantic_version: None,
        forward_only: true,
        memetic: None,
    }
}

/// Bytes this thread allocated while one merge ran at `input`. The creature is
/// built before the counter is zeroed, so only the merge is billed.
fn cost_of_merging(input: usize) -> usize {
    let creature = bridges_declaring(input);

    THREAD_BYTES.with(|counter| counter.set(0));
    let merged = merge_parallel_bridges(&creature);
    let spent = THREAD_BYTES.with(Cell::get);

    let merged = merged.expect("finite weights merge");
    assert_eq!(
        merged.removed_neurons, 1,
        "the pair must merge at a declared input of {input}"
    );
    spent
}

#[test]
fn a_merge_costs_the_same_whatever_input_width_is_declared() {
    let at_width = cost_of_merging(OVERSIZED_INPUT);
    let at_quadruple = cost_of_merging(QUADRUPLED_INPUT);
    let growth = at_quadruple.saturating_sub(at_width);

    assert!(
        at_width <= WIDTH_BUDGET_BYTES,
        "merge_parallel_bridges allocated {at_width} B against a declared input of \
         {OVERSIZED_INPUT} (budget {WIDTH_BUDGET_BYTES} B); the declared width is being walked"
    );
    assert!(
        growth <= GROWTH_SLACK_BYTES,
        "merge_parallel_bridges spent {growth} B more at {QUADRUPLED_INPUT} inputs than at \
         {OVERSIZED_INPUT} ({at_quadruple} B against {at_width} B, slack \
         {GROWTH_SLACK_BYTES} B); its cost follows the declared width, not the payload"
    );
}
