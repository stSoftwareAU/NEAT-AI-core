//! Wiring invariants shared by the topology validators (Issue #560).
//!
//! Two validators ask the same structural questions of a creature:
//! [`crate::topology_ops::validate_structural_integrity`] (flat typed arrays,
//! answers with a numeric code and a neuron index) and
//! [`crate::creature_validate::creature_validate`] (a `CreatureExport`, answers
//! with the TypeScript message NEAT-AI's tests assert on). The *answers* differ,
//! so neither can call the other — but the questions must not be asked twice, or
//! the two stacks drift on what "wired in" means.
//!
//! This module is the single home of those questions:
//!
//! - [`ConnectionIndex`] — inward / outward degree and the inward synapse list,
//!   built once in `O(neurons + synapses)`, so no caller rescans the synapse
//!   list per neuron.
//! - [`hidden_wiring_fault`] — a hidden neuron needs an inward *and* an outward
//!   edge, inward reported first.
//! - [`if_neuron_fault`] — an `IF` neuron needs [`IF_MINIMUM_INWARD`] inward
//!   edges carrying all three roles, reported condition → positive → negative.
//!
//! What is deliberately *not* here: the checks that are a single comparison over
//! the values above (`constant has inward`, `bias is finite`). Wrapping
//! `inward_count(i) > 0` in a named function moves no logic and only adds a hop
//! for a reader to follow.

use crate::synapse_type::SynapseType;

/// Inward edges an `IF` neuron must carry — one per role, at minimum.
pub const IF_MINIMUM_INWARD: usize = 3;

/// Inward / outward adjacency for one topology, built once from its synapse
/// list.
///
/// Out-of-range endpoints are ignored rather than rejected: the callers police
/// their own index space (`validate_structural_integrity` returns
/// `STRUCTURAL_MALFORMED_BUFFER`, `creature_validate` resolves every UUID before
/// building the index), and a WASM export must not trap on a bad buffer.
pub struct ConnectionIndex {
    /// `inward_starts[n]..inward_starts[n + 1]` slices [`Self::inward_synapses`].
    inward_starts: Vec<u32>,
    /// Synapse indices, grouped by destination neuron.
    inward_synapses: Vec<u32>,
    /// Outward degree per neuron.
    outward_counts: Vec<u32>,
}

impl ConnectionIndex {
    /// Build the index for `num_neurons` neurons wired by `from` / `to`.
    ///
    /// # Panics
    ///
    /// Panics when `from` and `to` differ in length — a caller that lost a
    /// synapse endpoint must fail loudly rather than validate half a topology.
    pub fn build(from: &[u32], to: &[u32], num_neurons: usize) -> Self {
        assert_eq!(
            from.len(),
            to.len(),
            "synapse endpoint lists must be the same length"
        );

        let mut inward_starts = vec![0u32; num_neurons + 1];
        let mut outward_counts = vec![0u32; num_neurons];

        for i in 0..from.len() {
            let source = from[i] as usize;
            let target = to[i] as usize;
            if source < num_neurons {
                outward_counts[source] += 1;
            }
            if target < num_neurons {
                // Counting pass: offset by one so the prefix sum below lands
                // each neuron's start without a second shift.
                inward_starts[target + 1] += 1;
            }
        }

        for n in 0..num_neurons {
            inward_starts[n + 1] += inward_starts[n];
        }

        let total_inward = inward_starts[num_neurons] as usize;
        let mut inward_synapses = vec![0u32; total_inward];
        let mut cursor = inward_starts.clone();
        for i in 0..to.len() {
            let target = to[i] as usize;
            if target < num_neurons {
                inward_synapses[cursor[target] as usize] = i as u32;
                cursor[target] += 1;
            }
        }

        Self {
            inward_starts,
            inward_synapses,
            outward_counts,
        }
    }

    /// Number of neurons the index covers.
    pub fn num_neurons(&self) -> usize {
        self.outward_counts.len()
    }

    /// How many synapses arrive at `neuron`.
    ///
    /// # Panics
    ///
    /// Panics when `neuron` is outside `0..num_neurons`.
    pub fn inward_count(&self, neuron: usize) -> usize {
        self.inward_synapses(neuron).len()
    }

    /// How many synapses leave `neuron`.
    ///
    /// # Panics
    ///
    /// Panics when `neuron` is outside `0..num_neurons`.
    pub fn outward_count(&self, neuron: usize) -> usize {
        self.outward_counts[neuron] as usize
    }

    /// Indices of the synapses arriving at `neuron`, in declaration order.
    ///
    /// # Panics
    ///
    /// Panics when `neuron` is outside `0..num_neurons`.
    pub fn inward_synapses(&self, neuron: usize) -> &[u32] {
        let start = self.inward_starts[neuron] as usize;
        let end = self.inward_starts[neuron + 1] as usize;
        &self.inward_synapses[start..end]
    }
}

/// Which of the three `IF` roles a neuron's inward synapses carry.
///
/// A synapse with no declared type is a positive input — NEAT-AI's
/// `c.type ?? "positive"` and this crate's [`SynapseType::Standard`] are the
/// same case.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IfRoles {
    /// At least one `condition` input.
    pub condition: bool,
    /// At least one `positive` (or untyped) input.
    pub positive: bool,
    /// At least one `negative` input.
    pub negative: bool,
}

impl IfRoles {
    /// Tally the roles carried by a neuron's inward synapse types.
    pub fn tally(types: impl IntoIterator<Item = SynapseType>) -> Self {
        let mut roles = Self::default();
        for synapse_type in types {
            match synapse_type {
                SynapseType::Condition => roles.condition = true,
                SynapseType::Negative => roles.negative = true,
                SynapseType::Positive | SynapseType::Standard => roles.positive = true,
            }
        }
        roles
    }
}

