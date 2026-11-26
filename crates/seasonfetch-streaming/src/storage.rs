//! Cross-platform sparse chunked `TorrentStorage` for SeasonFetch streaming.
//!
//! Why this exists (and why standard contiguous file storage fails for streaming):
//! On Windows NTFS, writing a 16 KiB block near the tail of a 6–50 GB MKV file (e.g., to read
//! Matroska `Cues` or seek mid-movie) forces the OS to physically zero-fill the entire 6–50 GB
//! file unless the storage is sparse. On machines or Android devices with limited free disk space
//! (e.g. 1–4 GB free), contiguous storage fails immediately with `os error 112` (disk full).
//!
//! `SparseChunkStorage` maps every `(file_id, chunk_index)` to an independent 2 MiB block file
//! (`f{file_id}_c{chunk_index:06}.blk`) inside the session's temporary cache directory:
//! - Seeking to byte 6,367,000,000 of a 6.37 GB BluRay MKV allocates <= 2 MiB on disk.
//! - Works identically on Windows NTFS/exFAT/FAT32, Linux ext4, and Android scoped storage.
//! - Uses a bounded open-file LRU pool (max 32 handles) and releases all handles instantaneously
//!   on `take()` / `close_all_handles()` so session teardown removes temp files cleanly on Windows.

