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
        return vec![SORT_ERROR_FROM, 0];
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

/// Scan for available forward-only connection slots.
///
/// Returns all `(from, to)` pairs where `from < to`, `to >= num_inputs`, the
/// target neuron is not constant, and no connection already exists. Uses a
/// flat boolean array for O(1) existence checks.
///
/// # Returns
/// Flattened `[from, to, from, to, ...]` pairs.
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

    let mut conn_set = vec![false; n * n];
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

    let mut out_degree = vec![0i32; n];
    let mut inward: Vec<Vec<u32>> = vec![Vec::new(); n];

    for i in 0..from_indices.len() {
        let from = from_indices[i] as usize;
        let to = to_indices[i] as usize;

        if from == to {
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
    fn validate_mismatched_lengths_reports_error() {
        let from = [0u32, 1];
        let to = [2u32];
        let result = validate_topology(&from, &to);
        assert_eq!(result[0], SORT_ERROR_FROM);
    }

    // -----------------------------------------------------------------------
    // scan_available_connections (Issue #1959)
    // -----------------------------------------------------------------------

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
}
