//! Issue #298 — wasm64 lane (c) training-data offload suite.
//!
//! These "what" tests exercise the observable offload contract from milestone
//! #295: neat-core owns the training dataset in its own linear memory, JS holds
//! a handle, and per-generation evaluation reads batches **by index** without
//! the dataset re-crossing the boundary. The registry's live/peak byte counters
//! stand in for WASM linear-memory footprint so a load → evaluate → free
//! lifecycle leak is observable natively (the wasm-bindgen shims are thin
//! wrappers over exactly this API).

use neat_core::network::CompiledNetwork;
use neat_core::training_data::TrainingDataConfig;
use neat_core::wasm_dataset::{DatasetError, DatasetRegistry, TrainingDataset};

/// Build a compiled network with `num_inputs` inputs and a single IDENTITY
/// output neuron that sums every input (weight 1.0) and adds `bias`.
///
/// Output for record `x` is therefore `sum(x) + bias`, so expected MSE against
/// a target is easy to compute by hand in the assertions below.
fn summing_identity_network(num_inputs: usize, bias: f64) -> CompiledNetwork {
    let mut data = Vec::new();
    let num_neurons = (num_inputs + 1) as u32;
    data.extend_from_slice(&num_neurons.to_le_bytes());
    data.extend_from_slice(&(num_inputs as u32).to_le_bytes());

    // Single output neuron.
    data.extend_from_slice(&bias.to_le_bytes()); // bias f64
    data.push(0); // squash IDENTITY
    data.push(0); // is_constant = false
    data.extend_from_slice(&(num_inputs as u16).to_le_bytes()); // num_synapses

    // One synapse per input, weight 1.0.
    for from_index in 0..num_inputs as u16 {
        data.extend_from_slice(&from_index.to_le_bytes());
        data.push(0); // synapse_type
        data.push(0); // padding
        data.extend_from_slice(&1.0_f64.to_le_bytes()); // weight f64
    }

    CompiledNetwork::new(&data).expect("network should parse")
}

/// Pack interleaved records (each: inputs then outputs) into `.bin` bytes.
fn pack(records: &[Vec<f32>]) -> Vec<u8> {
    records
        .iter()
        .flat_map(|r| r.iter().flat_map(|v| v.to_le_bytes()))
        .collect()
}

#[test]
fn evaluation_reads_the_batch_by_index_from_owned_memory() {
    // 3 records, 2 inputs + 1 target each. output = i0 + i1 + bias(0.0).
    let bytes = pack(&[
        vec![1.0, 2.0, 3.0], // predict 3.0, target 3.0 → err 0
        vec![2.0, 2.0, 5.0], // predict 4.0, target 5.0 → err 1
        vec![0.0, 0.0, 2.0], // predict 0.0, target 2.0 → err 4
    ]);
    let ds = TrainingDataset::from_packed_bytes(&bytes, TrainingDataConfig::new(2, 1)).unwrap();
    let mut net = summing_identity_network(2, 0.0);

    // Evaluate only the last two records by index — MSE = (1 + 4) / 2 = 2.5.
    let mse = ds.evaluate_mse(&mut net, 1, 2).unwrap();
    assert!((mse - 2.5).abs() < 1e-5, "batch MSE was {mse}");

    // A different batch reads a different slice of the same owned buffer.
    let first = ds.evaluate_mse(&mut net, 0, 1).unwrap();
    assert!(
        first.abs() < 1e-6,
        "first record should be exact, got {first}"
    );
}

#[test]
fn evaluation_does_not_reload_or_mutate_the_dataset() {
    let bytes = pack(&[vec![1.0, 1.0], vec![2.0, 3.0], vec![4.0, 4.0]]);
    let ds = TrainingDataset::from_packed_bytes(&bytes, TrainingDataConfig::new(1, 1)).unwrap();
    let mut net = summing_identity_network(1, 0.0);

    let footprint_before = ds.byte_len();
    let records_before = ds.num_records();

    // Many generations over the whole dataset: the result is deterministic and
    // the owned buffer neither grows nor shrinks — the dataset is not re-passed
    // or re-marshalled per call.
    let first = ds.evaluate_mse(&mut net, 0, records_before).unwrap();
    for _ in 0..50 {
        let again = ds.evaluate_mse(&mut net, 0, records_before).unwrap();
        assert_eq!(
            again, first,
            "evaluation must be deterministic across generations"
        );
    }

    assert_eq!(ds.byte_len(), footprint_before);
    assert_eq!(ds.num_records(), records_before);
}

