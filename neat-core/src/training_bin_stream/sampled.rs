//! Record-sampled reads: fetch only the records a stride keeps.
//!
//! [`super::for_each_read_chunk`] reads **every** byte of the corpus. A caller
//! that scores a stratified subsample (NEAT-AI-scorer's `--sample-rate`) then
//! throws ~95 % of those bytes away after decoding them, so a 5 % call costs
//! very nearly what a full-corpus call costs — the fixed per-call cost measured
//! in NEAT-AI-Lamarck#112.
//!
//! [`for_each_sampled_read_chunk`] reads the kept records **only**, seeking past
//! the rest, and hands the caller chunks that contain nothing else. The caller's
//! decode and scoring shrink with the sample rate because the bytes never arrive
//! in the first place.
//!
//! ## Why it needs concurrent readers
//!
//! Skipping bytes trades sequential bandwidth for seeks, and a single-threaded
//! seek loop **loses** that trade: on the 21 GiB / 10 048 B-per-record
//! production corpus, one thread reading every 20th record took 9.8–31.8 s
//! against 5.0–6.2 s for a full sequential sweep. The same reads spread over 16
//! threads took 1.27 s — the device needs several requests in flight before
//! sparse reads beat streaming. So the sampled reader is a **pool** of readers
//! by construction, not an optional tuning.
//!
//! Order is preserved regardless of the reader count: segment `k` is read by
//! reader `k % readers` and the consumer pulls segments back in `k` order, so
//! `on_chunk` sees exactly the records a sequential sweep would have kept, in
//! the same order. A caller accumulating a float sum therefore gets a
//! bit-identical result whatever the pool size.
//!
//! ## When it is worth it
//!
//! [`sampled_read_is_worthwhile`] answers that: sparse enough that most bytes
//! are skipped, and with skips long enough to be worth a seek. Callers above
//! that density keep the sequential sweep.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

/// Densest sample a sampled read is offered for (25 % of records).
///
/// Above this the skipped runs are short, the read is nearly sequential anyway,
/// and the seeks cost more than the bytes they save.
pub const SAMPLED_READ_MAX_KEPT_FRACTION: f64 = 0.25;

/// Shortest mean skip worth a seek (64 KiB).
///
/// Modelled from the production corpus measurement: a sparse read costs about
/// 11 µs of pool throughput where 64 KiB of sequential read costs about 16 µs,
/// so skipping less than this buys nothing.
pub const SAMPLED_READ_MIN_SKIP_BYTES: usize = 64 * 1024;

/// True when reading `kept_fraction` of `record_bytes`-sized records is cheaper
/// than streaming the whole corpus and discarding the rest.
///
/// Both halves matter: a dense sample skips almost nothing, and a sample of
/// tiny records leaves skips too short to seek over.
///
/// # Examples
///
/// ```
/// use neat_core::training_bin_stream::sampled_read_is_worthwhile;
///
/// // Production shape: 5 % of 10 048-byte records — skips ~191 KiB at a time.
/// assert!(sampled_read_is_worthwhile(10_048, 0.05));
/// // Too dense: half the records are kept, so nothing is really skipped.
/// assert!(!sampled_read_is_worthwhile(10_048, 0.5));
/// // Records too small: 5 % of 64-byte records skips ~1.2 KiB — not worth a seek.
/// assert!(!sampled_read_is_worthwhile(64, 0.05));
/// // A full-rate (or nonsensical) fraction never takes the sampled path.
/// assert!(!sampled_read_is_worthwhile(10_048, 1.0));
/// ```
pub fn sampled_read_is_worthwhile(record_bytes: usize, kept_fraction: f64) -> bool {
    if record_bytes == 0 || !kept_fraction.is_finite() {
        return false;
    }
    if kept_fraction <= 0.0 || kept_fraction > SAMPLED_READ_MAX_KEPT_FRACTION {
        return false;
    }
    let mean_skip = record_bytes as f64 * (1.0 / kept_fraction - 1.0);
    mean_skip >= SAMPLED_READ_MIN_SKIP_BYTES as f64
}

/// One contiguous window of records inside one file, read by one reader.
///
/// Segments tile the corpus in global record order; `index` is that order and
/// is what the consumer reassembles by.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Segment {
    index: usize,
    file_index: usize,
    /// First record of the window, relative to the start of its file.
    first_record_in_file: u64,
    records: u64,
    /// Global index of `first_record_in_file`, across all files in order.
    global_first_record: u64,
}

