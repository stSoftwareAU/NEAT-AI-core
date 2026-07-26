//! Training-data offload into neat-core's own linear memory (Issue #298).
//!
//! wasm64 lane (c) of milestone #295. Lane (a) attributed the ~4 GB Learn
//! ceiling to the **V8 JS heap** (exit-133 / "Reached heap limit"), not WASM
//! linear memory. The relief is to stop retaining the large numeric training
//! arrays on the JS heap: neat-core **owns** the dataset inside its own linear
//! memory, JS holds only a small integer **handle**, and per-generation
//! evaluation reads batches **by index** from the WASM-owned buffers instead of
//! copying the full dataset across the JS↔WASM boundary on every call.
//!
//! # Ownership model
//!
//! ```text
//!   JS                          neat-core linear memory
//!   ──                          ───────────────────────
//!   handle: u32   ──lookup──▶   DatasetRegistry
//!                               └─ TrainingDataset (inputs SoA + targets SoA)
//! ```
//!
//! * [`TrainingDataset`] de-interleaves the packed `.bin` record stream
//!   (`inputs … outputs` per record) into two contiguous structure-of-arrays
//!   buffers **once**, at load time. Batch reads are then contiguous slices.
//! * [`DatasetRegistry`] is the handle table: `load` returns a `u32` handle,
//!   `free` drops the dataset and releases its bytes. It tracks live and
//!   peak footprint so a leak (memory retained past `free`) is observable —
//!   the load → evaluate → free lifecycle high-water mark stays flat.
//! * Evaluation ([`TrainingDataset::evaluate_mse`]) reads inputs and targets
//!   for the requested `[start, start + count)` batch straight from the owned
//!   buffers. The dataset bytes never re-cross the boundary after load.
//!
//! Fail-loud (Issue #3234): every fallible operation returns a [`DatasetError`]
//! — a bad record length, an out-of-range batch, an unknown handle, or a
//! network/dataset shape mismatch surfaces immediately rather than degrading
//! into a silent wrong result.

use crate::batch_scoring::{BatchScratch, RecordBatch};
use crate::network::CompiledNetwork;
use crate::training_data::TrainingDataConfig;

/// Records scored per [`TrainingDataset::evaluate_mse`] chunk.
///
/// A multiple of the 8-record SIMD group, so chunking does not change which
/// records are grouped (and therefore does not change the result), while keeping
/// the scratch output buffer a bounded, constant size no matter how large the
/// requested batch is — the >4 GB Memory64 lane can ask for millions of records.
const EVAL_CHUNK_RECORDS: usize = 1024;

/// Errors from the training-data offload path.
#[derive(Debug, PartialEq)]
pub enum DatasetError {
    /// The configuration specifies zero inputs or zero outputs.
    InvalidConfig {
        /// Human-readable description of the invalid configuration.
        message: String,
    },
    /// The packed byte buffer length is not an exact multiple of the record
    /// size, so it cannot be split cleanly into records.
    UnalignedBuffer {
        /// Length of the supplied buffer in bytes.
        buffer_len: usize,
        /// Expected byte size of a single record.
        record_size: usize,
    },
    /// A requested `[start, start + count)` batch falls outside the dataset.
    BatchOutOfRange {
        /// First record index requested.
        start: usize,
        /// Number of records requested.
        count: usize,
        /// Total records held by the dataset.
        num_records: usize,
    },
    /// A handle did not correspond to a live dataset in the registry.
    UnknownHandle {
        /// The handle that was looked up.
        handle: u32,
    },
    /// The network's input arity does not match the dataset's input arity.
    ShapeMismatch {
        /// Inputs the network expects.
        network_inputs: usize,
        /// Inputs each dataset record carries.
        dataset_inputs: usize,
    },
}

impl std::fmt::Display for DatasetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DatasetError::InvalidConfig { message } => {
                write!(f, "Invalid dataset configuration: {message}")
            }
            DatasetError::UnalignedBuffer {
                buffer_len,
                record_size,
            } => write!(
                f,
                "Buffer length {buffer_len} bytes is not a multiple of record size {record_size}"
            ),
            DatasetError::BatchOutOfRange {
                start,
                count,
                num_records,
            } => write!(
                f,
                "Batch [{start}, {start}+{count}) is out of range for {num_records} records"
            ),
            DatasetError::UnknownHandle { handle } => {
                write!(f, "No live dataset for handle {handle}")
            }
            DatasetError::ShapeMismatch {
                network_inputs,
                dataset_inputs,
            } => write!(
                f,
                "Network expects {network_inputs} inputs but dataset records carry {dataset_inputs}"
            ),
        }
    }
}

