//! One API to stream packed `.bin` training bytes in large chunks.
//!
//! [`for_each_read_chunk`] delegates to [`for_each_read_chunk_with_mode`] with
//! [`TrainingReadMode::PipelinedDoubleBuffer`] on native hosts.
//!
//! - **Native + [`TrainingReadMode::PipelinedDoubleBuffer`]**: reader thread + ping-pong
//!   `Vec<u8>` buffers (overlap disk read with consumer work).
//! - **Native + [`TrainingReadMode::SingleBufferSequential`]**: one `read_buf`, sequential
//!   `File::read` (tuning experiments; no background thread).
//! - **`wasm32-unknown-unknown`:** always sequential chunked `read` (mode is ignored).
//!
//! ## Process env (NEAT-AI-scorer / `float_scan_bench`)
//!
//! - **`NEAT_SCORER_IO_MODE`**: `double` (default) | `single`
//! - **`NEAT_SCORER_READ_BYTES`**: decimal target bytes per read (default `2097152`, max `67108864`)

use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

/// How to read `.bin` bytes on **native** targets. Ignored on `wasm32` (always sequential).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrainingReadMode {
    /// Reader thread + two recycled buffers (default).
    #[default]
    PipelinedDoubleBuffer,
    /// One buffer, main thread only — for A/B tuning vs pipelined.
    SingleBufferSequential,
}

/// Stable label for logging / JSON (`NEAT_SCORER_IO_MODE` in the scorer).
const DEFAULT_READ_BYTES: usize = 2 * 1024 * 1024;
const MAX_READ_BYTES: usize = 64 * 1024 * 1024;

/// Parse [`TrainingReadMode`] and read-size target from **`NEAT_SCORER_*`** env vars.
///
/// `record_bytes` is used to clamp the lower bound. The returned byte target is **before**
/// rounding down to whole records; callers should still do `(target / record_bytes) * record_bytes`.
pub fn training_read_tuning_from_env(record_bytes: usize) -> (TrainingReadMode, usize) {
    let mode = match std::env::var("NEAT_SCORER_IO_MODE") {
        Ok(s) if s.trim().eq_ignore_ascii_case("single") => {
            TrainingReadMode::SingleBufferSequential
        }
        Ok(s) if s.trim().eq_ignore_ascii_case("double") => TrainingReadMode::PipelinedDoubleBuffer,
        Ok(_) => TrainingReadMode::PipelinedDoubleBuffer,
        Err(_) => TrainingReadMode::PipelinedDoubleBuffer,
    };
    let raw = std::env::var("NEAT_SCORER_READ_BYTES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(DEFAULT_READ_BYTES);
    let target = raw.clamp(record_bytes.max(1), MAX_READ_BYTES);
    (mode, target)
}

pub fn io_backend_label(mode: TrainingReadMode) -> &'static str {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = mode;
        "sequential_chunked_file_read"
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        match mode {
            TrainingReadMode::PipelinedDoubleBuffer => "pipelined_double_buffer",
            TrainingReadMode::SingleBufferSequential => "single_buffer_sequential",
        }
    }
}

/// Sequential large `read` calls into one buffer (all targets).
fn for_each_read_chunk_sequential<F>(
    bin_files: &[PathBuf],
    read_buf_len: usize,
    mut on_chunk: F,
) -> Result<(), String>
where
    F: FnMut(&[u8]) -> Result<(), String>,
{
    if read_buf_len == 0 {
        return Err("read_buf_len must be positive".to_string());
    }
    let mut read_buf = vec![0u8; read_buf_len];
    for path in bin_files {
        let mut file = File::open(path)
            .map_err(|e| format!("Failed to open training file '{}': {e}", path.display()))?;
        loop {
            let n = file
                .read(&mut read_buf)
                .map_err(|e| format!("Failed reading training file '{}': {e}", path.display()))?;
            if n > 0 {
                on_chunk(&read_buf[..n])?;
            }
            if n == 0 {
                break;
            }
        }
    }
    Ok(())
}

