//! One API to stream packed `.bin` training bytes in large chunks.
//!
//! [`for_each_read_chunk`] is the **only** scan entry point used for production-sized
//! forward passes: callers append each chunk to a staging buffer and unpack records.
//!
//! - **Native (`not(wasm32)`):** pipelined double-buffer reads (reader thread + ping-pong
//!   `Vec<u8>`) so the main thread can overlap disk I/O with `f32` unpack + network work.
//! - **`wasm32-unknown-unknown`:** sequential `File::read` into one buffer (no threads,
//!   no mmap). Same callback contract as native — one mental model, target-specific
//!   implementation only.
//!
//! ## Sequential escape hatch (issue #25)
//!
//! On native hosts, setting the environment variable `NEAT_TRAINING_READ_SEQUENTIAL=1`
//! (accepted values: `1`, `true`, `yes`, `on`, case-insensitive) selects the same
//! sequential `File::read` loop used on `wasm32`. This avoids the pipelined reader
//! entirely — handy for downstream consumers who hit teardown races while the
//! pipelined path is under investigation.

use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

/// Environment variable that, when set to a truthy value, forces the sequential
/// single-threaded read path even on native targets.
pub const SEQUENTIAL_ENV: &str = "NEAT_TRAINING_READ_SEQUENTIAL";

/// Read every byte from `bin_files` in order, invoking `on_chunk` for each read segment.
///
/// `read_buf_len` should be at least one record (`>= record_bytes`); callers typically
/// use ~2 MiB rounded down to a whole number of records.
///
/// On native targets, reads are pipelined with a background thread unless the
/// [`SEQUENTIAL_ENV`] env var forces the sequential path. On `wasm32`, reads
/// are always sequential and as large as `read_buf_len` allows — still chunked
/// so you never load an entire shard into memory at once.
pub fn for_each_read_chunk<F>(
    bin_files: &[PathBuf],
    read_buf_len: usize,
    on_chunk: F,
) -> Result<(), String>
where
    F: FnMut(&[u8]) -> Result<(), String>,
{
    if read_buf_len == 0 {
        return Err("read_buf_len must be positive".to_string());
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        if sequential_env_requested() {
            return for_each_read_chunk_sequential(bin_files, read_buf_len, on_chunk);
        }
        for_each_read_chunk_native(bin_files, read_buf_len, on_chunk)
    }

    #[cfg(target_arch = "wasm32")]
    {
        for_each_read_chunk_sequential(bin_files, read_buf_len, on_chunk)
    }
}

