//! Backend contract separating SeasonFetch's streaming/transfer orchestration from the
//! underlying `libtorrent-rasterbar 2.1.1` (`stremio-native/stream-server`) engine.

use std::future::Future;
use std::pin::Pin;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone)]
pub struct TorrentFileEntry {
    pub index: usize,
    pub name: String,
    pub length: u64,
}

#[derive(Debug, Clone, Default)]
pub struct TorrentTransferStats {
    pub peers_connected: u64,
    pub peers_seen: u64,
    pub download_speed_bps: u64,
    pub upload_speed_bps: u64,
    pub downloaded_bytes: u64,
    pub file_downloaded_bytes: u64,
    pub has_metadata: bool,
    pub is_finished: bool,
    pub error: Option<String>,
}

/// Pluggable BitTorrent backend trait.
pub trait TorrentBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn warmup(&self) -> BoxFuture<'_, Result<(), String>>;
}
