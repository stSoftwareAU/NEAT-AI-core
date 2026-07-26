//! Issue #8 — Topology helpers lifted from NEAT-AI `wasm_activation/src/topology_ops.rs`.
//!
//! Pure-computation topology helpers over typed-array neuron/synapse descriptors.
//! These were originally WASM-only; migrated here so both native consumers
//! (CLI tools, scorer, discovery) and the `wasm_activation` crate share a
//! single implementation.
//!
//! Functions are exported as ordinary `pub fn` items, with
//! `#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]` on each export so the
//! same source compiles for native and WASM targets — matching the
//! pre-existing pattern used by `accumulate`.
//!
//! Upstream context:
//! - NEAT-AI #1959 — read-heavy topology operations.
//! - NEAT-AI #1960 — batch API design.
//! - NEAT-AI #1961 — structural integrity + cycle detection.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use crate::squash::SquashType;
use crate::synapse_type::SynapseType;

// ===========================================================================
// Topology validation error codes — must match TypeScript constants
// in `wasm_activation`-consuming code (`WasmTopologyOps.ts`).
// ===========================================================================

/// Topology is valid.
pub const VALID: i32 = 0;
/// Self-connection detected.
pub const SELF_CONNECTION: i32 = 1;
/// Backward connection detected (`from > to`).
pub const BACKWARD_CONNECTION: i32 = 2;
/// `from` indices not in non-decreasing order.
pub const SORT_ERROR_FROM: i32 = 3;
/// `to` indices not strictly increasing within the same `from`.
pub const SORT_ERROR_TO: i32 = 4;
/// Duplicate `(from, to)` connection.
pub const DUPLICATE_CONNECTION: i32 = 5;
/// Input buffers are malformed — length mismatch, index out of range
/// relative to `num_neurons`, or `num_neurons` itself implausibly large.
/// Issue NEAT-AI #2659 — root-cause defence in depth so consumers receive
/// a defined error code rather than a WASM `memory access out of bounds`
/// trap when an evolved creature emits a pathological edge list.
pub const MALFORMED_BUFFER: i32 = 6;

// ===========================================================================
// Structural integrity error codes.
// ===========================================================================

/// Structural integrity is valid.
pub const STRUCTURAL_VALID: i32 = 0;
/// A synapse targets an input neuron.
pub const STRUCTURAL_SYNAPSE_TARGETS_INPUT: i32 = 1;
/// A constant neuron has an inward connection.
pub const STRUCTURAL_CONSTANT_HAS_INWARD: i32 = 2;
/// A hidden neuron has no inward connection.
pub const STRUCTURAL_HIDDEN_NO_INWARD: i32 = 3;
/// A hidden neuron has no outward connection.
pub const STRUCTURAL_HIDDEN_NO_OUTWARD: i32 = 4;
/// A bias is not finite (NaN or infinite).
pub const STRUCTURAL_BIAS_NOT_FINITE: i32 = 5;
/// An IF neuron has fewer than 3 inward connections.
pub const STRUCTURAL_IF_TOO_FEW_INWARD: i32 = 6;
/// An IF neuron is missing a condition synapse.
pub const STRUCTURAL_IF_MISSING_CONDITION: i32 = 7;
/// An IF neuron is missing a positive (or standard) synapse.
pub const STRUCTURAL_IF_MISSING_POSITIVE: i32 = 8;
/// An IF neuron is missing a negative synapse.
pub const STRUCTURAL_IF_MISSING_NEGATIVE: i32 = 9;
/// Structural input buffers are malformed — length mismatch between
/// `from_indices`/`to_indices`, `num_inputs`/`num_outputs` larger than
/// `biases.len()`, or `num_inputs + num_outputs > num_neurons`. Issue
/// NEAT-AI #2659 — defined error code in place of an `unreachable` or
/// `memory access out of bounds` trap on malformed input.
pub const STRUCTURAL_MALFORMED_BUFFER: i32 = 10;

/// Squash-type code for IF neurons — resolved from [`SquashType::If`].
const IF_SQUASH: u8 = SquashType::If as u8;
/// Synapse-type codes — resolved from [`SynapseType`] discriminants.
const SYN_STANDARD: u8 = SynapseType::Standard as u8;
const SYN_CONDITION: u8 = SynapseType::Condition as u8;
const SYN_NEGATIVE: u8 = SynapseType::Negative as u8;
const SYN_POSITIVE: u8 = SynapseType::Positive as u8;

/// Validate topology synapse ordering and forward-only constraints.
///
/// Checks that synapses are sorted (ascending `from`, then ascending `to`
/// within the same `from`), contain no self-connections, and contain no
/// backward connections (`from > to`).
///
/// # Arguments
/// * `from_indices` - source neuron index per synapse
/// * `to_indices` - destination neuron index per synapse
///
/// # Returns
/// A two-element vector `[error_code, synapse_index]`.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn validate_topology(from_indices: &[u32], to_indices: &[u32]) -> Vec<i32> {
    let len = from_indices.len();
    if len != to_indices.len() {
        // Issue NEAT-AI #2659 — length mismatch now reports a dedicated
        // malformed-buffer code instead of the legacy SORT_ERROR_FROM
        // shadow code. Forward-only ordering errors are still reported as
        // SORT_ERROR_FROM below.
        return vec![MALFORMED_BUFFER, 0];
    }

    let mut last_from: i64 = -1;
    let mut last_to: i64 = -1;

    for i in 0..len {
        let from = from_indices[i] as i64;
        let to = to_indices[i] as i64;

        if from == to {
            return vec![SELF_CONNECTION, i as i32];
        }

        if from > to {
            return vec![BACKWARD_CONNECTION, i as i32];
        }

        if from < last_from {
            return vec![SORT_ERROR_FROM, i as i32];
        } else if from > last_from {
            last_to = -1;
        }

        if from == last_from {
            if to < last_to {
                return vec![SORT_ERROR_TO, i as i32];
            } else if to == last_to {
                return vec![DUPLICATE_CONNECTION, i as i32];
            }
        }

        last_from = from;
        last_to = to;
    }

    vec![VALID, 0]
}

/// First `to` index the availability scan considers for a given `from`.
///
/// Forward-only (`to > from`) and never targeting an input neuron
/// (`to >= num_inputs`), so the scan starts at whichever bound is higher.
#[inline]
fn scan_start_to(from_idx: usize, input_count: usize) -> usize {
    if from_idx + 1 > input_count {
        from_idx + 1
    } else {
        input_count
    }
}

/// Whether `to_idx` is a constant neuron the scan must skip.
///
/// Indices beyond `is_constant` are treated as non-constant, preserving the
/// original bounds-tolerant behaviour on a short `is_constant` buffer.
#[inline]
fn scan_is_constant(is_constant: &[u8], to_idx: usize) -> bool {
    to_idx < is_constant.len() && is_constant[to_idx] != 0
}