/// Tile `record_counts` into windows of at most `records_per_segment` records,
/// in global record order.
fn plan_segments(record_counts: &[u64], records_per_segment: u64) -> Vec<Segment> {
    debug_assert!(records_per_segment > 0);
    let mut segments = Vec::new();
    let mut global = 0_u64;
    for (file_index, &records) in record_counts.iter().enumerate() {
        let mut offset = 0_u64;
        while offset < records {
            let len = records_per_segment.min(records - offset);
            segments.push(Segment {
                index: segments.len(),
                file_index,
                first_record_in_file: offset,
                records: len,
                global_first_record: global + offset,
            });
            offset += len;
        }
        global += records;
    }
    segments
}

/// Byte ranges covering the kept records of `segment`, consecutive kept records
/// coalesced into one range.
///
/// Returns `(offset_in_file, length_in_bytes)` pairs in ascending order. The
/// summed length is exactly `kept records × record_bytes` — the bytes the
/// reader is about to fetch, and no others.
fn kept_runs(
    segment: &Segment,
    record_bytes: u64,
    keep: &(dyn Fn(u64) -> bool + Sync),
) -> Vec<(u64, usize)> {
    let mut runs: Vec<(u64, usize)> = Vec::new();
    let mut run_start: Option<u64> = None;
    let mut run_len = 0_u64;
    for r in 0..segment.records {
        if keep(segment.global_first_record + r) {
            let record_in_file = segment.first_record_in_file + r;
            match run_start {
                Some(_) => run_len += 1,
                None => {
                    run_start = Some(record_in_file);
                    run_len = 1;
                }
            }
        } else if let Some(start) = run_start.take() {
            runs.push((start * record_bytes, (run_len * record_bytes) as usize));
            run_len = 0;
        }
    }
    if let Some(start) = run_start {
        runs.push((start * record_bytes, (run_len * record_bytes) as usize));
    }
    runs
}

/// Read one segment's kept records into a fresh buffer.
///
/// `open` is threaded through so a reader can keep the file it is already on
/// open across consecutive segments instead of reopening per window.
fn read_segment(
    file: &mut File,
    segment: &Segment,
    record_bytes: u64,
    keep: &(dyn Fn(u64) -> bool + Sync),
    path: &Path,
) -> Result<Vec<u8>, String> {
    let runs = kept_runs(segment, record_bytes, keep);
    let total: usize = runs.iter().map(|(_, len)| *len).sum();
    let mut out = vec![0_u8; total];
    let mut written = 0_usize;
    for (offset, len) in runs {
        file.seek(SeekFrom::Start(offset)).map_err(|e| {
            format!(
                "Failed seeking to byte {offset} of training file '{}': {e}",
                path.display()
            )
        })?;
        file.read_exact(&mut out[written..written + len])
            .map_err(|e| {
                format!(
                    "Failed reading {len} bytes at byte {offset} of training file '{}': {e}",
                    path.display()
                )
            })?;
        written += len;
    }
    Ok(out)
}

/// Record counts per file, rejecting any file that is not whole records.
fn record_counts(bin_files: &[PathBuf], record_bytes: u64) -> Result<Vec<u64>, String> {
    let mut counts = Vec::with_capacity(bin_files.len());
    for (idx, path) in bin_files.iter().enumerate() {
        let len = std::fs::metadata(path)
            .map_err(|e| {
                format!(
                    "Failed to stat training file #{idx} '{}': {e}",
                    path.display()
                )
            })?
            .len();
        if len % record_bytes != 0 {
            return Err(format!(
                "Training file #{idx} '{}' is {len} bytes, not a whole number of {record_bytes}-byte records",
                path.display()
            ));
        }
        counts.push(len / record_bytes);
    }
    Ok(counts)
}