use anyhow::{anyhow, Context, Result};
use librqbit::storage::{BoxStorageFactory, StorageFactory, StorageFactoryExt, TorrentStorage};
use librqbit::{ManagedTorrentShared, TorrentMetadata};
use std::collections::{HashMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const SPARSE_CHUNK_SIZE: u64 = 2 * 1024 * 1024; // 2 MiB per chunk file
const MAX_OPEN_HANDLES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ChunkKey {
    file_id: usize,
    chunk_idx: u64,
}

struct HandlePool {
    root: PathBuf,
    closed: bool,
    handles: HashMap<ChunkKey, File>,
    lru: VecDeque<ChunkKey>,
}

impl HandlePool {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            closed: false,
            handles: HashMap::new(),
            lru: VecDeque::new(),
        }
    }

    fn chunk_path(&self, key: ChunkKey) -> PathBuf {
        self.root
            .join(format!("f{}_c{:06}.blk", key.file_id, key.chunk_idx))
    }

    fn touch_lru(&mut self, key: ChunkKey) {
        if let Some(pos) = self.lru.iter().position(|k| *k == key) {
            self.lru.remove(pos);
        }
        self.lru.push_back(key);
        while self.lru.len() > MAX_OPEN_HANDLES {
            if let Some(oldest) = self.lru.pop_front() {
                self.handles.remove(&oldest);
            }
        }
    }

    fn get_or_open(&mut self, key: ChunkKey, create: bool) -> Result<&mut File> {
        if self.closed {
            return Err(anyhow!("sparse streaming storage has been closed"));
        }
        if !self.handles.contains_key(&key) {
            let path = self.chunk_path(key);
            if !create && !path.exists() {
                return Err(anyhow!("chunk {:?} not yet present", key));
            }
            if create {
                let _ = std::fs::create_dir_all(&self.root);
            }
            // Always open with read+write so a handle cached by `pread_exact`
            // (after LRU eviction) never causes `Access is denied (os error 5)`
            // when `pwrite_all` later writes another block into the same 2 MiB chunk.
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(create)
                .open(&path)
                .with_context(|| format!("failed to open sparse chunk {}", path.display()))?;
            self.handles.insert(key, file);
        }
        self.touch_lru(key);
        self.handles
            .get_mut(&key)
            .ok_or_else(|| anyhow!("chunk handle missing from pool"))
    }

    fn close_all(&mut self) {
        self.closed = true;
        self.handles.clear();
        self.lru.clear();
    }

    fn remove_file_chunks(&mut self, file_id: usize) {
        let keys: Vec<ChunkKey> = self
            .handles
            .keys()
            .copied()
            .filter(|k| k.file_id == file_id)
            .collect();
        for k in keys {
            self.handles.remove(&k);
        }
        self.lru.retain(|k| k.file_id != file_id);

        let prefix = format!("f{file_id}_c");
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.starts_with(&prefix) && name.ends_with(".blk") {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct SparseChunkStorage {
    pool: Arc<Mutex<HandlePool>>,
}

impl SparseChunkStorage {
    pub fn new(root: PathBuf) -> Self {
        Self {
            pool: Arc::new(Mutex::new(HandlePool::new(root))),
        }
    }

    pub fn close_all_handles(&self) {
        if let Ok(mut guard) = self.pool.lock() {
            guard.close_all();
        }
    }
}

impl TorrentStorage for SparseChunkStorage {
    fn init(
        &mut self,
        _shared: &ManagedTorrentShared,
        _metadata: &TorrentMetadata,
    ) -> Result<()> {
        let mut guard = self
            .pool
            .lock()
            .map_err(|_| anyhow!("storage mutex poisoned"))?;
        guard.closed = false;
        std::fs::create_dir_all(&guard.root)
            .with_context(|| format!("failed to create sparse cache dir {}", guard.root.display()))?;
        Ok(())
    }

    fn pread_exact(&self, file_id: usize, offset: u64, buf: &mut [u8]) -> Result<()> {
        if buf.is_empty() {
            return Ok(());
        }
        let mut guard = self
            .pool
            .lock()
            .map_err(|_| anyhow!("storage mutex poisoned"))?;

        let mut remaining = buf.len();
        let mut buf_pos = 0usize;
        let mut cur_offset = offset;

        while remaining > 0 {
            let chunk_idx = cur_offset / SPARSE_CHUNK_SIZE;
            let in_chunk = (cur_offset % SPARSE_CHUNK_SIZE) as usize;
            let take = remaining.min((SPARSE_CHUNK_SIZE as usize) - in_chunk);

            let key = ChunkKey { file_id, chunk_idx };
            let file = guard.get_or_open(key, false)?;
            file.seek(SeekFrom::Start(in_chunk as u64))?;
            file.read_exact(&mut buf[buf_pos..buf_pos + take])?;

            buf_pos += take;
            cur_offset += take as u64;
            remaining -= take;
        }
        Ok(())
    }

    fn pwrite_all(&self, file_id: usize, offset: u64, buf: &[u8]) -> Result<()> {
        if buf.is_empty() {
            return Ok(());
        }
        let mut guard = self
            .pool
            .lock()
            .map_err(|_| anyhow!("storage mutex poisoned"))?;

        let mut remaining = buf.len();
        let mut buf_pos = 0usize;
        let mut cur_offset = offset;

        while remaining > 0 {
            let chunk_idx = cur_offset / SPARSE_CHUNK_SIZE;
            let in_chunk = (cur_offset % SPARSE_CHUNK_SIZE) as usize;
            let take = remaining.min((SPARSE_CHUNK_SIZE as usize) - in_chunk);

            let key = ChunkKey { file_id, chunk_idx };
            let file = guard.get_or_open(key, true)?;
            file.seek(SeekFrom::Start(in_chunk as u64))?;
            file.write_all(&buf[buf_pos..buf_pos + take])?;

            buf_pos += take;
            cur_offset += take as u64;
            remaining -= take;
        }
        Ok(())
    }

    fn remove_file(&self, file_id: usize, _filename: &Path) -> Result<()> {
        if let Ok(mut guard) = self.pool.lock() {
            guard.remove_file_chunks(file_id);
        }
        Ok(())
    }

    fn remove_directory_if_empty(&self, path: &Path) -> Result<()> {
        let _ = std::fs::remove_dir(path);
        Ok(())
    }

    fn ensure_file_length(&self, _file_id: usize, _length: u64) -> Result<()> {
        // Intentionally a no-op: never pre-allocate multi-GB files on disk!
        Ok(())
    }

    fn take(&self) -> Result<Box<dyn TorrentStorage>> {
        let mut guard = self
            .pool
            .lock()
            .map_err(|_| anyhow!("storage mutex poisoned"))?;
        let root = guard.root.clone();
        guard.close_all();
        Ok(Box::new(SparseChunkStorage::new(root)))
    }
}

#[derive(Clone)]
pub struct SparseChunkStorageFactory {
    current_root: Arc<Mutex<PathBuf>>,
    /// Reference to the active storage instance so the streaming engine can
    /// force-close every open Windows file handle synchronously on player close or app exit.
    active_storage: Arc<Mutex<Option<SparseChunkStorage>>>,
}

impl SparseChunkStorageFactory {
    pub fn new(default_root: PathBuf) -> Self {
        Self {
            current_root: Arc::new(Mutex::new(default_root)),
            active_storage: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_session_dir(&self, dir: PathBuf) {
        if let Ok(mut guard) = self.current_root.lock() {
            *guard = dir;
        }
    }

    pub fn close_active_handles(&self) {
        if let Ok(mut guard) = self.active_storage.lock() {
            if let Some(st) = guard.take() {
                st.close_all_handles();
            }
        }
    }
}

impl StorageFactory for SparseChunkStorageFactory {
    type Storage = SparseChunkStorage;

    fn create(
        &self,
        _shared: &ManagedTorrentShared,
        _metadata: &TorrentMetadata,
    ) -> Result<Self::Storage> {
        let root = self
            .current_root
            .lock()
            .map_err(|_| anyhow!("root mutex poisoned"))?
            .clone();
        let storage = SparseChunkStorage::new(root);
        if let Ok(mut guard) = self.active_storage.lock() {
            *guard = Some(storage.clone());
        }
        Ok(storage)
    }

    fn clone_box(&self) -> BoxStorageFactory {
        self.clone().boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_reads_at_6gb_offset_using_minimal_disk_space() {
        let tmp = std::env::temp_dir().join(format!(
            "sf-sparse-storage-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let storage = SparseChunkStorage::new(tmp.clone());

        // Write 32 KiB spanning a 2 MiB chunk boundary at ~6.37 GB offset
        let offset_6gb: u64 = 6_367_000_000;
        let payload: Vec<u8> = (0..32768).map(|i| (i % 251) as u8).collect();
        storage.pwrite_all(0, offset_6gb, &payload).unwrap();

        let mut read_back = vec![0u8; payload.len()];
        storage.pread_exact(0, offset_6gb, &mut read_back).unwrap();
        assert_eq!(read_back, payload);

        // Total disk footprint must be tiny (<= 4 MiB), NOT 6.37 GB!
        let mut total_disk = 0u64;
        for entry in std::fs::read_dir(&tmp).unwrap().flatten() {
            total_disk += entry.metadata().unwrap().len();
        }
        assert!(
            total_disk <= 4 * 1024 * 1024,
            "expected <= 4 MiB on disk for 6.37 GB tail write, got {total_disk} bytes"
        );

        storage.close_all_handles();
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn pread_after_lru_eviction_does_not_block_subsequent_pwrite() {
        let tmp = std::env::temp_dir().join(format!(
            "sf-sparse-evict-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let storage = SparseChunkStorage::new(tmp.clone());

        // Write first block into chunk 0
        storage.pwrite_all(4, 0, &[0xAA; 16384]).unwrap();

        // Evict chunk 0 from the open-handle LRU cache by touching > MAX_OPEN_HANDLES other chunks
        for c in 1..=(MAX_OPEN_HANDLES as u64 + 6) {
            storage
                .pwrite_all(4, c * SPARSE_CHUNK_SIZE, &[0x55; 64])
                .unwrap();
        }

        // Re-open chunk 0 via pread_exact, then immediately write another block into chunk 0.
        // Previously this failed on Windows with `Access is denied (os error 5)` because
        // pread_exact cached a read-only handle!
        let mut buf = [0u8; 16384];
        storage.pread_exact(4, 0, &mut buf).unwrap();
        assert_eq!(buf[0], 0xAA);
        storage.pwrite_all(4, 16384, &[0xBB; 16384]).unwrap();

        let mut verify = [0u8; 16384];
        storage.pread_exact(4, 16384, &mut verify).unwrap();
        assert_eq!(verify[0], 0xBB);

        storage.close_all_handles();
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