#[test]
fn evaluation_rejects_network_dataset_shape_mismatch() {
    let bytes = pack(&[vec![1.0, 2.0, 9.0]]);
    let ds = TrainingDataset::from_packed_bytes(&bytes, TrainingDataConfig::new(2, 1)).unwrap();
    // Network takes 3 inputs but the dataset carries 2.
    let mut net = summing_identity_network(3, 0.0);

    let err = ds.evaluate_mse(&mut net, 0, 1).unwrap_err();
    assert_eq!(
        err,
        DatasetError::ShapeMismatch {
            network_inputs: 3,
            dataset_inputs: 2,
        }
    );
}

#[test]
fn evaluation_rejects_out_of_range_batch() {
    let bytes = pack(&[vec![1.0, 1.0], vec![2.0, 2.0]]);
    let ds = TrainingDataset::from_packed_bytes(&bytes, TrainingDataConfig::new(1, 1)).unwrap();
    let mut net = summing_identity_network(1, 0.0);

    let err = ds.evaluate_mse(&mut net, 1, 10).unwrap_err();
    assert_eq!(
        err,
        DatasetError::BatchOutOfRange {
            start: 1,
            count: 10,
            num_records: 2,
        }
    );
}

#[test]
fn load_evaluate_free_lifecycle_keeps_high_water_mark_stable() {
    // The leak gate: repeatedly load a dataset, evaluate several generations,
    // then free it. If any bytes are retained past `free`, `live_bytes` would
    // not return to zero and `peak_bytes` would climb every cycle.
    let mut registry = DatasetRegistry::new();
    let config = TrainingDataConfig::new(2, 1);
    let bytes = pack(&[
        vec![1.0, 2.0, 3.0],
        vec![2.0, 2.0, 4.0],
        vec![3.0, 1.0, 4.0],
        vec![0.5, 0.5, 1.0],
    ]);
    let mut net = summing_identity_network(2, 0.0);

    let mut peak_after_first_cycle = None;
    for _cycle in 0..8 {
        let handle = registry.load_packed(&bytes, config.clone()).unwrap();

        // Evaluate N "generations" reading batches by index from linear memory.
        let ds = registry.get(handle).unwrap();
        let num_records = ds.num_records();
        for _generation in 0..5 {
            let mse = ds.evaluate_mse(&mut net, 0, num_records).unwrap();
            assert!(mse.is_finite());
        }

        // Live footprint is exactly one dataset while loaded.
        assert_eq!(registry.live_bytes(), 4 * 3 * 4);

        registry.free(handle).unwrap();

        // Every byte released on free — nothing retained.
        assert_eq!(registry.live_bytes(), 0, "dataset bytes leaked past free");
        assert_eq!(registry.live_count(), 0);

        // Peak must not grow across cycles — equal-sized reload reuses memory.
        match peak_after_first_cycle {
            None => peak_after_first_cycle = Some(registry.peak_bytes()),
            Some(peak) => assert_eq!(
                registry.peak_bytes(),
                peak,
                "linear-memory high-water mark grew across load/free cycles"
            ),
        }
    }

    assert_eq!(peak_after_first_cycle, Some(4 * 3 * 4));
}

#[test]
fn registry_hands_out_handles_not_bytes() {
    // JS holds only the returned u32 handle; the bytes stay owned by the
    // registry and are reachable solely through it.
    let mut registry = DatasetRegistry::new();
    let config = TrainingDataConfig::new(1, 1);

    let a = registry
        .load_packed(&pack(&[vec![1.0, 1.0]]), config.clone())
        .unwrap();
    let b = registry
        .load_packed(&pack(&[vec![2.0, 2.0]]), config)
        .unwrap();
    assert_ne!(a, b);
    assert_eq!(registry.live_count(), 2);

    assert_eq!(registry.get(a).unwrap().record_inputs(0).unwrap(), &[1.0]);
    assert_eq!(registry.get(b).unwrap().record_inputs(0).unwrap(), &[2.0]);
}