/// Stream **only** the records `keep` selects, in global record order.
///
/// `on_chunk` is handed whole records back to back — never a partial record and
/// never an unkept one — in chunks of at most `read_buf_len` bytes' worth of
/// kept records. A caller that already filters by the same predicate can
/// therefore switch its own filter off; nothing else about its accumulation
/// changes, and the delivered byte stream is identical for every `readers`
/// value.
///
/// `keep` takes a **global** record index — 0-based across `bin_files` in the
/// order given — so the kept set never depends on how the corpus is split into
/// files, segments or chunks.
///
/// `readers` is the size of the reader pool; sparse reads need several requests
/// in flight to beat a sequential sweep (see the module docs), so `1` is
/// honoured but slow. Values are clamped to at least 1.
///
/// # Errors
///
/// Fails loud rather than skipping data: a file that cannot be stated, opened or
/// read, a file whose length is not a whole number of records, a
/// `record_bytes`/`read_buf_len` of 0, or a reader thread that panicked. An
/// error from `on_chunk` stops the sweep and propagates.
pub fn for_each_sampled_read_chunk<F>(
    bin_files: &[PathBuf],
    read_buf_len: usize,
    record_bytes: usize,
    readers: usize,
    keep: &(dyn Fn(u64) -> bool + Sync),
    mut on_chunk: F,
) -> Result<(), String>
where
    F: FnMut(&[u8]) -> Result<(), String>,
{
    if record_bytes == 0 {
        return Err("record_bytes must be positive".to_string());
    }
    if read_buf_len == 0 {
        return Err("read_buf_len must be positive".to_string());
    }
    let record_bytes_u64 = record_bytes as u64;
    let counts = record_counts(bin_files, record_bytes_u64)?;
    let records_per_segment = (read_buf_len / record_bytes).max(1) as u64;
    let segments = plan_segments(&counts, records_per_segment);
    if segments.is_empty() {
        return Ok(());
    }
    let readers = readers.max(1).min(segments.len());

    // Reader `t` takes segments t, t + readers, t + 2·readers, … and the
    // consumer pulls them back in that same interleaving, so segments arrive in
    // global order with no reorder buffer and at most one segment queued per
    // reader.
    let stop = AtomicBool::new(false);
    let mut receivers = Vec::with_capacity(readers);
    let mut result: Result<(), String> = Ok(());

    std::thread::scope(|scope| {
        for reader in 0..readers {
            let (tx, rx) = mpsc::sync_channel::<Result<Vec<u8>, String>>(1);
            receivers.push(rx);
            let segments = &segments;
            let stop = &stop;
            scope.spawn(move || {
                let mut open: Option<(usize, File)> = None;
                for segment in segments.iter().skip(reader).step_by(readers) {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let path = &bin_files[segment.file_index];
                    if open
                        .as_ref()
                        .is_none_or(|(idx, _)| *idx != segment.file_index)
                    {
                        match File::open(path) {
                            Ok(file) => open = Some((segment.file_index, file)),
                            Err(e) => {
                                let _ = tx.send(Err(format!(
                                    "Failed to open training file #{} '{}': {e}",
                                    segment.file_index,
                                    path.display()
                                )));
                                return;
                            }
                        }
                    }
                    let (_, file) = open.as_mut().expect("opened above");
                    let read = read_segment(file, segment, record_bytes_u64, keep, path);
                    let failed = read.is_err();
                    if tx.send(read).is_err() || failed {
                        return;
                    }
                }
            });
        }

        for segment in &segments {
            match receivers[segment.index % readers].recv() {
                Ok(Ok(bytes)) => {
                    if bytes.is_empty() {
                        continue;
                    }
                    if let Err(e) = on_chunk(&bytes) {
                        result = Err(format!(
                            "on_chunk failed at file #{} (n={} bytes): {e}",
                            segment.file_index,
                            bytes.len()
                        ));
                        break;
                    }
                }
                Ok(Err(e)) => {
                    result = Err(e);
                    break;
                }
                Err(_) => {
                    result = Err(format!(
                        "sampled reader #{} disconnected before segment {} of file #{}",
                        segment.index % readers,
                        segment.index,
                        segment.file_index
                    ));
                    break;
                }
            }
        }

        // Release any reader still blocked on a full channel, then drain so the
        // scope can join them.
        stop.store(true, Ordering::Relaxed);
        for rx in &receivers {
            while rx.try_recv().is_ok() {}
        }
        drop(receivers);
    });

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const RECORD_BYTES: usize = 8;

    /// A corpus whose every record is `[file_marker, global_index]` as two
    /// little-endian `u32`s, so a delivered chunk identifies exactly which
    /// records it carries.
    fn write_corpus(dir: &std::path::Path, records_per_file: &[u64]) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut global = 0_u32;
        for (i, &records) in records_per_file.iter().enumerate() {
            let mut bytes = Vec::new();
            for _ in 0..records {
                bytes.extend_from_slice(&(i as u32).to_le_bytes());
                bytes.extend_from_slice(&global.to_le_bytes());
                global += 1;
            }
            let path = dir.join(format!("{i}.bin"));
            fs::write(&path, &bytes).unwrap();
            files.push(path);
        }
        files
    }

    /// Global indices carried by a run of delivered chunks.
    fn indices(chunks: &[Vec<u8>]) -> Vec<u32> {
        chunks
            .iter()
            .flat_map(|c| {
                c.chunks_exact(RECORD_BYTES)
                    .map(|r| u32::from_le_bytes([r[4], r[5], r[6], r[7]]))
            })
            .collect()
    }

    fn stride(rate: f64, phase: u64) -> impl Fn(u64) -> bool + Sync {
        move |i: u64| {
            let j = (i + phase) as f64;
            ((j + 1.0) * rate).floor() > (j * rate).floor()
        }
    }

    fn collect(
        files: &[PathBuf],
        read_buf_len: usize,
        readers: usize,
        keep: &(dyn Fn(u64) -> bool + Sync),
    ) -> Result<Vec<Vec<u8>>, String> {
        let mut chunks = Vec::new();
        for_each_sampled_read_chunk(files, read_buf_len, RECORD_BYTES, readers, keep, |c| {
            chunks.push(c.to_vec());
            Ok(())
        })?;
        Ok(chunks)
    }

    /// The whole contract: the delivered records are exactly the ones a
    /// sequential sweep would have kept, in the same order — for every rate,
    /// phase, chunk size and pool size.
    #[test]
    fn delivers_exactly_the_kept_records_in_order() {
        let dir = TempDir::new().unwrap();
        let files = write_corpus(dir.path(), &[10, 1, 37, 52]);
        let total = 10 + 1 + 37 + 52_u64;
        for (rate, phase) in [(0.05, 0), (0.1, 3), (0.25, 0), (0.5, 1), (1.0, 0)] {
            let keep = stride(rate, phase);
            let expected: Vec<u32> = (0..total).filter(|&i| keep(i)).map(|i| i as u32).collect();
            for readers in [1_usize, 2, 3, 8, 64] {
                for read_buf_len in [RECORD_BYTES, RECORD_BYTES * 7, 1024, 1 << 20] {
                    let chunks = collect(&files, read_buf_len, readers, &keep).unwrap();
                    assert_eq!(
                        indices(&chunks),
                        expected,
                        "rate {rate} phase {phase} readers {readers} buf {read_buf_len}"
                    );
                    assert!(
                        chunks.iter().all(|c| !c.is_empty()),
                        "an empty chunk must never be delivered"
                    );
                }
            }
        }
    }

    /// Chunks are whole records and no larger than the caller's read buffer.
    #[test]
    fn chunks_are_whole_records_within_the_read_buffer() {
        let dir = TempDir::new().unwrap();
        let files = write_corpus(dir.path(), &[500]);
        let read_buf_len = RECORD_BYTES * 20;
        let chunks = collect(&files, read_buf_len, 4, &stride(0.5, 0)).unwrap();
        assert!(!chunks.is_empty());
        for chunk in &chunks {
            assert_eq!(chunk.len() % RECORD_BYTES, 0, "partial record delivered");
            assert!(
                chunk.len() <= read_buf_len,
                "chunk exceeded the read buffer"
            );
        }
    }

    /// A sampled read fetches the kept bytes and nothing else — the saving is
    /// the bytes never asked for.
    #[test]
    fn plans_reads_covering_only_the_kept_bytes() {
        let segment = Segment {
            index: 0,
            file_index: 0,
            first_record_in_file: 100,
            records: 40,
            global_first_record: 100,
        };
        let keep = stride(0.05, 0);
        let runs = kept_runs(&segment, RECORD_BYTES as u64, &keep);
        let kept = (100..140_u64).filter(|&i| keep(i)).count();
        assert_eq!(runs.len(), kept, "sparse records must not be coalesced");
        let bytes: usize = runs.iter().map(|(_, len)| *len).sum();
        assert_eq!(bytes, kept * RECORD_BYTES);
        // Every planned read lies inside the segment's own byte range.
        for (offset, len) in &runs {
            assert!(*offset >= 100 * RECORD_BYTES as u64);
            assert!(*offset + *len as u64 <= 140 * RECORD_BYTES as u64);
        }
    }

    /// Consecutive kept records are fetched as one read rather than one each.
    #[test]
    fn coalesces_consecutive_kept_records() {
        let segment = Segment {
            index: 0,
            file_index: 0,
            first_record_in_file: 0,
            records: 8,
            global_first_record: 0,
        };
        let runs = kept_runs(&segment, RECORD_BYTES as u64, &|_| true);
        assert_eq!(runs, vec![(0, 8 * RECORD_BYTES)]);
    }

    #[test]
    fn segments_tile_every_record_in_order() {
        let segments = plan_segments(&[5, 0, 3], 2);
        assert_eq!(
            segments
                .iter()
                .map(|s| (
                    s.file_index,
                    s.first_record_in_file,
                    s.records,
                    s.global_first_record
                ))
                .collect::<Vec<_>>(),
            vec![
                (0, 0, 2, 0),
                (0, 2, 2, 2),
                (0, 4, 1, 4),
                (2, 0, 2, 5),
                (2, 2, 1, 7)
            ]
        );
        assert_eq!(
            segments.iter().map(|s| s.index).collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4]
        );
    }

    /// A corpus that is not a whole number of records is a fault, not a
    /// silently truncated read.
    #[test]
    fn a_ragged_file_fails_loudly() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ragged.bin");
        fs::write(&path, vec![0_u8; RECORD_BYTES * 3 + 1]).unwrap();
        let err = collect(&[path], 1024, 2, &stride(0.5, 0)).unwrap_err();
        assert!(err.contains("whole number"), "unhelpful error: {err}");
    }

    #[test]
    fn a_missing_file_fails_loudly() {
        let dir = TempDir::new().unwrap();
        let err = collect(&[dir.path().join("absent.bin")], 1024, 2, &stride(0.5, 0)).unwrap_err();
        assert!(err.contains("absent.bin"), "unhelpful error: {err}");
    }

    #[test]
    fn zero_record_bytes_fails_loudly() {
        let err = for_each_sampled_read_chunk(&[], 1024, 0, 1, &|_| true, |_| Ok(())).unwrap_err();
        assert!(err.contains("record_bytes"), "unhelpful error: {err}");
        let err = for_each_sampled_read_chunk(&[], 0, 8, 1, &|_| true, |_| Ok(())).unwrap_err();
        assert!(err.contains("read_buf_len"), "unhelpful error: {err}");
    }

    /// A consumer that stops early (the scorer's abort path) stops the sweep and
    /// its error reaches the caller — readers do not keep the process alive.
    #[test]
    fn an_on_chunk_error_stops_the_sweep_and_propagates() {
        let dir = TempDir::new().unwrap();
        let files = write_corpus(dir.path(), &[400, 400]);
        let mut seen = 0_usize;
        let err = for_each_sampled_read_chunk(
            &files,
            RECORD_BYTES * 4,
            RECORD_BYTES,
            4,
            &stride(0.25, 0),
            |_| {
                seen += 1;
                if seen == 2 {
                    Err("stop here".to_string())
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
        assert!(err.contains("stop here"), "unhelpful error: {err}");
        assert_eq!(seen, 2, "the sweep must stop at the failing chunk");
    }

    #[test]
    fn an_empty_corpus_is_not_an_error() {
        let chunks = collect(&[], 1024, 4, &stride(0.5, 0)).unwrap();
        assert!(chunks.is_empty());
    }

    /// A rate that keeps nothing in a window delivers no chunk for it, and a
    /// predicate that keeps nothing at all delivers none.
    #[test]
    fn a_predicate_that_keeps_nothing_delivers_nothing() {
        let dir = TempDir::new().unwrap();
        let files = write_corpus(dir.path(), &[64]);
        let chunks = collect(&files, 1024, 4, &|_| false).unwrap();
        assert!(chunks.is_empty());
    }

    #[test]
    fn worthwhile_needs_both_sparsity_and_a_long_enough_skip() {
        // Production shape.
        assert!(sampled_read_is_worthwhile(10_048, 0.05));
        assert!(sampled_read_is_worthwhile(9_848, 0.05));
        // Dense samples keep the sequential sweep.
        assert!(!sampled_read_is_worthwhile(10_048, 0.26));
        assert!(!sampled_read_is_worthwhile(10_048, 1.0));
        // Small records leave skips too short to seek over.
        assert!(!sampled_read_is_worthwhile(64, 0.05));
        assert!(!sampled_read_is_worthwhile(1_024, 0.25));
        // Degenerate inputs never take the sampled path.
        assert!(!sampled_read_is_worthwhile(0, 0.05));
        assert!(!sampled_read_is_worthwhile(10_048, 0.0));
        assert!(!sampled_read_is_worthwhile(10_048, -0.5));
        assert!(!sampled_read_is_worthwhile(10_048, f64::NAN));
        // The boundary: 64 KiB of skip at the densest offered fraction.
        let boundary = SAMPLED_READ_MIN_SKIP_BYTES.div_ceil(3); // 25 % keeps 1 in 4 → skip = 3 records
        assert!(sampled_read_is_worthwhile(
            boundary,
            SAMPLED_READ_MAX_KEPT_FRACTION
        ));
        assert!(!sampled_read_is_worthwhile(
            boundary - 1,
            SAMPLED_READ_MAX_KEPT_FRACTION
        ));
    }
}