/// Why a hidden neuron is not wired into the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WiringFault {
    /// Nothing feeds the neuron.
    NoInward,
    /// The neuron feeds nothing.
    NoOutward,
}

/// The first wiring fault of a hidden neuron, or `None` when it is wired in.
///
/// Inward is reported before outward, which is the order NEAT-AI's
/// `CreatureValidate.ts` raises them in.
pub fn hidden_wiring_fault(inward: usize, outward: usize) -> Option<WiringFault> {
    if inward == 0 {
        return Some(WiringFault::NoInward);
    }
    if outward == 0 {
        return Some(WiringFault::NoOutward);
    }
    None
}

/// Why an `IF` neuron cannot make its decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IfFault {
    /// Fewer than [`IF_MINIMUM_INWARD`] inward synapses.
    TooFewInward {
        /// Inward synapses the neuron actually has.
        found: usize,
    },
    /// No `condition` input — nothing to branch on.
    MissingCondition,
    /// No `positive` (or untyped) input — the true branch is empty.
    MissingPositive,
    /// No `negative` input — the false branch is empty.
    MissingNegative,
}

/// The first `IF` fault of a neuron, or `None` when all three roles are present.
///
/// Count first, then condition → positive → negative: the order NEAT-AI's
/// `CreatureValidate.ts` raises them in, and the order
/// `validate_structural_integrity` returns its `STRUCTURAL_IF_*` codes in.
pub fn if_neuron_fault(inward: usize, roles: IfRoles) -> Option<IfFault> {
    if inward < IF_MINIMUM_INWARD {
        return Some(IfFault::TooFewInward { found: inward });
    }
    if !roles.condition {
        return Some(IfFault::MissingCondition);
    }
    if !roles.positive {
        return Some(IfFault::MissingPositive);
    }
    if !roles.negative {
        return Some(IfFault::MissingNegative);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Degrees and the inward synapse list come from one build pass, and a
    /// neuron with no edges reads as zero rather than panicking.
    #[test]
    fn the_index_reports_degrees_and_inward_synapses_per_neuron() {
        // 0 -> 2, 1 -> 2, 2 -> 3, and neuron 4 wired to nothing.
        let from = [0u32, 1, 2];
        let to = [2u32, 2, 3];
        let index = ConnectionIndex::build(&from, &to, 5);

        assert_eq!(index.num_neurons(), 5);
        assert_eq!(index.inward_count(0), 0);
        assert_eq!(index.outward_count(0), 1);
        assert_eq!(index.inward_count(2), 2);
        assert_eq!(index.outward_count(2), 1);
        assert_eq!(index.inward_synapses(2), &[0, 1], "declaration order");
        assert_eq!(index.inward_synapses(3), &[2]);
        assert_eq!(index.inward_count(4), 0);
        assert_eq!(index.outward_count(4), 0);
    }

    /// An endpoint outside the neuron range is ignored, so a malformed buffer
    /// cannot index out of bounds (or trap in WASM).
    #[test]
    fn out_of_range_endpoints_are_ignored() {
        let index = ConnectionIndex::build(&[0, 9], &[1, 9], 2);

        assert_eq!(index.outward_count(0), 1);
        assert_eq!(index.inward_count(1), 1);
        assert_eq!(index.inward_synapses(1), &[0]);
    }

    #[test]
    #[should_panic(expected = "same length")]
    fn mismatched_endpoint_lists_fail_loudly() {
        let _ = ConnectionIndex::build(&[0, 1], &[1], 2);
    }

    /// An untyped synapse is a positive input, the same reading NEAT-AI's
    /// `c.type ?? "positive"` gives it.
    #[test]
    fn an_untyped_synapse_counts_as_the_positive_role() {
        let roles = IfRoles::tally([SynapseType::Standard]);

        assert_eq!(
            roles,
            IfRoles {
                condition: false,
                positive: true,
                negative: false
            }
        );
    }

    #[test]
    fn roles_tally_condition_positive_and_negative() {
        let roles = IfRoles::tally([
            SynapseType::Condition,
            SynapseType::Negative,
            SynapseType::Positive,
        ]);

        assert_eq!(
            roles,
            IfRoles {
                condition: true,
                positive: true,
                negative: true
            }
        );
        assert_eq!(IfRoles::tally([]), IfRoles::default());
    }

    #[test]
    fn a_hidden_neuron_needs_an_inward_edge_before_an_outward_one() {
        assert_eq!(hidden_wiring_fault(0, 0), Some(WiringFault::NoInward));
        assert_eq!(hidden_wiring_fault(0, 3), Some(WiringFault::NoInward));
        assert_eq!(hidden_wiring_fault(2, 0), Some(WiringFault::NoOutward));
        assert_eq!(hidden_wiring_fault(2, 3), None);
    }

    #[test]
    fn an_if_neuron_reports_its_count_then_each_missing_role_in_turn() {
        let all = IfRoles {
            condition: true,
            positive: true,
            negative: true,
        };

        assert_eq!(
            if_neuron_fault(2, all),
            Some(IfFault::TooFewInward { found: 2 }),
            "the count is checked before the roles"
        );
        assert_eq!(if_neuron_fault(IF_MINIMUM_INWARD, all), None);
        assert_eq!(
            if_neuron_fault(
                3,
                IfRoles {
                    condition: false,
                    ..all
                }
            ),
            Some(IfFault::MissingCondition)
        );
        assert_eq!(
            if_neuron_fault(
                3,
                IfRoles {
                    positive: false,
                    ..all
                }
            ),
            Some(IfFault::MissingPositive)
        );
        assert_eq!(
            if_neuron_fault(
                3,
                IfRoles {
                    negative: false,
                    ..all
                }
            ),
            Some(IfFault::MissingNegative)
        );
    }
}
