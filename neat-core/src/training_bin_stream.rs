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

use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

/// Read every byte from `bin_files` in order, invoking `on_chunk` for each read segment.
///
/// `read_buf_len` should be at least one record (`>= record_bytes`); callers typically
/// use ~2 MiB rounded down to a whole number of records.
///
/// On native targets, reads are pipelined with a background thread. On `wasm32`, reads
/// are sequential and as large as `read_buf_len` allows — still chunked so you never
/// load an entire shard into memory at once.
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
        for_each_read_chunk_native(bin_files, read_buf_len, on_chunk)
    }

    #[cfg(target_arch = "wasm32")]
    {
        for_each_read_chunk_wasm32(bin_files, read_buf_len, on_chunk)
    }
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

    while let ReaderMsg::Chunk(buf, n) = fill_rx
        .recv()
        .map_err(|_| "reader disconnected".to_string())?
    {
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

    drop(empty_tx);
    match reader_handle.join() {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("reader thread panicked".to_string()),
    }
}

#[cfg(target_arch = "wasm32")]
fn for_each_read_chunk_wasm32<F>(
    bin_files: &[PathBuf],
    read_buf_len: usize,
    mut on_chunk: F,
) -> Result<(), String>
where
    F: FnMut(&[u8]) -> Result<(), String>,
{
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn for_each_read_chunk_concatenates_files() -> Result<(), String> {
        let dir = TempDir::new().map_err(|e| e.to_string())?;
        fs::write(dir.path().join("0.bin"), [1u8, 2, 3, 4]).map_err(|e| e.to_string())?;
        fs::write(dir.path().join("1.bin"), [5u8, 6]).map_err(|e| e.to_string())?;
        let files = vec![dir.path().join("0.bin"), dir.path().join("1.bin")];
        let mut acc = Vec::new();
        for_each_read_chunk(&files, 3, |c| {
            acc.extend_from_slice(c);
            Ok(())
        })?;
        assert_eq!(acc, vec![1, 2, 3, 4, 5, 6]);
        Ok(())
    }
}
