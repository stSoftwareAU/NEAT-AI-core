//! Input-to-output reachability diagnostics (Issue #759).
//!
//! Every neuron is classed by two O(V + E) sweeps: forward from the inputs and
//! backward from the outputs ([`ReachabilityClass`]):
//!
//! - `Live` — reached from an input and reaches an output.
//! - `BiasDriven` — reaches an output but no input reaches it. This is
//!   constant-driven computation that still contributes a value, so the
//!   absence of an input path alone does **not** prove a neuron removable.
//! - `DeadEnd` — reached from an input but reaches no output.
//! - `Isolated` — neither.
//!
//! This is a diagnostic only and never rewrites a creature:
//! [`crate::prune_cleanup::cleanup_creature`] remains the sole rewrite owner.
//! In a `forwardOnly` creature after cleanup every interior neuron is `Live`
//! or `BiasDriven`, because cleanup removes nodes with no outward edge to a
//! fixed point. Recurrent edges are handled; duplicate and unsorted edges do
//! not change the answer.
//!
//! This is opt-in research tooling for Issue #759 and changes no default.
//!
//! Indices follow the compiled-network convention: `0..num_inputs` are inputs,
//! the last `num_outputs` indices are outputs, and everything between is a
//! constant or hidden neuron.

use std::fmt;

/// Per-neuron reachability class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ReachabilityClass {
    /// Reached from an input AND reaches an output.
    Live = 0,
    /// Reaches an output, but no input path reaches it: constant/bias-driven
    /// computation. Not removable on reachability alone — it still contributes
    /// a value to the output (cleanup keeps it — a hidden neuron with no
    /// inward edge is folded exactly into a support constant, never dropped).
    BiasDriven = 1,
    /// Reached from an input but reaches no output: dead structure nothing reads.
    DeadEnd = 2,
    /// Neither reached from an input nor reaching an output.
    Isolated = 3,
}

/// Why a reachability request was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReachabilityError {
    /// The parallel edge arrays differ in length.
    MismatchedEdgeLengths {
        /// Length of the source array.
        from_len: usize,
        /// Length of the target array.
        to_len: usize,
    },
    /// `num_inputs + num_outputs > num_neurons`, or `num_outputs == 0`.
    InvalidWidths {
        /// Neuron count.
        num_neurons: usize,
        /// Declared inputs.
        num_inputs: usize,
        /// Declared outputs.
        num_outputs: usize,
    },
    /// An edge endpoint is `>= num_neurons`.
    EdgeOutOfRange {
        /// Offending edge position.
        edge_index: usize,
        /// Source index.
        from: u32,
        /// Target index.
        to: u32,
        /// Neuron count.
        num_neurons: usize,
    },
}

impl fmt::Display for ReachabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MismatchedEdgeLengths { from_len, to_len } => write!(
                f,
                "edge arrays differ in length: {from_len} sources, {to_len} targets"
            ),
            Self::InvalidWidths {
                num_neurons,
                num_inputs,
                num_outputs,
            } => write!(
                f,
                "invalid widths: {num_inputs} inputs + {num_outputs} outputs must fit in \
                 {num_neurons} neurons with at least one output"
            ),
            Self::EdgeOutOfRange {
                edge_index,
                from,
                to,
                num_neurons,
            } => write!(
                f,
                "edge {edge_index} ({from} -> {to}) has an endpoint outside 0..{num_neurons}"
            ),
        }
    }
}

impl std::error::Error for ReachabilityError {}

/// Counts of interior (constant and hidden) neurons by class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClassCounts {
    /// Reached from an input and reaching an output.
    pub live: usize,
    /// Reaching an output with no input path.
    pub bias_driven: usize,
    /// Reached from an input, reaching no output.
    pub dead_end: usize,
    /// Neither reached nor reaching.
    pub isolated: usize,
}

/// The per-neuron classes for one network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachabilityReport {
    classes: Vec<ReachabilityClass>,
    num_inputs: usize,
    num_outputs: usize,
}

impl ReachabilityReport {
    /// One class per neuron, in index order.
    pub fn classes(&self) -> &[ReachabilityClass] {
        &self.classes
    }

    /// The class of `neuron`, or `None` when it is out of range.
    pub fn class_of(&self, neuron: usize) -> Option<ReachabilityClass> {
        self.classes.get(neuron).copied()
    }