/// Returns true if the sequential escape hatch is selected via env var.
#[cfg(not(target_arch = "wasm32"))]
fn sequential_env_requested() -> bool {
    match std::env::var(SEQUENTIAL_ENV) {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            matches!(v.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => false,
    }
}

/// Sequential read loop — shared between the wasm32 target and the native
/// escape hatch. Same observable contract as the pipelined path.
fn for_each_read_chunk_sequential<F>(
    bin_files: &[PathBuf],
    read_buf_len: usize,
    mut on_chunk: F,
) -> Result<(), String>
where
    F: FnMut(&[u8]) -> Result<(), String>,
{
    let mut read_buf = vec![0u8; read_buf_len];
    for (idx, path) in bin_files.iter().enumerate() {
        let mut file = File::open(path).map_err(|e| {
            format!(
                "Failed to open training file #{idx} '{}': {e}",
                path.display()
            )
        })?;
        loop {
            let n = file.read(&mut read_buf).map_err(|e| {
                format!(
                    "Failed reading training file #{idx} '{}': {e}",
                    path.display()
                )
            })?;
            if n == 0 {
                break;
            }
            on_chunk(&read_buf[..n])
                .map_err(|e| format!("on_chunk failed at file #{idx} (n={n} bytes): {e}"))?;
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn for_each_read_chunk_native<F>(
    bin_files: &[PathBuf],
    read_buf_len: usize,
    mut on_chunk: F,
) -> Result<(), String>
where
    F: FnMut(&[u8]) -> Result<(), String>,
{
    use std::sync::mpsc;
    use std::thread;

    enum ReaderMsg {
        Chunk {
            buf: Vec<u8>,
            n: usize,
            file_idx: usize,
        },
        Done,
    }

    let (fill_tx, fill_rx) = mpsc::sync_channel::<ReaderMsg>(1);
    let (empty_tx, empty_rx) = mpsc::sync_channel::<Vec<u8>>(2);
    // Shutdown gate (issue #25): the reader parks here AFTER sending `Done` so
    // it keeps `empty_rx` alive until the consumer has finished returning every
    // buffer. Without this gate, a slow `on_chunk` (e.g. Rayon fused MSE) could
    // finish processing the final chunk AFTER the reader had already exited and
    // dropped `empty_rx`, so the consumer's final `empty_tx.send` failed with
    // "failed to return read buffer to pool".
    let (shutdown_tx, shutdown_rx) = mpsc::sync_channel::<()>(1);

    empty_tx
        .send(vec![0u8; read_buf_len])
        .map_err(|_| "failed to seed read buffer (consumer dropped?)".to_string())?;
    empty_tx
        .send(vec![0u8; read_buf_len])
        .map_err(|_| "failed to seed read buffer (consumer dropped?)".to_string())?;

    // The reader keeps its own sender into the pool so it can return unused
    // buffers (e.g. after an EOF read) without shrinking the pool and
    // deadlocking on the next `empty_rx.recv()`.
    let empty_tx_reader = empty_tx.clone();
    let paths: Vec<PathBuf> = bin_files.to_vec();
    let reader_handle = thread::spawn(move || -> Result<(), String> {
        for (idx, path) in paths.iter().enumerate() {
            let mut file = File::open(path).map_err(|e| {
                format!(
                    "Failed to open training file #{idx} '{}': {e}",
                    path.display()
                )
            })?;
            loop {
                let mut buf = match empty_rx.recv() {
                    Ok(b) => b,
                    // Consumer shut down; nothing more to do.
                    Err(_) => return Ok(()),
                };
                let n = file.read(&mut buf).map_err(|e| {
                    format!(
                        "Failed reading training file #{idx} '{}': {e}",
                        path.display()
                    )
                })?;
                if n == 0 {
                    // EOF — do not forward an empty chunk (the consumer never
                    // needs to see it and forwarding was part of the original
                    // teardown race). Return `buf` to the pool so the reader
                    // does not shrink the buffer pool on every file boundary.
                    if empty_tx_reader.send(buf).is_err() {
                        return Ok(());
                    }
                    break;
                }
                if fill_tx
                    .send(ReaderMsg::Chunk {
                        buf,
                        n,
                        file_idx: idx,
                    })
                    .is_err()
                {
                    return Ok(());
                }
            }
        }
        // All files drained. Tell the consumer and park on the shutdown gate
        // so `empty_rx` stays alive until the consumer has returned its last
        // buffer (closes the teardown race).
        let _ = fill_tx.send(ReaderMsg::Done);
        let _ = shutdown_rx.recv();
        Ok(())
    });

    let consumer_result: Result<(), String> = (|| {
        loop {
            let msg = fill_rx
                .recv()
                .map_err(|_| "reader disconnected".to_string())?;
            match msg {
                ReaderMsg::Done => return Ok(()),
                ReaderMsg::Chunk { buf, n, file_idx } => {
                    debug_assert!(n > 0, "reader must not forward empty chunks");
                    if let Err(e) = on_chunk(&buf[..n]) {
                        // Best-effort return so the reader can shut down cleanly.
                        let _ = empty_tx.send(buf);
                        return Err(format!(
                            "on_chunk failed at file #{file_idx} (n={n} bytes): {e}"
                        ));
                    }
                    empty_tx.send(buf).map_err(|_| {
                        format!(
                            "failed to return read buffer to pool after file #{file_idx} (n={n} bytes)"
                        )
                    })?;
                }
            }
        }
    })();

    // Release the reader's shutdown gate and drop our ends of the channels.
    let _ = shutdown_tx.send(());
    drop(empty_tx);
    drop(fill_rx);

    match reader_handle.join() {
        Ok(Ok(())) => consumer_result,
        Ok(Err(reader_err)) => match consumer_result {
            Ok(()) => Err(reader_err),
            Err(consumer_err) => Err(format!(
                "{reader_err}; consumer also failed: {consumer_err}"
            )),
        },
        Err(_) => Err("reader thread panicked".to_string()),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    // Serialises tests that mutate process-wide env vars.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn write_bins(dir: &std::path::Path, shards: &[Vec<u8>]) -> Vec<PathBuf> {
        let mut files = Vec::with_capacity(shards.len());
        for (i, bytes) in shards.iter().enumerate() {
            let p = dir.join(format!("{i}.bin"));
            fs::write(&p, bytes).unwrap();
            files.push(p);
        }
        files
    }

    #[test]
    fn for_each_read_chunk_concatenates_files() -> Result<(), String> {
        let dir = TempDir::new().map_err(|e| e.to_string())?;
        let files = write_bins(dir.path(), &[vec![1u8, 2, 3, 4], vec![5u8, 6]]);
        let mut acc = Vec::new();
        for_each_read_chunk(&files, 3, |c| {
            acc.extend_from_slice(c);
            Ok(())
        })?;
        assert_eq!(acc, vec![1, 2, 3, 4, 5, 6]);
        Ok(())
    }

    /// Regression for issue #25: a slow `on_chunk` must NOT cause the pipelined
    /// reader to tear down and drop `empty_rx` before the consumer has returned
    /// its last buffer. Previously this surfaced as
    /// "failed to return read buffer to pool".
    #[test]
    fn slow_on_chunk_does_not_race_reader_teardown() -> Result<(), String> {
        let dir = TempDir::new().map_err(|e| e.to_string())?;
        // Multiple files so the reader iterates its outer loop; last chunks of
        // the last file are where the teardown race surfaced.
        let shards: Vec<Vec<u8>> = (0..4).map(|i| vec![i as u8; 1024]).collect();
        let files = write_bins(dir.path(), &shards);

        let mut total = 0usize;
        for_each_read_chunk(&files, 256, |c| {
            // Simulate heavy downstream work so the reader completes well
            // before we return the final buffer.
            thread::sleep(Duration::from_millis(20));
            total += c.len();
            Ok(())
        })?;
        assert_eq!(total, 4 * 1024);
        Ok(())
    }

    #[test]
    fn stress_many_small_files_pipelined() -> Result<(), String> {
        let dir = TempDir::new().map_err(|e| e.to_string())?;
        let shards: Vec<Vec<u8>> = (0..32)
            .map(|i| {
                // Varying sizes including a one-byte file to stress n==0 EOF handling.
                let size = if i == 7 { 1 } else { 37 * (i + 1) };
                vec![(i & 0xff) as u8; size]
            })
            .collect();
        let expected_total: usize = shards.iter().map(|s| s.len()).sum();
        let files = write_bins(dir.path(), &shards);

        let mut total = 0usize;
        for_each_read_chunk(&files, 128, |c| {
            total += c.len();
            Ok(())
        })?;
        assert_eq!(total, expected_total);
        Ok(())
    }

    /// `on_chunk` error must propagate and include the file index + chunk size
    /// for better diagnostics (issue #25 request).
    #[test]
    fn on_chunk_error_includes_file_index_diagnostic() {
        let dir = TempDir::new().unwrap();
        let files = write_bins(dir.path(), &[vec![0u8; 16], vec![1u8; 16], vec![2u8; 16]]);

        // Fail on the second file's chunk.
        let mut seen_first_file = false;
        let err = for_each_read_chunk(&files, 16, |c| {
            if c[0] == 0 {
                seen_first_file = true;
                Ok(())
            } else {
                Err("boom".to_string())
            }
        })
        .expect_err("on_chunk error must propagate");
        assert!(seen_first_file, "first file must have processed first");
        assert!(
            err.contains("file #1") || err.contains("#1"),
            "expected diagnostic to include file index, got: {err}"
        );
        assert!(
            err.contains("boom"),
            "expected original error message, got: {err}"
        );
    }

    #[test]
    fn missing_file_error_includes_file_index() {
        let dir = TempDir::new().unwrap();
        let good = dir.path().join("good.bin");
        fs::write(&good, [1u8, 2, 3]).unwrap();
        let missing = dir.path().join("does-not-exist.bin");
        let err = for_each_read_chunk(&[good, missing], 8, |_| Ok(())).expect_err("expected error");
        assert!(
            err.contains("file #1"),
            "expected file index in error, got: {err}"
        );
    }

    #[test]
    fn zero_read_buf_len_is_rejected() {
        let err = for_each_read_chunk(&[], 0, |_| Ok(())).expect_err("expected error");
        assert!(err.contains("read_buf_len"));
    }

    #[test]
    fn sequential_env_var_selects_sequential_path() -> Result<(), String> {
        let _guard = ENV_LOCK.lock().unwrap();
        // SAFETY: serialised via ENV_LOCK for all env-mutating tests in this module.
        unsafe { std::env::set_var(SEQUENTIAL_ENV, "1") };

        let dir = TempDir::new().map_err(|e| e.to_string())?;
        let files = write_bins(dir.path(), &[vec![9u8; 100], vec![8u8; 50]]);
        let mut total = 0usize;
        let result = for_each_read_chunk(&files, 16, |c| {
            total += c.len();
            Ok(())
        });

        // SAFETY: serialised via ENV_LOCK.
        unsafe { std::env::remove_var(SEQUENTIAL_ENV) };

        result?;
        assert_eq!(total, 150);
        Ok(())
    }

    #[test]
    fn sequential_env_var_accepts_truthy_variants() -> Result<(), String> {
        for val in ["1", "true", "TRUE", "Yes", "on"] {
            let _guard = ENV_LOCK.lock().unwrap();
            // SAFETY: serialised via ENV_LOCK.
            unsafe { std::env::set_var(SEQUENTIAL_ENV, val) };

            let dir = TempDir::new().map_err(|e| e.to_string())?;
            let files = write_bins(dir.path(), &[vec![1u8; 8]]);
            let mut total = 0usize;
            let result = for_each_read_chunk(&files, 4, |c| {
                total += c.len();
                Ok(())
            });

            // SAFETY: serialised via ENV_LOCK.
            unsafe { std::env::remove_var(SEQUENTIAL_ENV) };

            result?;
            assert_eq!(total, 8, "variant {val} should select sequential path");
        }
        Ok(())
    }

    #[test]
    fn sequential_env_var_ignored_when_falsy() -> Result<(), String> {
        for val in ["0", "false", "no", "off", ""] {
            let _guard = ENV_LOCK.lock().unwrap();
            // SAFETY: serialised via ENV_LOCK.
            unsafe { std::env::set_var(SEQUENTIAL_ENV, val) };

            let dir = TempDir::new().map_err(|e| e.to_string())?;
            let files = write_bins(dir.path(), &[vec![1u8; 8]]);
            let mut total = 0usize;
            let result = for_each_read_chunk(&files, 4, |c| {
                total += c.len();
                Ok(())
            });

            // SAFETY: serialised via ENV_LOCK.
            unsafe { std::env::remove_var(SEQUENTIAL_ENV) };

            result?;
            assert_eq!(
                total, 8,
                "variant {val:?} must not break the pipelined path"
            );
        }
        Ok(())
    }

    /// Sequential and pipelined paths must produce identical byte streams for
    /// the same input corpus (same observable contract).
    #[test]
    fn sequential_and_pipelined_agree_on_output() -> Result<(), String> {
        let dir = TempDir::new().map_err(|e| e.to_string())?;
        let shards: Vec<Vec<u8>> = (0..5)
            .map(|i| (0..(13 * (i + 1))).map(|b| b as u8).collect())
            .collect();
        let files = write_bins(dir.path(), &shards);

        // Pipelined (no env var).
        let mut pipelined_bytes = Vec::new();
        for_each_read_chunk(&files, 7, |c| {
            pipelined_bytes.extend_from_slice(c);
            Ok(())
        })?;

        // Sequential (via env var).
        let sequential_bytes = {
            let _guard = ENV_LOCK.lock().unwrap();
            // SAFETY: serialised via ENV_LOCK.
            unsafe { std::env::set_var(SEQUENTIAL_ENV, "1") };
            let mut acc = Vec::new();
            let r = for_each_read_chunk(&files, 7, |c| {
                acc.extend_from_slice(c);
                Ok(())
            });
            // SAFETY: serialised via ENV_LOCK.
            unsafe { std::env::remove_var(SEQUENTIAL_ENV) };
            r?;
            acc
        };

        assert_eq!(pipelined_bytes, sequential_bytes);
        Ok(())
    }
}