/// Scan for available forward-only connection slots.
///
/// Returns all `(from, to)` pairs where `from < to`, `to >= num_inputs`, the
/// target neuron is not constant, and no connection already exists.
///
/// Issue #387 — existence is answered from a compressed per-`from` run of
/// existing targets (an `O(n + synapses)` adjacency built once), merge-walked
/// against the candidate range. The previous implementation allocated a dense
/// `n × n` boolean matrix for the same answer: 2.78 MB zeroed per call on the
/// production topology to record ~21.5k synapses, a fill factor under 0.8%.
/// The candidate count is also computed up front so the result vector is
/// allocated exactly once at its final size instead of growing through a
/// realloc chain. Output — pairs and their order — is unchanged.
///
/// Sorted input is *not* required: each per-`from` run is sorted on build, so
/// an unsorted or duplicate-bearing edge list yields the same answer.
///
/// # Returns
/// Flattened `[from, to, from, to, ...]` pairs. Returns an empty vector for
/// malformed input — mismatched `from`/`to` lengths, an implausibly large
/// `num_neurons`, or a candidate count whose result vector could not be
/// addressed — rather than panicking (which would trap under WASM).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn scan_available_connections(
    from_indices: &[u32],
    to_indices: &[u32],
    is_constant: &[u8],
    num_neurons: u32,
    num_inputs: u32,
) -> Vec<u32> {
    let n = num_neurons as usize;
    let input_count = num_inputs as usize;

    // Issue NEAT-AI #2659 — defensive bail-out. A length mismatch or a
    // pathologically large `num_neurons` previously caused a multiplication
    // panic (and so a WASM trap) instead of a defined empty result. The
    // `n * n` plausibility bound is retained: it is no longer an allocation
    // size, but it still rejects a neuron count whose forward-pair space
    // could never be materialised.
    if from_indices.len() != to_indices.len() {
        return Vec::new();
    }
    if n.checked_mul(n).is_none_or(|v| v > isize::MAX as usize) {
        return Vec::new();
    }
    if n == 0 {
        return Vec::new();
    }

    // -----------------------------------------------------------------
    // Compressed adjacency: for each `from`, the ascending run of existing
    // targets that could ever block a candidate. Only pairs the scan can
    // actually emit are retained (`to > from`, `to >= input_count`,
    // both endpoints in range), which drops backward, self and
    // out-of-range synapses exactly as the old matrix lookup did.
    // -----------------------------------------------------------------
    let mut starts = vec![0usize; n + 1];
    for i in 0..from_indices.len() {
        let from = from_indices[i] as usize;
        let to = to_indices[i] as usize;
        if from < n && to < n && to > from && to >= input_count {
            starts[from + 1] += 1;
        }
    }
    for f in 0..n {
        starts[f + 1] += starts[f];
    }
    let mut cursor = starts.clone();
    let mut targets = vec![0u32; starts[n]];
    for i in 0..from_indices.len() {
        let from = from_indices[i] as usize;
        let to = to_indices[i] as usize;
        if from < n && to < n && to > from && to >= input_count {
            targets[cursor[from]] = to as u32;
            cursor[from] += 1;
        }
    }
    // Each run is already ascending for a `validate_topology`-clean edge
    // list, so this is an O(run) insertion-sort pass in the common case; it
    // is what lets an unsorted list fall through to the same answer.
    for f in 0..n {
        targets[starts[f]..starts[f + 1]].sort_unstable();
    }

    // Prefix sums of constant neurons, so "how many constants in
    // [start_to, n)?" is O(1) per `from`.
    let mut const_prefix = vec![0u32; n + 1];
    for j in 0..n {
        const_prefix[j + 1] = const_prefix[j] + u32::from(scan_is_constant(is_constant, j));
    }

    // -----------------------------------------------------------------
    // Exact candidate count — O(n + synapses), no O(n^2) pre-pass. For each
    // `from` the candidate range is [start_to, n); subtract the constants in
    // that range and the distinct existing (non-constant) targets in it.
    // The two subtracted sets are disjoint, so the result never underflows.
    // -----------------------------------------------------------------
    let mut total_pairs: usize = 0;
    for from_idx in 0..n {
        let start_to = scan_start_to(from_idx, input_count);
        if start_to >= n {
            continue;
        }
        let span = n - start_to;
        let constants = (const_prefix[n] - const_prefix[start_to]) as usize;
        let mut blocked = 0usize;
        let mut previous: Option<u32> = None;
        for k in starts[from_idx]..starts[from_idx + 1] {
            let target = targets[k];
            // A duplicate edge blocks the same slot once.
            if previous == Some(target) {
                continue;
            }
            previous = Some(target);
            if !scan_is_constant(is_constant, target as usize) {
                blocked += 1;
            }
        }
        total_pairs += span - constants - blocked;
    }

    // Flattened pairs: two `u32` per candidate. Refuse rather than abort if
    // the exact result could not be addressed.
    let capacity = match total_pairs.checked_mul(2) {
        Some(c)
            if c.checked_mul(size_of::<u32>())
                .is_some_and(|b| b <= isize::MAX as usize) =>
        {
            c
        }
        _ => return Vec::new(),
    };

    let mut available: Vec<u32> = Vec::with_capacity(capacity);

    for from_idx in 0..n {
        let start_to = scan_start_to(from_idx, input_count);
        if start_to >= n {
            continue;
        }
        // Merge-walk: `to_idx` ascends, so the run cursor only moves forward.
        let mut p = starts[from_idx];
        let end = starts[from_idx + 1];
        for to_idx in start_to..n {
            if scan_is_constant(is_constant, to_idx) {
                continue;
            }
            while p < end && (targets[p] as usize) < to_idx {
                p += 1;
            }
            if p < end && targets[p] as usize == to_idx {
                continue;
            }
            available.push(from_idx as u32);
            available.push(to_idx as u32);
        }
    }

    debug_assert_eq!(
        available.len(),
        capacity,
        "pre-sized capacity must match the emitted pair count"
    );

    available
}