impl std::error::Error for DatasetError {}

/// A training dataset owned inside neat-core's linear memory.
///
/// Stored structure-of-arrays: [`inputs`](Self::inputs_len) is `num_records ×
/// num_inputs` contiguous `f32`, [`targets`](Self::targets_len) is
/// `num_records × num_outputs`. This makes an input batch a single contiguous
/// slice, so per-generation evaluation gathers one cache-friendly run rather
/// than re-marshalling per-record arrays across the boundary.
#[derive(Debug, Clone)]
pub struct TrainingDataset {
    config: TrainingDataConfig,
    num_records: usize,
    inputs: Vec<f32>,
    targets: Vec<f32>,
}

impl TrainingDataset {
    /// Load a dataset from the packed `.bin` byte stream produced by the
    /// TypeScript `DataSet` module: each record is `num_inputs + num_outputs`
    /// little-endian `f32` values, records packed contiguously.
    ///
    /// The buffer is de-interleaved into the structure-of-arrays layout once,
    /// here. Fails loud if the buffer length is not a whole number of records.
    pub fn from_packed_bytes(
        bytes: &[u8],
        config: TrainingDataConfig,
    ) -> Result<Self, DatasetError> {
        validate_config(&config)?;
        let record_size = config.bytes_per_record();
        if !bytes.len().is_multiple_of(record_size) {
            return Err(DatasetError::UnalignedBuffer {
                buffer_len: bytes.len(),
                record_size,
            });
        }

        let num_records = bytes.len() / record_size;
        let num_inputs = config.num_inputs;
        let num_outputs = config.num_outputs;
        let mut inputs = Vec::with_capacity(num_records * num_inputs);
        let mut targets = Vec::with_capacity(num_records * num_outputs);

        for record in bytes.chunks_exact(record_size) {
            let values = record
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]));
            for (i, value) in values.enumerate() {
                if i < num_inputs {
                    inputs.push(value);
                } else {
                    targets.push(value);
                }
            }
        }

        Ok(Self {
            config,
            num_records,
            inputs,
            targets,
        })
    }

    /// Build a dataset directly from separate input and target buffers already
    /// in structure-of-arrays layout. Fails loud if either buffer length is
    /// not consistent with `config` and a common record count.
    pub fn from_soa(
        inputs: Vec<f32>,
        targets: Vec<f32>,
        config: TrainingDataConfig,
    ) -> Result<Self, DatasetError> {
        validate_config(&config)?;
        let by_inputs = record_count(inputs.len(), config.num_inputs)?;
        let by_targets = record_count(targets.len(), config.num_outputs)?;
        if by_inputs != by_targets {
            return Err(DatasetError::InvalidConfig {
                message: format!(
                    "input buffer implies {by_inputs} records but target buffer implies {by_targets}"
                ),
            });
        }
        Ok(Self {
            config,
            num_records: by_inputs,
            inputs,
            targets,
        })
    }

    /// Number of records held.
    pub fn num_records(&self) -> usize {
        self.num_records
    }

    /// Inputs per record.
    pub fn num_inputs(&self) -> usize {
        self.config.num_inputs
    }

    /// Outputs (targets) per record.
    pub fn num_outputs(&self) -> usize {
        self.config.num_outputs
    }

    /// Length of the owned input buffer in `f32` values.
    pub fn inputs_len(&self) -> usize {
        self.inputs.len()
    }

    /// Length of the owned target buffer in `f32` values.
    pub fn targets_len(&self) -> usize {
        self.targets.len()
    }

    /// Footprint of this dataset in neat-core's linear memory, in bytes.
    ///
    /// This is the quantity that must return to its pre-load baseline once the
    /// dataset is freed — the anchor for the leak/high-water-mark check.
    pub fn byte_len(&self) -> usize {
        (self.inputs.len() + self.targets.len()) * std::mem::size_of::<f32>()
    }

    /// Contiguous input slice for the `[start, start + count)` record batch.
    ///
    /// Reads straight from the owned buffer — no copy crosses the boundary.
    pub fn input_batch(&self, start: usize, count: usize) -> Result<&[f32], DatasetError> {
        let (lo, hi) = self.batch_bounds(start, count, self.num_inputs())?;
        Ok(&self.inputs[lo..hi])
    }

    /// Contiguous target slice for the `[start, start + count)` record batch.
    pub fn target_batch(&self, start: usize, count: usize) -> Result<&[f32], DatasetError> {
        let (lo, hi) = self.batch_bounds(start, count, self.num_outputs())?;
        Ok(&self.targets[lo..hi])
    }

    /// Inputs for a single record by index.
    pub fn record_inputs(&self, index: usize) -> Result<&[f32], DatasetError> {
        self.input_batch(index, 1)
    }

    /// Targets for a single record by index.
    pub fn record_outputs(&self, index: usize) -> Result<&[f32], DatasetError> {
        self.target_batch(index, 1)
    }

    /// Evaluate a network over the `[start, start + count)` record batch and
    /// return the mean squared error against the owned targets.
    ///
    /// The dataset is read **by index** from linear memory; only the network
    /// and the batch bounds are supplied per call — the training arrays never
    /// re-cross the boundary. Fails loud on an out-of-range batch or a
    /// network/dataset input-arity mismatch.
    ///
    /// The batch is bounds-checked **once** and then driven through the flat
    /// batched scoring path (Issue #386): the SoA input buffer is already in the
    /// layout [`CompiledNetwork::score_records_flat`] wants, so it is handed over
    /// as one slice — no per-record `Vec`, no per-record bounds check, and the
    /// full 8-record interleaved SIMD path (Issues #230 / #287) instead of the
    /// single-record `activate` kernel. Records are scored in fixed-size chunks
    /// (a multiple of the 8-record SIMD group, so grouping is unchanged) to keep
    /// the scratch output buffer bounded on the >4 GB Memory64 offload lane.
    ///
    /// Numerics: the batched path re-associates the `f32` weighted sums and uses
    /// the vectorised squash, so the result matches the per-record reference
    /// within the documented SIMD tolerance (see [`crate::batch_scoring`]) rather
    /// than bit-for-bit. The MSE itself still accumulates in `f64`.
    pub fn evaluate_mse(
        &self,
        network: &mut CompiledNetwork,
        start: usize,
        count: usize,
    ) -> Result<f32, DatasetError> {
        let num_inputs = self.num_inputs();
        let num_outputs = self.num_outputs();
        if network.num_inputs != num_inputs {
            return Err(DatasetError::ShapeMismatch {
                network_inputs: network.num_inputs,
                dataset_inputs: num_inputs,
            });
        }
        // Bounds-check the whole batch once, up front (fail loud before any
        // work), then read it as two contiguous slices.
        let inputs = self.input_batch(start, count)?;
        let targets = self.target_batch(start, count)?;

        if count == 0 {
            return Ok(0.0);
        }

        let mut scratch = BatchScratch::new(network.num_neurons);
        let mut outputs = vec![0.0f32; EVAL_CHUNK_RECORDS.min(count) * num_outputs];
        let mut squared_error_sum = 0.0f64;

        for (input_chunk, target_chunk) in inputs
            .chunks(EVAL_CHUNK_RECORDS * num_inputs)
            .zip(targets.chunks(EVAL_CHUNK_RECORDS * num_outputs))
        {
            let chunk_records = input_chunk.len() / num_inputs;
            let predicted = &mut outputs[..chunk_records * num_outputs];
            network.score_batch_into(
                &mut scratch,
                RecordBatch::flat(input_chunk, num_inputs),
                num_outputs,
                predicted,
            );
            for (predicted, expected) in predicted.iter().zip(target_chunk.iter()) {
                let diff = f64::from(*predicted) - f64::from(*expected);
                squared_error_sum += diff * diff;
            }
        }

        let terms = (count * num_outputs) as f64;
        Ok((squared_error_sum / terms) as f32)
    }

    /// Translate a record batch into buffer `[lo, hi)` bounds for a per-record
    /// stride, rejecting any batch that runs past the end.
    fn batch_bounds(
        &self,
        start: usize,
        count: usize,
        stride: usize,
    ) -> Result<(usize, usize), DatasetError> {
        let end = start.checked_add(count).filter(|&e| e <= self.num_records);
        match end {
            Some(end) => Ok((start * stride, end * stride)),
            None => Err(DatasetError::BatchOutOfRange {
                start,
                count,
                num_records: self.num_records,
            }),
        }
    }
}

