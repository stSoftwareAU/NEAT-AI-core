//! `detect_cycles` answers exactly what the quadratic rescan answered
//! (NEAT-AI#3832).
//!
//! The relaxation pass used to rescan the whole synapse list once per dequeued
//! neuron. On a 4 272-neuron, 22 928-synapse production creature that was
//! 10.8 ms of the 10.9 ms `creature_validate` spent, and the same cost again on
//! every `TypedTopology.detectCycles` call from the host. Replacing it with an
//! outward adjacency built once makes the walk linear in neurons plus
//! synapses.
//!
//! Speeding a validator up is only worth anything if it answers the same, so
//! this file pins the new walk against [`rescan_detect_cycles`] below — the
//! algorithm as it stood, transcribed — over topologies that include every
//! shape the two could plausibly disagree on:
//!
//! | Shape | Why it could diverge |
//! |-------|----------------------|
//! | a duplicated `(from, to)` pair | the in-degree must be decremented once per copy, not once per pair |
//! | an endpoint at or past `num_neurons` | the counting pass admits `from >= num_neurons`; the walk can never dequeue it, so its in-degree contribution must stay |
//! | a synapse into an input neuron | counted by neither pass |
//! | a synapse out of an input neuron | inputs are never queued, so it must not relax anything |
//! | a self-loop on a non-input neuron | short-circuits to "cycle" before either walk starts |
//! | an unsorted synapse list | the adjacency groups by source, so it must not depend on synapse order |

use neat_core::topology_ops::detect_cycles;

/// Kahn's algorithm as `detect_cycles` ran it before the outward adjacency —
/// rescanning every synapse for each dequeued neuron.
///
/// Kept verbatim rather than tidied: it is the oracle, so anything "obviously
/// redundant" about it is exactly what the parity assertion is protecting.
fn rescan_detect_cycles(
    from_indices: &[u32],
    to_indices: &[u32],
    num_neurons: u32,
    num_inputs: u32,
) -> u32 {
    let n = num_neurons as usize;
    let input_count = num_inputs as usize;

    if from_indices.len() != to_indices.len() {
        return 0;
    }
    if input_count > n {
        return 0;
    }

    for i in 0..from_indices.len() {
        if from_indices[i] == to_indices[i] && (from_indices[i] as usize) >= input_count {
            return 1;
        }
    }

    let mut in_degree = vec![0i32; n];
    for i in 0..from_indices.len() {
        let from = from_indices[i] as usize;
        let to = to_indices[i] as usize;
        if from == to {
            continue;
        }
        if from >= input_count && to >= input_count && to < n {
            in_degree[to] += 1;
        }
    }

    let mut queue: Vec<usize> = Vec::new();
    for i in input_count..n {
        if in_degree[i] == 0 {
            queue.push(i);
        }
    }

    let mut processed = 0usize;
    let mut head = 0;
    while head < queue.len() {
        let idx = queue[head];
        head += 1;
        processed += 1;

        for s in 0..from_indices.len() {
            if from_indices[s] as usize != idx {
                continue;
            }
            let to = to_indices[s] as usize;
            if to == idx || to < input_count || to >= n {
                continue;
            }
            in_degree[to] -= 1;
            if in_degree[to] == 0 {
                queue.push(to);
            }
        }
    }

    let non_input_count = n - input_count;
    if processed < non_input_count { 1 } else { 0 }
}

/// A reproducible pseudo-random source — the parity corpus has to be the same
/// corpus on every machine and every run, so a failure is a failure anyone can
/// reproduce from the seed alone.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        // Numerical Recipes' 64-bit LCG constants.
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    /// A value in `0..bound`, for a non-zero bound.
    fn below(&mut self, bound: u32) -> u32 {
        (self.next() >> 33) as u32 % bound
    }
}

/// One random topology: neuron count, input width and a synapse list drawn
/// from a range **wider** than the neuron count, so out-of-range endpoints and
/// input-targeting synapses turn up without being special-cased in.
fn random_topology(rng: &mut Lcg) -> (Vec<u32>, Vec<u32>, u32, u32) {
    let num_neurons = 2 + rng.below(14);
    let num_inputs = rng.below(num_neurons);
    let synapse_count = rng.below(24);
    // Two past the last neuron: enough to draw an endpoint the walk must
    // refuse to dequeue, without swamping the corpus with them.
    let endpoint_bound = num_neurons + 2;

    let mut from = Vec::with_capacity(synapse_count as usize);
    let mut to = Vec::with_capacity(synapse_count as usize);
    for _ in 0..synapse_count {
        from.push(rng.below(endpoint_bound));
        to.push(rng.below(endpoint_bound));
    }

    (from, to, num_neurons, num_inputs)
}