/// Compute reverse topological order for backpropagation.
///
/// Uses Kahn's algorithm on the forward connection graph. Returns neuron
/// indices ordered with output neurons first, then hidden neurons after
/// their downstream consumers. Input neurons are excluded. Neurons remaining
/// in cycles are appended at the end.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn compute_reverse_topological_order(
    from_indices: &[u32],
    to_indices: &[u32],
    num_neurons: u32,
    num_inputs: u32,
) -> Vec<u32> {
    let n = num_neurons as usize;
    let input_count = num_inputs as usize;

    // Issue NEAT-AI #2659 — bail out on malformed inputs rather than
    // letting the indexing operations below trap. Callers receive an
    // empty result and can recover (e.g. drop the offending creature)
    // without the WASM run aborting.
    if from_indices.len() != to_indices.len() {
        return Vec::new();
    }
    if input_count > n {
        return Vec::new();
    }

    let mut out_degree = vec![0i32; n];

    // -----------------------------------------------------------------
    // Issue #388 — inward adjacency as CSR (compressed sparse row) rather
    // than one `Vec<u32>` per neuron. The old shape cost n + 1 heap
    // allocations per call plus the geometric regrowth of every inner
    // `Vec`, and scattered Kahn's walk across n unrelated allocations. The
    // CSR form is three allocations total and the walk reads one contiguous
    // run per neuron — the same layout `PropagateInput` already uses for
    // its inward lists.
    //
    // A synapse is retained here exactly when the old code would have
    // pushed it: not a self-loop, and both endpoints inside `n`. Filtering
    // identically in both passes is what keeps the emitted order
    // element-identical to the pre-#388 implementation.
    // -----------------------------------------------------------------

    // Pass 1 — inward degree per neuron, accumulated one slot to the right
    // so the prefix sum below turns it straight into run starts.
    let mut inward_starts = vec![0usize; n + 1];
    for i in 0..from_indices.len() {
        let from = from_indices[i] as usize;
        let to = to_indices[i] as usize;

        // Defensive: skip synapses whose endpoints fall outside the
        // declared neuron count rather than panicking. Production has
        // observed pathological evolved creatures emitting stale indices
        // after a neuron rename; #2659.
        if from == to || from >= n || to >= n {
            continue;
        }
        inward_starts[to + 1] += 1;
    }
    for t in 0..n {
        inward_starts[t + 1] += inward_starts[t];
    }

    // Pass 2 — fill the flat index array with a moving per-neuron cursor.
    // `out_degree` is accumulated here too, on the same surviving synapses.
    let mut cursor = inward_starts.clone();
    let mut inward_indices = vec![0u32; inward_starts[n]];
    for i in 0..from_indices.len() {
        let from = from_indices[i] as usize;
        let to = to_indices[i] as usize;

        if from == to || from >= n || to >= n {
            continue;
        }
        if from >= input_count {
            out_degree[from] += 1;
        }
        inward_indices[cursor[to]] = from as u32;
        cursor[to] += 1;
    }

    // Ready queue and result hold at most one entry per non-input neuron.
    // The queue can exceed that only on a duplicate-edge topology, where
    // the capacity is a hint rather than a bound.
    let non_input_count = n - input_count;
    let mut queue: Vec<usize> = Vec::with_capacity(non_input_count);
    for i in input_count..n {
        if out_degree[i] == 0 {
            queue.push(i);
        }
    }

    let mut result: Vec<u32> = Vec::with_capacity(non_input_count);
    // Indexed by absolute neuron index, so this stays `n` wide.
    let mut visited = vec![false; n];
    let mut head = 0;

    while head < queue.len() {
        let idx = queue[head];
        head += 1;

        if visited[idx] {
            continue;
        }
        visited[idx] = true;
        result.push(idx as u32);

        // Self-loops were dropped when the adjacency was built, so no
        // `from == idx` check is needed inside this run.
        for k in inward_starts[idx]..inward_starts[idx + 1] {
            let from = inward_indices[k] as usize;
            if from < input_count {
                continue;
            }
            if visited[from] {
                continue;
            }

            out_degree[from] -= 1;
            if out_degree[from] <= 0 {
                queue.push(from);
            }
        }
    }

    for i in input_count..n {
        if !visited[i] {
            result.push(i as u32);
        }
    }

    result
}

/// Batch topology validation for multiple creatures.
///
/// Validates multiple topologies in a single call to amortise WASM boundary
/// crossing. Each topology's `from`/`to` indices are concatenated; the
/// `lengths` array specifies per-topology synapse counts.
///
/// # Returns
/// `[error_code_0, synapse_index_0, error_code_1, synapse_index_1, ...]`.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn validate_topology_batch(
    all_from_indices: &[u32],
    all_to_indices: &[u32],
    lengths: &[u32],
) -> Vec<i32> {
    let num_topologies = lengths.len();
    let mut result = vec![0i32; num_topologies * 2];
    let mut offset: usize = 0;

    for t in 0..num_topologies {
        let len = lengths[t] as usize;
        let end = offset + len;

        if end > all_from_indices.len() || end > all_to_indices.len() {
            result[t * 2] = SORT_ERROR_FROM;
            result[t * 2 + 1] = 0;
            offset = end;
            continue;
        }

        let from_slice = &all_from_indices[offset..end];
        let to_slice = &all_to_indices[offset..end];
        let single_result = validate_topology(from_slice, to_slice);

        result[t * 2] = single_result[0];
        result[t * 2 + 1] = single_result[1];

        offset = end;
    }

    result
}

/// Validate structural integrity of a typed topology.
///
/// Checks:
/// - No synapse targets an input neuron.
/// - Constant neurons have no inward connections.
/// - Hidden neurons have at least 1 inward and 1 outward connection.
/// - Non-input neuron biases are finite.
/// - IF neurons have at least 3 inward connections with condition,
///   positive (or standard), and negative synapse types.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn validate_structural_integrity(
    from_indices: &[u32],
    to_indices: &[u32],
    is_constant: &[u8],
    squash_types: &[u8],
    biases: &[f64],
    num_inputs: u32,
    num_outputs: u32,
    synapse_types: &[u8],
) -> Vec<i32> {
    let num_neurons = biases.len();
    let num_synapses = from_indices.len();
    let input_count = num_inputs as usize;
    let output_count = num_outputs as usize;

    // Issue NEAT-AI #2659 — refuse pathological inputs with a defined
    // error code so callers do not see a WASM trap (`memory access out
    // of bounds` from `output_start = num_neurons - output_count`
    // underflow, or out-of-range writes into `inward_count`).
    if to_indices.len() != num_synapses {
        return vec![STRUCTURAL_MALFORMED_BUFFER, 0];
    }
    if input_count > num_neurons || output_count > num_neurons {
        return vec![STRUCTURAL_MALFORMED_BUFFER, 0];
    }
    if input_count.saturating_add(output_count) > num_neurons {
        return vec![STRUCTURAL_MALFORMED_BUFFER, 0];
    }

    for i in 0..num_synapses {
        if (to_indices[i] as usize) < input_count {
            return vec![STRUCTURAL_SYNAPSE_TARGETS_INPUT, to_indices[i] as i32];
        }
    }

    let mut inward_count = vec![0u32; num_neurons];
    let mut outward_count = vec![0u32; num_neurons];

    for i in 0..num_synapses {
        let from = from_indices[i] as usize;
        let to = to_indices[i] as usize;
        if from < num_neurons {
            outward_count[from] += 1;
        }
        if to < num_neurons {
            inward_count[to] += 1;
        }
    }

    let output_start = num_neurons - output_count;

    for i in input_count..num_neurons {
        let is_output = i >= output_start;
        let is_const = i < is_constant.len() && is_constant[i] != 0;

        if !is_const {
            let bias = biases[i];
            if bias.is_nan() || bias.is_infinite() {
                return vec![STRUCTURAL_BIAS_NOT_FINITE, i as i32];
            }
        }

        if is_const {
            if inward_count[i] > 0 {
                return vec![STRUCTURAL_CONSTANT_HAS_INWARD, i as i32];
            }
            continue;
        }

        if !is_output {
            if inward_count[i] == 0 {
                return vec![STRUCTURAL_HIDDEN_NO_INWARD, i as i32];
            }
            if outward_count[i] == 0 {
                return vec![STRUCTURAL_HIDDEN_NO_OUTWARD, i as i32];
            }
        }

        if i < squash_types.len() && squash_types[i] == IF_SQUASH {
            if inward_count[i] < 3 {
                return vec![STRUCTURAL_IF_TOO_FEW_INWARD, i as i32];
            }

            let mut has_condition = false;
            let mut has_positive = false;
            let mut has_negative = false;

            for s in 0..num_synapses {
                if to_indices[s] as usize != i {
                    continue;
                }
                if s < synapse_types.len() {
                    let st = synapse_types[s];
                    if st == SYN_CONDITION {
                        has_condition = true;
                    }
                    if st == SYN_POSITIVE || st == SYN_STANDARD {
                        has_positive = true;
                    }
                    if st == SYN_NEGATIVE {
                        has_negative = true;
                    }
                }
            }

            if !has_condition {
                return vec![STRUCTURAL_IF_MISSING_CONDITION, i as i32];
            }
            if !has_positive {
                return vec![STRUCTURAL_IF_MISSING_POSITIVE, i as i32];
            }
            if !has_negative {
                return vec![STRUCTURAL_IF_MISSING_NEGATIVE, i as i32];
            }
        }
    }

    vec![STRUCTURAL_VALID, 0]
}