    /// Counts over the non-input, non-output neurons only.
    pub fn interior_counts(&self) -> ClassCounts {
        let mut counts = ClassCounts::default();
        let end = self.classes.len() - self.num_outputs;
        for class in &self.classes[self.num_inputs..end] {
            match class {
                ReachabilityClass::Live => counts.live += 1,
                ReachabilityClass::BiasDriven => counts.bias_driven += 1,
                ReachabilityClass::DeadEnd => counts.dead_end += 1,
                ReachabilityClass::Isolated => counts.isolated += 1,
            }
        }
        counts
    }

    /// Inputs with no path to any output.
    pub fn unused_inputs(&self) -> Vec<usize> {
        (0..self.num_inputs)
            .filter(|&i| self.classes[i] == ReachabilityClass::DeadEnd)
            .collect()
    }

    /// Outputs no input reaches: constant outputs.
    pub fn input_independent_outputs(&self) -> Vec<usize> {
        (self.classes.len() - self.num_outputs..self.classes.len())
            .filter(|&i| self.classes[i] == ReachabilityClass::BiasDriven)
            .collect()
    }
}

/// Compressed adjacency: neighbours of `n` are `targets[offsets[n]..offsets[n + 1]]`.
fn csr(keys: &[u32], values: &[u32], num_neurons: usize) -> (Vec<usize>, Vec<u32>) {
    let mut offsets = vec![0usize; num_neurons + 1];
    for &k in keys {
        offsets[k as usize + 1] += 1;
    }
    for i in 0..num_neurons {
        offsets[i + 1] += offsets[i];
    }
    let mut cursor = offsets.clone();
    let mut targets = vec![0u32; keys.len()];
    for (&k, &v) in keys.iter().zip(values) {
        targets[cursor[k as usize]] = v;
        cursor[k as usize] += 1;
    }
    (offsets, targets)
}

fn sweep(roots: std::ops::Range<usize>, offsets: &[usize], targets: &[u32]) -> Vec<bool> {
    let mut seen = vec![false; offsets.len() - 1];
    let mut stack: Vec<usize> = roots.collect();
    for &r in &stack {
        seen[r] = true;
    }
    while let Some(n) = stack.pop() {
        for &next in &targets[offsets[n]..offsets[n + 1]] {
            let next = next as usize;
            if !seen[next] {
                seen[next] = true;
                stack.push(next);
            }
        }
    }
    seen
}

/// Classes every neuron in O(V + E).
///
/// Inputs are always input-reached and outputs always output-reaching.
///
/// # Errors
///
/// Refuses mismatched edge arrays, widths that do not fit (or no output), a
/// `num_neurons` above `u32::MAX` (edges are `u32`, so no edge could name such
/// an index, and the per-neuron vectors are never allocated for it), and edge
/// endpoints outside `0..num_neurons`.
pub fn reachability_diagnostics(
    from_indices: &[u32],
    to_indices: &[u32],
    num_neurons: usize,
    num_inputs: usize,
    num_outputs: usize,
) -> Result<ReachabilityReport, ReachabilityError> {
    if from_indices.len() != to_indices.len() {
        return Err(ReachabilityError::MismatchedEdgeLengths {
            from_len: from_indices.len(),
            to_len: to_indices.len(),
        });
    }
    let fits = num_inputs
        .checked_add(num_outputs)
        .is_some_and(|sum| sum <= num_neurons);
    if num_outputs == 0 || !fits || num_neurons > u32::MAX as usize {
        return Err(ReachabilityError::InvalidWidths {
            num_neurons,
            num_inputs,
            num_outputs,
        });
    }
    for (edge_index, (&from, &to)) in from_indices.iter().zip(to_indices).enumerate() {
        if from as usize >= num_neurons || to as usize >= num_neurons {
            return Err(ReachabilityError::EdgeOutOfRange {
                edge_index,
                from,
                to,
                num_neurons,
            });
        }
    }

    let (fwd_offsets, fwd_targets) = csr(from_indices, to_indices, num_neurons);
    let (rev_offsets, rev_targets) = csr(to_indices, from_indices, num_neurons);
    let from_input = sweep(0..num_inputs, &fwd_offsets, &fwd_targets);
    let to_output = sweep(
        num_neurons - num_outputs..num_neurons,
        &rev_offsets,
        &rev_targets,
    );

    let classes = from_input
        .iter()
        .zip(&to_output)
        .map(|(&i, &o)| match (i, o) {
            (true, true) => ReachabilityClass::Live,
            (false, true) => ReachabilityClass::BiasDriven,
            (true, false) => ReachabilityClass::DeadEnd,
            (false, false) => ReachabilityClass::Isolated,
        })
        .collect();
    Ok(ReachabilityReport {
        classes,
        num_inputs,
        num_outputs,
    })
}