/// Handle table owning every live [`TrainingDataset`] in linear memory.
///
/// This is the ownership seam agreed with NEAT-AI#3410: JS holds the returned
/// `u32` handle only, never the dataset bytes. Freed slots are reused so a
/// steady load → free cycle of equal-sized datasets keeps
/// [`peak_bytes`](Self::peak_bytes) flat — a growing peak means a leak.
#[derive(Debug, Default)]
pub struct DatasetRegistry {
    slots: Vec<Option<TrainingDataset>>,
    live_bytes: usize,
    peak_bytes: usize,
}

impl DatasetRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a packed dataset and return its handle.
    pub fn load_packed(
        &mut self,
        bytes: &[u8],
        config: TrainingDataConfig,
    ) -> Result<u32, DatasetError> {
        let dataset = TrainingDataset::from_packed_bytes(bytes, config)?;
        Ok(self.insert(dataset))
    }

    /// Insert an already-built dataset and return its handle, reusing a freed
    /// slot where one is available.
    pub fn insert(&mut self, dataset: TrainingDataset) -> u32 {
        self.live_bytes += dataset.byte_len();
        self.peak_bytes = self.peak_bytes.max(self.live_bytes);

        let free_slot = self.slots.iter().position(Option::is_none);
        match free_slot {
            Some(index) => {
                self.slots[index] = Some(dataset);
                index as u32
            }
            None => {
                self.slots.push(Some(dataset));
                (self.slots.len() - 1) as u32
            }
        }
    }

    /// Borrow the dataset behind a handle, failing loud if it is not live.
    pub fn get(&self, handle: u32) -> Result<&TrainingDataset, DatasetError> {
        self.slots
            .get(handle as usize)
            .and_then(Option::as_ref)
            .ok_or(DatasetError::UnknownHandle { handle })
    }

    /// Free the dataset behind a handle, releasing its bytes. Returns the freed
    /// dataset's footprint in bytes. Fails loud on an unknown handle so a
    /// double-free cannot be mistaken for success (Issue #3234).
    pub fn free(&mut self, handle: u32) -> Result<usize, DatasetError> {
        let slot = self
            .slots
            .get_mut(handle as usize)
            .filter(|slot| slot.is_some());
        match slot {
            Some(slot) => {
                let dataset = slot.take().expect("slot is Some by the filter above");
                let freed = dataset.byte_len();
                self.live_bytes -= freed;
                Ok(freed)
            }
            None => Err(DatasetError::UnknownHandle { handle }),
        }
    }

    /// Bytes currently held by live datasets.
    pub fn live_bytes(&self) -> usize {
        self.live_bytes
    }

    /// Highest [`live_bytes`](Self::live_bytes) observed over this registry's
    /// lifetime — the linear-memory high-water mark.
    pub fn peak_bytes(&self) -> usize {
        self.peak_bytes
    }

    /// Number of live datasets.
    pub fn live_count(&self) -> usize {
        self.slots.iter().filter(|slot| slot.is_some()).count()
    }
}