/// Detect whether the topology contains cycles among non-input neurons.
///
/// Uses Kahn's algorithm on non-input neurons. Self-loops are explicitly
/// detected as cycles.
///
/// # Returns
/// `0` if acyclic, `1` if a cycle is detected.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn detect_cycles(
    from_indices: &[u32],
    to_indices: &[u32],
    num_neurons: u32,
    num_inputs: u32,
) -> u32 {
    let n = num_neurons as usize;
    let input_count = num_inputs as usize;

    // Issue NEAT-AI #2659 — refuse malformed buffers with a safe
    // "no cycle" result. Length mismatch or `input_count > n` previously
    // panicked while iterating; an empty/safe answer lets the caller
    // recover instead of aborting the WASM run.
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

    // Only count edges from non-input neurons; inputs cannot be part of a cycle.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn if_squash_matches_squash_type() {
        // Guard against drift between the topology_ops IF marker and SquashType::If.
        assert_eq!(IF_SQUASH, SquashType::If as u8);
        assert_eq!(IF_SQUASH, 34);
    }

    #[test]
    fn synapse_type_constants_match_enum() {
        assert_eq!(SYN_STANDARD, SynapseType::Standard as u8);
        assert_eq!(SYN_CONDITION, SynapseType::Condition as u8);
        assert_eq!(SYN_NEGATIVE, SynapseType::Negative as u8);
        assert_eq!(SYN_POSITIVE, SynapseType::Positive as u8);
    }

    // -----------------------------------------------------------------------
    // validate_topology (Issue #1959)
    // -----------------------------------------------------------------------

    #[test]
    fn validate_valid_topology() {
        let from = [0, 1, 2];
        let to = [2, 2, 3];
        let result = validate_topology(&from, &to);
        assert_eq!(result[0], VALID);
    }

    #[test]
    fn validate_self_connection() {
        let from = [0, 2, 2];
        let to = [2, 2, 3];
        let result = validate_topology(&from, &to);
        assert_eq!(result[0], SELF_CONNECTION);
        assert_eq!(result[1], 1);
    }

    #[test]
    fn validate_backward_connection() {
        let from = [0, 3];
        let to = [2, 1];
        let result = validate_topology(&from, &to);
        assert_eq!(result[0], BACKWARD_CONNECTION);
        assert_eq!(result[1], 1);
    }

    #[test]
    fn validate_sort_error_from() {
        let from = [0, 2, 1];
        let to = [2, 3, 3];
        let result = validate_topology(&from, &to);
        assert_eq!(result[0], SORT_ERROR_FROM);
        assert_eq!(result[1], 2);
    }

    #[test]
    fn validate_sort_error_to() {
        let from = [0, 0];
        let to = [3, 2];
        let result = validate_topology(&from, &to);
        assert_eq!(result[0], SORT_ERROR_TO);
        assert_eq!(result[1], 1);
    }

    #[test]
    fn validate_duplicate() {
        let from = [0, 0];
        let to = [2, 2];
        let result = validate_topology(&from, &to);
        assert_eq!(result[0], DUPLICATE_CONNECTION);
        assert_eq!(result[1], 1);
    }

    #[test]
    fn validate_empty() {
        let from: [u32; 0] = [];
        let to: [u32; 0] = [];
        let result = validate_topology(&from, &to);
        assert_eq!(result[0], VALID);
    }

    #[test]
    fn validate_mismatched_lengths_reports_malformed_buffer() {
        // Issue NEAT-AI #2659 — length mismatch reports the dedicated
        // MALFORMED_BUFFER code (was SORT_ERROR_FROM before #2659).
        let from = [0u32, 1];
        let to = [2u32];
        let result = validate_topology(&from, &to);
        assert_eq!(result[0], MALFORMED_BUFFER);
    }

    // -----------------------------------------------------------------------
    // Malformed-buffer hardening — Issue NEAT-AI #2659.
    //
    // Each test feeds an intentionally pathological edge list and asserts
    // that the function returns a defined value rather than panicking
    // (which would surface as a WASM `memory access out of bounds` trap).
    // -----------------------------------------------------------------------

    #[test]
    fn reverse_topological_order_oob_from_does_not_panic() {
        // `from = 99` is beyond `num_neurons = 4`. Before #2659 this
        // panicked while incrementing `out_degree[99]`; after #2659 the
        // synapse is skipped and the function returns a defined result.
        let from = [0u32, 1, 99];
        let to = [2u32, 2, 3];
        let result = compute_reverse_topological_order(&from, &to, 4, 2);
        // The valid synapses (0->2, 1->2) still yield a topological order
        // covering the two non-input neurons.
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn reverse_topological_order_oob_to_does_not_panic() {
        // `to = 99` is beyond `num_neurons = 4`. Before #2659 the
        // `inward[99].push(...)` call panicked; after #2659 the synapse
        // is silently dropped.
        let from = [0u32, 1];
        let to = [2u32, 99];
        let result = compute_reverse_topological_order(&from, &to, 4, 2);
        // Returns successfully without trap; the surviving 0->2 edge
        // leaves neuron 2 (and the orphan 3) covered by the Kahn pass.
        assert!(result.len() <= 2);
    }

    #[test]
    fn reverse_topological_order_mismatched_lengths_returns_empty() {
        let from = [0u32, 1, 2];
        let to = [2u32, 2];
        let result = compute_reverse_topological_order(&from, &to, 4, 2);
        assert!(result.is_empty());
    }

    #[test]
    fn reverse_topological_order_input_count_exceeds_neurons() {
        let from = [0u32];
        let to = [1u32];
        // num_inputs > num_neurons would underflow the start of the
        // ready queue range. Defended.
        let result = compute_reverse_topological_order(&from, &to, 2, 99);
        assert!(result.is_empty());
    }

    #[test]
    fn scan_available_connections_mismatched_lengths_returns_empty() {
        let from = [0u32, 1];
        let to = [2u32];
        let is_const = [0u8, 0, 0, 0];
        let result = scan_available_connections(&from, &to, &is_const, 4, 2);
        assert!(result.is_empty());
    }

    #[test]
    fn scan_available_connections_huge_neuron_count_returns_empty() {
        // u32::MAX neurons would request `n * n` allocation, which
        // overflows usize on 32-bit WASM and panics. Defended via
        // `checked_mul`.
        let from: [u32; 0] = [];
        let to: [u32; 0] = [];
        let is_const: [u8; 0] = [];
        let result = scan_available_connections(&from, &to, &is_const, u32::MAX, 0);
        assert!(result.is_empty());
    }

    #[test]
    fn structural_mismatched_lengths_reports_malformed_buffer() {
        let from = [0u32, 1];
        let to = [2u32];
        let is_const = [0u8, 0, 0, 0];
        let squash = [0u8, 0, 1, 7];
        let biases = [0.0f64, 0.0, 0.5, -0.3];
        let syn_types = [0u8, 0];

        let result = validate_structural_integrity(
            &from, &to, &is_const, &squash, &biases, 2, 1, &syn_types,
        );
        assert_eq!(result[0], STRUCTURAL_MALFORMED_BUFFER);
    }

    #[test]
    fn structural_output_count_exceeds_neurons_reports_malformed_buffer() {
        // Before #2659, `output_start = num_neurons - output_count`
        // underflowed and the next loop trapped.
        let from = [0u32, 1];
        let to = [2u32, 3];
        let is_const = [0u8, 0, 0, 0];
        let squash = [0u8, 0, 1, 7];
        let biases = [0.0f64, 0.0, 0.5, -0.3];
        let syn_types = [0u8, 0];

        let result = validate_structural_integrity(
            &from, &to, &is_const, &squash, &biases, 2, 99, &syn_types,
        );
        assert_eq!(result[0], STRUCTURAL_MALFORMED_BUFFER);
    }

    #[test]
    fn structural_input_plus_output_exceeds_neurons_reports_malformed_buffer() {
        let from = [0u32, 1];
        let to = [2u32, 3];
        let is_const = [0u8, 0, 0, 0];
        let squash = [0u8, 0, 1, 7];
        let biases = [0.0f64, 0.0, 0.5, -0.3];
        let syn_types = [0u8, 0];

        // 3 inputs + 2 outputs = 5 > 4 neurons.
        let result = validate_structural_integrity(
            &from, &to, &is_const, &squash, &biases, 3, 2, &syn_types,
        );
        assert_eq!(result[0], STRUCTURAL_MALFORMED_BUFFER);
    }

    #[test]
    fn detect_cycles_mismatched_lengths_returns_no_cycle() {
        let from = [0u32, 1];
        let to = [2u32];
        // Defended — no panic, returns "no cycle" so caller can keep
        // running.
        assert_eq!(detect_cycles(&from, &to, 4, 2), 0);
    }

    #[test]
    fn detect_cycles_input_count_exceeds_neurons_returns_no_cycle() {
        let from = [0u32];
        let to = [1u32];
        assert_eq!(detect_cycles(&from, &to, 2, 99), 0);
    }

    // -----------------------------------------------------------------------
    // scan_available_connections (Issue #1959)
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Differential coverage for the Issue #387 rewrite.
    //
    // `reference_scan_available_connections` is the pre-#387 dense `n × n`
    // boolean-matrix implementation, kept verbatim as the oracle. The
    // optimised implementation must return a byte-identical `Vec<u32>` —
    // same pairs, same order — for every topology below.
    // -----------------------------------------------------------------------

    /// Pre-#387 implementation: dense `n × n` boolean matrix, O(n^2) scan.
    /// Retained only as the differential-test oracle.
    fn reference_scan_available_connections(
        from_indices: &[u32],
        to_indices: &[u32],
        is_constant: &[u8],
        num_neurons: u32,
        num_inputs: u32,
    ) -> Vec<u32> {
        let n = num_neurons as usize;
        let input_count = num_inputs as usize;

        if from_indices.len() != to_indices.len() {
            return Vec::new();
        }
        let conn_set_len = match n.checked_mul(n) {
            Some(v) if v <= isize::MAX as usize => v,
            _ => return Vec::new(),
        };

        let mut conn_set = vec![false; conn_set_len];
        for i in 0..from_indices.len() {
            let from = from_indices[i] as usize;
            let to = to_indices[i] as usize;
            if from < n && to < n {
                conn_set[from * n + to] = true;
            }
        }

        let mut available = Vec::new();

        for from_idx in 0..n {
            let start_to = if from_idx + 1 > input_count {
                from_idx + 1
            } else {
                input_count
            };
            for to_idx in start_to..n {
                if to_idx < is_constant.len() && is_constant[to_idx] != 0 {
                    continue;
                }
                if !conn_set[from_idx * n + to_idx] {
                    available.push(from_idx as u32);
                    available.push(to_idx as u32);
                }
            }
        }

        available
    }

    /// SplitMix64-style deterministic PRNG so the randomised cases are
    /// reproducible across runs and platforms.
    struct TestRng(u64);

    impl TestRng {
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        fn below(&mut self, bound: usize) -> usize {
            if bound == 0 {
                0
            } else {
                (self.next_u64() % bound as u64) as usize
            }
        }
    }

    /// A randomised topology under test.
    struct RandomTopology {
        from: Vec<u32>,
        to: Vec<u32>,
        is_constant: Vec<u8>,
        num_neurons: u32,
        num_inputs: u32,
    }

    /// Build a random topology. `density` is the percentage of *all* forward
    /// `(from, to)` pairs that carry a synapse — 0 gives an empty synapse
    /// list, 100 gives a fully-connected forward graph. `duplicate_rate` is
    /// the percentage of emitted edges repeated a second time. `sorted`
    /// controls whether the emitted edge list is in `validate_topology` order
    /// or deliberately shuffled (the defensive path).
    fn random_topology(
        rng: &mut TestRng,
        num_neurons: u32,
        num_inputs: u32,
        density: u32,
        duplicate_rate: u32,
        constant_rate: u32,
        sorted: bool,
    ) -> RandomTopology {
        let n = num_neurons as usize;
        let mut from = Vec::new();
        let mut to = Vec::new();
        for f in 0..n {
            for t in (f + 1)..n {
                if (rng.below(100) as u32) >= density {
                    continue;
                }
                from.push(f as u32);
                to.push(t as u32);
                // Duplicates stay adjacent, so a sorted list remains sorted.
                if (rng.below(100) as u32) < duplicate_rate {
                    from.push(f as u32);
                    to.push(t as u32);
                }
            }
        }

        if !sorted {
            // Fisher–Yates over the pair list, keeping `from`/`to` aligned.
            for i in (1..from.len()).rev() {
                let j = rng.below(i + 1);
                from.swap(i, j);
                to.swap(i, j);
            }
        }

        let is_constant = (0..n)
            .map(|_| u8::from((rng.below(100) as u32) < constant_rate))
            .collect();

        RandomTopology {
            from,
            to,
            is_constant,
            num_neurons,
            num_inputs,
        }
    }

    fn assert_matches_reference(case: &str, topology: &RandomTopology) {
        let expected = reference_scan_available_connections(
            &topology.from,
            &topology.to,
            &topology.is_constant,
            topology.num_neurons,
            topology.num_inputs,
        );
        let actual = scan_available_connections(
            &topology.from,
            &topology.to,
            &topology.is_constant,
            topology.num_neurons,
            topology.num_inputs,
        );
        assert_eq!(actual, expected, "divergence from reference for {case}");
    }

    #[test]
    fn scan_available_connections_matches_reference_on_random_topologies() {
        let mut rng = TestRng(0x5EED_1234_ABCD_0001);

        for num_neurons in [1u32, 2, 3, 5, 9, 16, 31] {
            for num_inputs in [0u32, 1, num_neurons / 2, num_neurons] {
                if num_inputs > num_neurons {
                    continue;
                }
                // density 0 = empty synapse list, 100 = fully connected.
                for density in [0u32, 15, 60, 100] {
                    for duplicate_rate in [0u32, 30] {
                        for constant_rate in [0u32, 25, 100] {
                            for sorted in [true, false] {
                                let topology = random_topology(
                                    &mut rng,
                                    num_neurons,
                                    num_inputs,
                                    density,
                                    duplicate_rate,
                                    constant_rate,
                                    sorted,
                                );
                                let case = format!(
                                    "n={num_neurons} inputs={num_inputs} density={density} \
                                     dup={duplicate_rate} const={constant_rate} sorted={sorted}"
                                );
                                assert_matches_reference(&case, &topology);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn scan_available_connections_matches_reference_with_out_of_range_and_duplicate_edges() {
        // Duplicates, self-connections, backward edges and out-of-range
        // endpoints must all be tolerated identically to the reference.
        let topology = RandomTopology {
            from: vec![0, 0, 1, 2, 2, 3, 4, 99, 2],
            to: vec![3, 3, 3, 2, 4, 1, 4, 4, 500],
            is_constant: vec![0, 1, 0, 0, 0],
            num_neurons: 5,
            num_inputs: 2,
        };
        assert_matches_reference("hostile edge list", &topology);
    }

    #[test]
    fn scan_available_connections_matches_reference_with_short_is_constant_buffer() {
        // `is_constant` shorter than `num_neurons`: indices past the end are
        // treated as non-constant by both implementations.
        let topology = RandomTopology {
            from: vec![0, 1],
            to: vec![2, 3],
            is_constant: vec![0, 1],
            num_neurons: 6,
            num_inputs: 2,
        };
        assert_matches_reference("short is_constant", &topology);
    }

    #[test]
    fn scan_available_connections_num_inputs_equals_num_neurons_is_empty() {
        // Every neuron is an input, so no candidate `to` exists.
        let from: [u32; 0] = [];
        let to: [u32; 0] = [];
        let is_const = [0u8; 4];
        let result = scan_available_connections(&from, &to, &is_const, 4, 4);
        assert!(result.is_empty());
    }

    #[test]
    fn scan_available_connections_unsorted_edges_match_sorted_equivalent() {
        // The rewrite must not depend on the caller having sorted the edge
        // list: the same edges in a different order give the same answer.
        let is_const = [0u8; 6];
        let sorted =
            scan_available_connections(&[0u32, 0, 1, 2], &[3u32, 4, 3, 5], &is_const, 6, 2);
        let shuffled =
            scan_available_connections(&[2u32, 0, 1, 0], &[5u32, 4, 3, 3], &is_const, 6, 2);
        assert_eq!(sorted, shuffled);
        assert!(!sorted.is_empty());
    }

    #[test]
    fn scan_available_simple() {
        // 4 neurons: 2 inputs (0,1), 1 hidden (2), 1 output (3)
        // Existing: 0->2, 1->2, 2->3.
        let from = [0, 1, 2];
        let to = [2, 2, 3];
        let is_const = [0, 0, 0, 0];
        let result = scan_available_connections(&from, &to, &is_const, 4, 2);
        assert!(result.len() % 2 == 0);
        let pairs: Vec<(u32, u32)> = result.chunks(2).map(|c| (c[0], c[1])).collect();
        assert!(pairs.contains(&(0, 3)));
        assert!(pairs.contains(&(1, 3)));
    }

    #[test]
    fn scan_skips_constant() {
        let from = [1u32];
        let to = [2u32];
        let is_const = [0, 1, 0];
        let result = scan_available_connections(&from, &to, &is_const, 3, 1);
        let pairs: Vec<(u32, u32)> = result.chunks(2).map(|c| (c[0], c[1])).collect();
        for (_, to_idx) in &pairs {
            assert_ne!(*to_idx, 1);
        }
    }

    // -----------------------------------------------------------------------
    // compute_reverse_topological_order (Issue #1959)
    // -----------------------------------------------------------------------

    #[test]
    fn reverse_topological_order_simple() {
        let from = [0, 1, 2];
        let to = [2, 2, 3];
        let result = compute_reverse_topological_order(&from, &to, 4, 2);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], 3);
        assert_eq!(result[1], 2);
    }

    #[test]
    fn reverse_topological_order_larger() {
        // 8 neurons: 3 inputs (0-2), 3 hidden (3-5), 2 outputs (6-7).
        let from = [0, 1, 2, 3, 3, 4, 5];
        let to = [3, 4, 5, 4, 6, 6, 7];
        let result = compute_reverse_topological_order(&from, &to, 8, 3);
        assert_eq!(result.len(), 5);

        let pos_of = |idx: u32| result.iter().position(|&x| x == idx).unwrap();
        assert!(pos_of(6) < pos_of(4));
        assert!(pos_of(6) < pos_of(3));
        assert!(pos_of(7) < pos_of(5));
    }

    // -----------------------------------------------------------------------
    // validate_topology_batch (Issue #1960)
    // -----------------------------------------------------------------------

    #[test]
    fn validate_topology_batch_multiple_valid() {
        let all_from = [0, 1, 2, 0, 2];
        let all_to = [2, 2, 3, 2, 3];
        let lengths = [3, 2];

        let result = validate_topology_batch(&all_from, &all_to, &lengths);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], VALID);
        assert_eq!(result[2], VALID);
    }

    #[test]
    fn validate_topology_batch_mixed_valid_invalid() {
        let all_from = [0, 1, 2, 3];
        let all_to = [2, 2, 3, 1];
        let lengths = [3, 1];

        let result = validate_topology_batch(&all_from, &all_to, &lengths);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], VALID);
        assert_eq!(result[2], BACKWARD_CONNECTION);
    }

    #[test]
    fn validate_topology_batch_empty() {
        let all_from: [u32; 0] = [];
        let all_to: [u32; 0] = [];
        let lengths: [u32; 0] = [];

        let result = validate_topology_batch(&all_from, &all_to, &lengths);
        assert_eq!(result.len(), 0);
    }

    // -----------------------------------------------------------------------
    // validate_structural_integrity (Issue #1961)
    // -----------------------------------------------------------------------

    #[test]
    fn structural_valid() {
        let from = [0u32, 1, 2];
        let to = [2u32, 2, 3];
        let is_const = [0u8, 0, 0, 0];
        let squash = [0u8, 0, 1, 7];
        let biases = [0.0f64, 0.0, 0.5, -0.3];
        let syn_types = [0u8, 0, 0];

        let result = validate_structural_integrity(
            &from, &to, &is_const, &squash, &biases, 2, 1, &syn_types,
        );
        assert_eq!(result[0], STRUCTURAL_VALID);
    }

    #[test]
    fn structural_synapse_targets_input() {
        let from = [0u32, 2];
        let to = [1u32, 3];
        let is_const = [0u8, 0, 0, 0];
        let squash = [0u8, 0, 1, 7];
        let biases = [0.0f64, 0.0, 0.5, -0.3];
        let syn_types = [0u8, 0];

        let result = validate_structural_integrity(
            &from, &to, &is_const, &squash, &biases, 2, 1, &syn_types,
        );
        assert_eq!(result[0], STRUCTURAL_SYNAPSE_TARGETS_INPUT);
    }

    #[test]
    fn structural_constant_has_inward() {
        let from = [0u32, 2];
        let to = [2u32, 3];
        let is_const = [0u8, 0, 1, 0];
        let squash = [0u8, 0, 0, 7];
        let biases = [0.0f64, 0.0, 1.0, -0.3];
        let syn_types = [0u8, 0];

        let result = validate_structural_integrity(
            &from, &to, &is_const, &squash, &biases, 2, 1, &syn_types,
        );
        assert_eq!(result[0], STRUCTURAL_CONSTANT_HAS_INWARD);
    }

    #[test]
    fn structural_hidden_no_inward() {
        let from = [2u32];
        let to = [3u32];
        let is_const = [0u8, 0, 0, 0];
        let squash = [0u8, 0, 1, 7];
        let biases = [0.0f64, 0.0, 0.5, -0.3];
        let syn_types = [0u8];

        let result = validate_structural_integrity(
            &from, &to, &is_const, &squash, &biases, 2, 1, &syn_types,
        );
        assert_eq!(result[0], STRUCTURAL_HIDDEN_NO_INWARD);
    }

    #[test]
    fn structural_hidden_no_outward() {
        let from = [0u32, 1];
        let to = [2u32, 3];
        let is_const = [0u8, 0, 0, 0];
        let squash = [0u8, 0, 1, 7];
        let biases = [0.0f64, 0.0, 0.5, -0.3];
        let syn_types = [0u8, 0];

        let result = validate_structural_integrity(
            &from, &to, &is_const, &squash, &biases, 2, 1, &syn_types,
        );
        assert_eq!(result[0], STRUCTURAL_HIDDEN_NO_OUTWARD);
    }

    #[test]
    fn structural_bias_not_finite() {
        let from = [0u32, 2];
        let to = [2u32, 3];
        let is_const = [0u8, 0, 0, 0];
        let squash = [0u8, 0, 1, 7];
        let biases = [0.0f64, 0.0, f64::INFINITY, -0.3];
        let syn_types = [0u8, 0];

        let result = validate_structural_integrity(
            &from, &to, &is_const, &squash, &biases, 2, 1, &syn_types,
        );
        assert_eq!(result[0], STRUCTURAL_BIAS_NOT_FINITE);
    }

    #[test]
    fn structural_bias_nan() {
        let from = [0u32, 2];
        let to = [2u32, 3];
        let is_const = [0u8, 0, 0, 0];
        let squash = [0u8, 0, 1, 7];
        let biases = [0.0f64, 0.0, f64::NAN, -0.3];
        let syn_types = [0u8, 0];

        let result = validate_structural_integrity(
            &from, &to, &is_const, &squash, &biases, 2, 1, &syn_types,
        );
        assert_eq!(result[0], STRUCTURAL_BIAS_NOT_FINITE);
    }

    #[test]
    fn structural_if_too_few_inward() {
        let from = [0u32, 1, 3];
        let to = [3u32, 3, 4];
        let is_const = [0u8, 0, 0, 0, 0];
        let squash = [0u8, 0, 0, IF_SQUASH, 0];
        let biases = [0.0f64, 0.0, 0.0, 0.0, 0.0];
        let syn_types = [SYN_CONDITION, SYN_POSITIVE, 0];

        let result = validate_structural_integrity(
            &from, &to, &is_const, &squash, &biases, 3, 1, &syn_types,
        );
        assert_eq!(result[0], STRUCTURAL_IF_TOO_FEW_INWARD);
    }

    #[test]
    fn structural_if_missing_negative() {
        let from = [0u32, 1, 2, 3];
        let to = [3u32, 3, 3, 4];
        let is_const = [0u8, 0, 0, 0, 0];
        let squash = [0u8, 0, 0, IF_SQUASH, 0];
        let biases = [0.0f64, 0.0, 0.0, 0.0, 0.0];
        let syn_types = [SYN_CONDITION, SYN_POSITIVE, SYN_POSITIVE, 0];

        let result = validate_structural_integrity(
            &from, &to, &is_const, &squash, &biases, 3, 1, &syn_types,
        );
        assert_eq!(result[0], STRUCTURAL_IF_MISSING_NEGATIVE);
    }

    #[test]
    fn structural_if_valid() {
        let from = [0u32, 1, 2, 3];
        let to = [3u32, 3, 3, 4];
        let is_const = [0u8, 0, 0, 0, 0];
        let squash = [0u8, 0, 0, IF_SQUASH, 0];
        let biases = [0.0f64, 0.0, 0.0, 0.0, 0.0];
        let syn_types = [SYN_CONDITION, SYN_POSITIVE, SYN_NEGATIVE, 0];

        let result = validate_structural_integrity(
            &from, &to, &is_const, &squash, &biases, 3, 1, &syn_types,
        );
        assert_eq!(result[0], STRUCTURAL_VALID);
    }

    // -----------------------------------------------------------------------
    // detect_cycles (Issue #1961)
    // -----------------------------------------------------------------------

    #[test]
    fn detect_cycles_acyclic() {
        let from = [0u32, 1, 2];
        let to = [2u32, 2, 3];
        assert_eq!(detect_cycles(&from, &to, 4, 2), 0);
    }

    #[test]
    fn detect_cycles_with_cycle() {
        let from = [0u32, 1, 2, 3];
        let to = [2u32, 3, 3, 2];
        assert_eq!(detect_cycles(&from, &to, 4, 2), 1);
    }

    #[test]
    fn detect_cycles_self_loop() {
        let from = [0u32, 2];
        let to = [2u32, 2];
        assert_eq!(detect_cycles(&from, &to, 3, 1), 1);
    }

    #[test]
    fn detect_cycles_longer_cycle() {
        let from = [0u32, 1, 2, 3, 4, 5];
        let to = [3u32, 4, 5, 4, 5, 3];
        assert_eq!(detect_cycles(&from, &to, 7, 3), 1);
    }

    #[test]
    fn detect_cycles_empty() {
        let from: [u32; 0] = [];
        let to: [u32; 0] = [];
        assert_eq!(detect_cycles(&from, &to, 3, 2), 0);
    }

    // -----------------------------------------------------------------------
    // compute_reverse_topological_order — CSR differential guard (Issue #388)
    //
    // `reference_reverse_topological_order` below is the pre-#388
    // `Vec<Vec<u32>>` implementation, kept verbatim as the behavioural
    // oracle. The CSR rewrite is an allocation change only, so every input
    // must produce an *element-identical* order. Any off-by-one in the
    // prefix sum or the cursor fill permutes or truncates the result and
    // fails these tests at `cargo test` time.
    // -----------------------------------------------------------------------

    /// Pre-#388 reference: inward adjacency as one `Vec<u32>` per neuron.
    fn reference_reverse_topological_order(
        from_indices: &[u32],
        to_indices: &[u32],
        num_neurons: u32,
        num_inputs: u32,
    ) -> Vec<u32> {
        let n = num_neurons as usize;
        let input_count = num_inputs as usize;

        if from_indices.len() != to_indices.len() {
            return Vec::new();
        }
        if input_count > n {
            return Vec::new();
        }

        let mut out_degree = vec![0i32; n];
        let mut inward: Vec<Vec<u32>> = vec![Vec::new(); n];

        for i in 0..from_indices.len() {
            let from = from_indices[i] as usize;
            let to = to_indices[i] as usize;

            if from == to {
                continue;
            }
            if from >= n || to >= n {
                continue;
            }
            if from >= input_count {
                out_degree[from] += 1;
            }
            inward[to].push(from as u32);
        }

        let mut queue: Vec<usize> = Vec::new();
        for i in input_count..n {
            if out_degree[i] == 0 {
                queue.push(i);
            }
        }

        let mut result: Vec<u32> = Vec::new();
        let mut visited = vec![false; n];
        let mut head = 0;

        while head < queue.len() {
            let idx = queue[head];
            head += 1;

            if visited[idx] {
                continue;
            }
            visited[idx] = true;
            result.push(idx as u32);

            for j in 0..inward[idx].len() {
                let from = inward[idx][j] as usize;
                if from == idx {
                    continue;
                }
                if from < input_count {
                    continue;
                }
                if visited[from] {
                    continue;
                }

                out_degree[from] -= 1;
                if out_degree[from] <= 0 {
                    queue.push(from);
                }
            }
        }

        for i in input_count..n {
            if !visited[i] {
                result.push(i as u32);
            }
        }

        result
    }

    /// Build a random forward-only DAG: every real `from` is strictly earlier
    /// than its `to`, so the edge list is acyclic by construction.
    ///
    /// Self-loops and out-of-range endpoints are sprinkled in deliberately —
    /// both passes of the CSR build must filter them *identically*, and a
    /// clean DAG would never expose a mismatch.
    fn random_dag(rng: &mut TestRng, n: usize, input_count: usize) -> (Vec<u32>, Vec<u32>) {
        let mut from_indices = Vec::new();
        let mut to_indices = Vec::new();
        for to in input_count..n {
            if rng.below(4) == 0 {
                // Self-loop: counted nowhere, dropped by both passes.
                from_indices.push(to as u32);
                to_indices.push(to as u32);
            }
            if rng.below(8) == 0 {
                // Out-of-range endpoint: skipped, never indexed.
                from_indices.push((n + 7) as u32);
                to_indices.push(to as u32);
            }
            let fan = rng.below(5);
            for _ in 0..fan {
                let from = rng.below(to);
                from_indices.push(from as u32);
                to_indices.push(to as u32);
            }
        }
        (from_indices, to_indices)
    }

    #[test]
    fn reverse_topological_order_matches_reference_on_random_dags() {
        let mut rng = TestRng(0x1234_5678_9ABC_DEF0);
        for case in 0..200u32 {
            let n = 2 + rng.below(60);
            let input_count = rng.below(n);
            let (from, to) = random_dag(&mut rng, n, input_count);

            let expected =
                reference_reverse_topological_order(&from, &to, n as u32, input_count as u32);
            let actual =
                compute_reverse_topological_order(&from, &to, n as u32, input_count as u32);

            assert_eq!(
                actual, expected,
                "case {case}: n={n} inputs={input_count} from={from:?} to={to:?}"
            );
        }
    }

    #[test]
    fn reverse_topological_order_matches_reference_on_malformed_inputs() {
        // Self-loops, out-of-range endpoints, duplicate edges, back edges
        // (cycles) and degenerate neuron counts — the defensive paths.
        let cases: [(&[u32], &[u32], u32, u32); 11] = [
            (&[0, 1, 99], &[2, 2, 3], 4, 2),
            (&[0, 1], &[2, 99], 4, 2),
            (&[0, 1, 2], &[2, 2], 4, 2),
            (&[0], &[1], 2, 99),
            (&[2, 2], &[2, 3], 4, 2),
            (&[3, 4], &[4, 3], 5, 2),
            (&[0, 0, 0], &[2, 2, 2], 3, 1),
            (&[], &[], 5, 2),
            (&[], &[], 0, 0),
            (&[2, 3, 4], &[3, 4, 2], 6, 2),
            // A self-loop with no input neurons: if the counting pass keeps
            // the self-loop the fill pass drops, neuron 0's run gains an
            // unwritten slot that reads as a phantom inward edge from
            // neuron 0 and releases it into the queue one step early.
            (&[1, 0, 0, 5], &[1, 2, 3, 2], 6, 0),
        ];

        for (i, (from, to, n, inputs)) in cases.iter().enumerate() {
            let expected = reference_reverse_topological_order(from, to, *n, *inputs);
            let actual = compute_reverse_topological_order(from, to, *n, *inputs);
            assert_eq!(actual, expected, "malformed case {i}");
        }
    }

    #[test]
    fn reverse_topological_order_appends_cycle_members_in_ascending_order() {
        // 2 inputs; neurons 2..4 form a cycle (2→3→4→2), 5 is a clean tail.
        let from = [0u32, 2, 3, 4, 1];
        let to = [2u32, 3, 4, 2, 5];
        let result = compute_reverse_topological_order(&from, &to, 6, 2);
        // 5 is reachable by Kahn's walk; the cycle members follow, ascending.
        assert_eq!(result, vec![5, 2, 3, 4]);
    }
}