pub fn for_each_read_chunk_with_mode<F>(
    bin_files: &[PathBuf],
    read_buf_len: usize,
    mode: TrainingReadMode,
    on_chunk: F,
) -> Result<(), String>
where
    F: FnMut(&[u8]) -> Result<(), String>,
{
    #[cfg(target_arch = "wasm32")]
    {
        let _ = mode;
        for_each_read_chunk_sequential(bin_files, read_buf_len, on_chunk)
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        match mode {
            TrainingReadMode::PipelinedDoubleBuffer => {
                for_each_read_chunk_native_double(bin_files, read_buf_len, on_chunk)
            }
            TrainingReadMode::SingleBufferSequential => {
                for_each_read_chunk_sequential(bin_files, read_buf_len, on_chunk)
            }
        }
    }
}

pub fn for_each_read_chunk<F>(
    bin_files: &[PathBuf],
    read_buf_len: usize,
    on_chunk: F,
) -> Result<(), String>
where
    F: FnMut(&[u8]) -> Result<(), String>,
{
    for_each_read_chunk_with_mode(
        bin_files,
        read_buf_len,
        TrainingReadMode::PipelinedDoubleBuffer,
        on_chunk,
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn for_each_read_chunk_native_double<F>(
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
        Chunk(Vec<u8>, usize),
        Done,
    }

    let (fill_tx, fill_rx) = mpsc::sync_channel::<ReaderMsg>(1);
    let (empty_tx, empty_rx) = mpsc::sync_channel::<Vec<u8>>(2);

    empty_tx
        .send(vec![0u8; read_buf_len])
        .map_err(|_| "failed to seed read buffer (consumer dropped?)".to_string())?;
    empty_tx
        .send(vec![0u8; read_buf_len])
        .map_err(|_| "failed to seed read buffer (consumer dropped?)".to_string())?;

    let paths: Vec<PathBuf> = bin_files.to_vec();
    let reader_handle = thread::spawn(move || -> Result<(), String> {
        for path in &paths {
            let mut file = File::open(path)
                .map_err(|e| format!("Failed to open training file '{}': {e}", path.display()))?;
            loop {
                let mut buf = empty_rx
                    .recv()
                    .map_err(|_| "read buffer pool closed unexpectedly".to_string())?;
                let n = file.read(&mut buf).map_err(|e| {
                    format!("Failed reading training file '{}': {e}", path.display())
                })?;
                if fill_tx.send(ReaderMsg::Chunk(buf, n)).is_err() {
                    return Ok(());
                }
                if n == 0 {
                    break;
                }
            }
        }
        let _ = fill_tx.send(ReaderMsg::Done);
        Ok(())
    });

    loop {
        match fill_rx
            .recv()
            .map_err(|_| "reader disconnected".to_string())?
        {
            ReaderMsg::Chunk(buf, n) => {
                let return_buf = if n > 0 {
                    let r = on_chunk(&buf[..n]);
                    if let Err(e) = r {
                        empty_tx
                            .send(buf)
                            .map_err(|_| "failed to return buffer after error".to_string())?;
                        return Err(e);
                    }
                    buf
                } else {
                    buf
                };
                empty_tx
                    .send(return_buf)
                    .map_err(|_| "failed to return read buffer to pool".to_string())?;
            }
            ReaderMsg::Done => break,
        }
    }

    drop(empty_tx);
    match reader_handle.join() {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("reader thread panicked".to_string()),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn run_files(mode: TrainingReadMode) -> Result<Vec<u8>, String> {
        let dir = TempDir::new().map_err(|e| e.to_string())?;
        fs::write(dir.path().join("0.bin"), [1u8, 2, 3, 4]).map_err(|e| e.to_string())?;
        fs::write(dir.path().join("1.bin"), [5u8, 6]).map_err(|e| e.to_string())?;
        let files = vec![dir.path().join("0.bin"), dir.path().join("1.bin")];
        let mut acc = Vec::new();
        for_each_read_chunk_with_mode(&files, 3, mode, |c| {
            acc.extend_from_slice(c);
            Ok(())
        })?;
        Ok(acc)
    }

    #[test]
    fn for_each_read_chunk_double_matches_single() -> Result<(), String> {
        assert_eq!(
            run_files(TrainingReadMode::PipelinedDoubleBuffer)?,
            run_files(TrainingReadMode::SingleBufferSequential)?
        );
        Ok(())
    }
}