#[test]
fn the_linear_walk_answers_what_the_rescan_answered() {
    let mut rng = Lcg(0x3832_0000_0000_0001);

    for case in 0..20_000 {
        let (from, to, num_neurons, num_inputs) = random_topology(&mut rng);

        assert_eq!(
            detect_cycles(&from, &to, num_neurons, num_inputs),
            rescan_detect_cycles(&from, &to, num_neurons, num_inputs),
            "case {case}: from={from:?} to={to:?} num_neurons={num_neurons} num_inputs={num_inputs}"
        );
    }
}

/// The corpus above is only worth its runtime if it actually reaches both
/// answers — a generator that only ever drew acyclic topologies would assert
/// nothing.
#[test]
fn the_random_corpus_reaches_both_answers() {
    let mut rng = Lcg(0x3832_0000_0000_0001);
    let mut cyclic = 0;
    let mut acyclic = 0;

    for _ in 0..20_000 {
        let (from, to, num_neurons, num_inputs) = random_topology(&mut rng);
        match detect_cycles(&from, &to, num_neurons, num_inputs) {
            0 => acyclic += 1,
            _ => cyclic += 1,
        }
    }

    assert!(cyclic > 1_000, "corpus found only {cyclic} cyclic cases");
    assert!(acyclic > 1_000, "corpus found only {acyclic} acyclic cases");
}

/// A duplicated edge contributes twice to the in-degree, so the walk has to
/// relax it twice. Grouping by source rather than rescanning is exactly where
/// an "obvious" dedup would silently change the answer.
#[test]
fn a_duplicated_synapse_is_relaxed_once_per_copy() {
    // input 0 → hidden 1 → output 2, with 1 → 2 listed twice.
    let from = [0, 1, 1];
    let to = [1, 2, 2];

    assert_eq!(detect_cycles(&from, &to, 3, 1), 0);
    assert_eq!(rescan_detect_cycles(&from, &to, 3, 1), 0);
}

/// A source past the last neuron can never be dequeued, so its contribution to
/// the target's in-degree is never taken back — and the topology is reported
/// as cyclic even though nothing in range forms a cycle. That is what the
/// rescan did; the adjacency must not "fix" it here.
#[test]
fn a_source_past_the_last_neuron_still_holds_its_target_down() {
    let from = [9];
    let to = [2];

    assert_eq!(detect_cycles(&from, &to, 3, 1), 1);
    assert_eq!(rescan_detect_cycles(&from, &to, 3, 1), 1);
}

/// A synapse leaving an input neuron relaxes its target the same way any other
/// does — inputs are never *queued*, but they are also never counted into an
/// in-degree, so the target starts at zero and is queued from the seed pass.
#[test]
fn a_synapse_out_of_an_input_neuron_leaves_its_target_free() {
    let from = [0, 0];
    let to = [1, 2];

    assert_eq!(detect_cycles(&from, &to, 3, 1), 0);
    assert_eq!(rescan_detect_cycles(&from, &to, 3, 1), 0);
}

/// Synapse order is not part of the answer: the adjacency groups by source, so
/// a list sorted by `(from, to)` and the same list reversed must agree.
#[test]
fn the_answer_does_not_depend_on_synapse_order() {
    let from = [0, 1, 2, 3];
    let to = [1, 2, 3, 4];
    let reversed_from = [3, 2, 1, 0];
    let reversed_to = [4, 3, 2, 1];

    assert_eq!(detect_cycles(&from, &to, 5, 1), 0);
    assert_eq!(detect_cycles(&reversed_from, &reversed_to, 5, 1), 0);
}

/// The walk stays linear: a wide, deep, forward-only chain is the shape the
/// rescan was quadratic on, and it has to come back acyclic.
#[test]
fn a_large_forward_only_chain_is_acyclic() {
    let num_inputs = 64u32;
    let num_neurons = 4_096u32;

    let mut from = Vec::new();
    let mut to = Vec::new();
    for target in num_inputs..num_neurons {
        // Two feeders each: one input, one immediate predecessor.
        from.push(target % num_inputs);
        to.push(target);
        from.push(target - 1);
        to.push(target);
    }

    assert_eq!(detect_cycles(&from, &to, num_neurons, num_inputs), 0);
}
