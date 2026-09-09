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
//! declared count; a width bounded first costs nothing.
//!
//! Modelled on `tests/topology_ops_allocations.rs`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use neat_core::if_graft::validate_creature_topology;
use neat_core::network::MAX_NODE_COUNT;
use neat_core::{CreatureExport, NeuronExport, SynapseExport, compile_creature};

/// Global allocator forwarding to the system allocator while totalling the
/// bytes it hands out.
///
/// The counter is **monotonic** — `dealloc` does not decrement it — so zeroing
/// it before a measurement can never leave it out of step with memory that was
/// already live. A live-bytes counter cannot: freeing something allocated
/// before the reset underflows it, and the arithmetic then panics *inside* the
/// allocator, which deadlocks the harness rather than failing a test.
///
/// `realloc` is deliberately *not* forwarded to `System.realloc` — the default
/// `GlobalAlloc::realloc` allocates, copies and frees, which is the behaviour a
/// growing `HashMap` must be assumed to have. That makes the figure
/// deterministic across platforms instead of depending on whether the system
/// allocator happened to extend a block in place.
struct TrackingAllocator;

static TOTAL_BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        TOTAL_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        // SAFETY: forwarding an unchanged layout to the system allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        TOTAL_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
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

/// The counters are process-wide, so only one measurement runs at a time.
static MEASURING: Mutex<()> = Mutex::new(());

/// A declared width far above [`MAX_NODE_COUNT`], kept small enough that an
/// unbounded walk still finishes rather than exhausting the test runner: the
/// point is the *shape* of the cost, not how far it can be pushed.
const OVERSIZED_INPUT: usize = 1_000_000;

/// Bytes a refusal may allocate in total. The declared width contributes
/// nothing once it is bounded first; an unbounded walk of [`OVERSIZED_INPUT`]
/// costs tens of megabytes in map entries and owned `String` keys alone.
const REFUSAL_BUDGET_BYTES: usize = 64 * 1024;

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

/// Take the measurement lock and zero the counter.
fn start_measuring() -> MutexGuard<'static, ()> {
    let guard = MEASURING.lock().unwrap_or_else(|e| e.into_inner());
    TOTAL_BYTES.store(0, Ordering::Relaxed);
    guard
}

fn assert_within_budget(spent: usize, entry_point: &str) {
    assert!(
        spent <= REFUSAL_BUDGET_BYTES,
        "{entry_point} allocated {spent} B refusing a declared input of {OVERSIZED_INPUT} \
         (budget {REFUSAL_BUDGET_BYTES} B, ceiling {MAX_NODE_COUNT}); \
         the declared width is being walked before it is bounded"
    );
}

#[test]
fn compiling_refuses_an_oversized_declared_input_without_paying_for_it() {
    let creature = creature_declaring(OVERSIZED_INPUT);

    let guard = start_measuring();
    let outcome = compile_creature(&creature);
    let spent = TOTAL_BYTES.load(Ordering::Relaxed);
    drop(guard);

    assert!(
        outcome.is_err(),
        "a declared input of {OVERSIZED_INPUT} must be refused"
    );
    assert_within_budget(spent, "compile_creature");
}

#[test]
fn topology_validation_refuses_an_oversized_declared_input_without_paying_for_it() {
    let creature = creature_declaring(OVERSIZED_INPUT);

    let guard = start_measuring();
    let outcome = validate_creature_topology(&creature);
    let spent = TOTAL_BYTES.load(Ordering::Relaxed);
    drop(guard);

    assert!(
        outcome.is_err(),
        "a declared input of {OVERSIZED_INPUT} must be refused"
    );
    assert_within_budget(spent, "validate_creature_topology");
}