/// Reject a configuration with zero inputs or zero outputs.
fn validate_config(config: &TrainingDataConfig) -> Result<(), DatasetError> {
    if config.num_inputs == 0 {
        return Err(DatasetError::InvalidConfig {
            message: "num_inputs must be greater than zero".to_string(),
        });
    }
    if config.num_outputs == 0 {
        return Err(DatasetError::InvalidConfig {
            message: "num_outputs must be greater than zero".to_string(),
        });
    }
    Ok(())
}

/// Records implied by a buffer length and a per-record stride, failing loud on
/// a ragged buffer.
fn record_count(buffer_len: usize, stride: usize) -> Result<usize, DatasetError> {
    if stride == 0 || !buffer_len.is_multiple_of(stride) {
        return Err(DatasetError::UnalignedBuffer {
            buffer_len,
            record_size: stride * std::mem::size_of::<f32>(),
        });
    }
    Ok(buffer_len / stride)
}

// ---------------------------------------------------------------------------
// Tests — core dataset + registry lifecycle. Higher-level offload behaviour
// (evaluation-by-index, leak high-water mark) lives in
// `tests/wasm_dataset_offload.rs`.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Pack interleaved records (inputs then outputs each) into `.bin` bytes.
    fn pack(records: &[Vec<f32>]) -> Vec<u8> {
        records
            .iter()
            .flat_map(|r| r.iter().flat_map(|v| v.to_le_bytes()))
            .collect()
    }

    #[test]
    fn from_packed_bytes_deinterleaves_into_soa() {
        // Two records, 2 inputs + 1 output each.
        let bytes = pack(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
        let ds = TrainingDataset::from_packed_bytes(&bytes, TrainingDataConfig::new(2, 1)).unwrap();

        assert_eq!(ds.num_records(), 2);
        assert_eq!(ds.record_inputs(0).unwrap(), &[1.0, 2.0]);
        assert_eq!(ds.record_outputs(0).unwrap(), &[3.0]);
        assert_eq!(ds.record_inputs(1).unwrap(), &[4.0, 5.0]);
        assert_eq!(ds.record_outputs(1).unwrap(), &[6.0]);
    }

    #[test]
    fn from_packed_bytes_rejects_unaligned_buffer() {
        // 2 inputs + 1 output => 12 bytes per record; 10 bytes is ragged.
        let err = TrainingDataset::from_packed_bytes(&[0u8; 10], TrainingDataConfig::new(2, 1))
            .unwrap_err();
        assert_eq!(
            err,
            DatasetError::UnalignedBuffer {
                buffer_len: 10,
                record_size: 12,
            }
        );
    }

    #[test]
    fn from_packed_bytes_rejects_zero_arity_config() {
        let err =
            TrainingDataset::from_packed_bytes(&[], TrainingDataConfig::new(0, 1)).unwrap_err();
        assert!(matches!(err, DatasetError::InvalidConfig { .. }));
    }

    #[test]
    fn input_batch_returns_contiguous_slice() {
        let bytes = pack(&[
            vec![1.0, 2.0, 10.0],
            vec![3.0, 4.0, 20.0],
            vec![5.0, 6.0, 30.0],
        ]);
        let ds = TrainingDataset::from_packed_bytes(&bytes, TrainingDataConfig::new(2, 1)).unwrap();

        // Batch of the middle two records.
        assert_eq!(ds.input_batch(1, 2).unwrap(), &[3.0, 4.0, 5.0, 6.0]);
        assert_eq!(ds.target_batch(1, 2).unwrap(), &[20.0, 30.0]);
    }

    #[test]
    fn batch_out_of_range_fails_loud() {
        let bytes = pack(&[vec![1.0, 2.0], vec![3.0, 4.0]]);
        let ds = TrainingDataset::from_packed_bytes(&bytes, TrainingDataConfig::new(1, 1)).unwrap();
        let err = ds.input_batch(1, 5).unwrap_err();
        assert_eq!(
            err,
            DatasetError::BatchOutOfRange {
                start: 1,
                count: 5,
                num_records: 2,
            }
        );
    }

    #[test]
    fn byte_len_counts_inputs_and_targets() {
        let bytes = pack(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
        let ds = TrainingDataset::from_packed_bytes(&bytes, TrainingDataConfig::new(2, 1)).unwrap();
        // 2 records * (2 inputs + 1 target) * 4 bytes.
        assert_eq!(ds.byte_len(), 2 * 3 * 4);
    }

    #[test]
    fn from_soa_rejects_mismatched_record_counts() {
        // 2 input records but 1 target record.
        let err = TrainingDataset::from_soa(
            vec![1.0, 2.0, 3.0, 4.0],
            vec![9.0],
            TrainingDataConfig::new(2, 1),
        )
        .unwrap_err();
        assert!(matches!(err, DatasetError::InvalidConfig { .. }));
    }

    #[test]
    fn registry_free_releases_bytes_and_reuses_slot() {
        let mut registry = DatasetRegistry::new();
        let bytes = pack(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
        let config = TrainingDataConfig::new(2, 1);

        let handle = registry.load_packed(&bytes, config.clone()).unwrap();
        assert_eq!(registry.live_count(), 1);
        assert_eq!(registry.live_bytes(), 2 * 3 * 4);

        let freed = registry.free(handle).unwrap();
        assert_eq!(freed, 2 * 3 * 4);
        assert_eq!(registry.live_bytes(), 0);
        assert_eq!(registry.live_count(), 0);

        // A same-sized reload reuses the freed slot (same handle index).
        let handle2 = registry.load_packed(&bytes, config).unwrap();
        assert_eq!(handle2, handle);
        assert_eq!(registry.slots.len(), 1);
    }

    #[test]
    fn registry_get_and_free_unknown_handle_fail_loud() {
        let mut registry = DatasetRegistry::new();
        assert_eq!(
            registry.get(7).unwrap_err(),
            DatasetError::UnknownHandle { handle: 7 }
        );
        assert_eq!(
            registry.free(7).unwrap_err(),
            DatasetError::UnknownHandle { handle: 7 }
        );
    }

    #[test]
    fn registry_double_free_fails_loud() {
        let mut registry = DatasetRegistry::new();
        let bytes = pack(&[vec![1.0, 2.0]]);
        let handle = registry
            .load_packed(&bytes, TrainingDataConfig::new(1, 1))
            .unwrap();
        registry.free(handle).unwrap();
        // Second free must not silently succeed.
        assert_eq!(
            registry.free(handle).unwrap_err(),
            DatasetError::UnknownHandle { handle }
        );
    }
}
