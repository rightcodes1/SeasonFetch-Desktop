//! Playback priority policy and container metadata heuristics.
//!
//! Adapted from the independent MIT-licensed open-source project
//! `stremio-native/stream-server` (Copyright (c) 2025 perpetus,
//! https://github.com/stremio-native/stream-server).

use serde::{Deserialize, Serialize};

pub const MIN_STARTUP_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_STARTUP_WINDOW_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_SEEK_HOT_WINDOW_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_WARM_WINDOW_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_CONTAINER_METADATA_WINDOW_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_DOWNLOAD_RANGE_WINDOW_BYTES: u64 = 32 * 1024 * 1024;
pub const SMALL_FILE_BYTES: u64 = 64 * 1024 * 1024;

pub const MAX_STARTUP_PIECES: i32 = 4;
pub const MAX_SMALL_FILE_STARTUP_PIECES: i32 = 32;
pub const MIN_STARTUP_PIECES: i32 = 2;

pub const MIN_SEEK_HOT_PIECES: i32 = 24;
pub const SEEK_IMMEDIATE_PIECES: i32 = 12;
pub const MAX_HOT_PIECES: i32 = 96;
pub const MAX_WARM_PIECES: i32 = 192;
const MIN_PIECE_DEADLINE_STEP_MS: u64 = 500;
const MAX_PIECE_DEADLINE_STEP_MS: u64 = 30_000;

/// Start treating reads as "container metadata" (MKV `Cues` / MP4 `moov`) when
/// they fall in the last 10 MiB or the last 5% of the file, whichever starts earlier.
pub fn container_metadata_start(file_size: u64) -> u64 {
    if file_size == 0 {
        0
    } else if file_size < SMALL_FILE_BYTES {
        file_size.saturating_mul(95) / 100
    } else {
        file_size
            .saturating_sub(10 * 1024 * 1024)
            .min(file_size.saturating_mul(95) / 100)
    }
}

pub fn is_container_metadata_request(start: u64, requested_len: u64, file_size: u64) -> bool {
    start > 0
        && file_size > 0
        && requested_len > 0
        && requested_len <= MAX_CONTAINER_METADATA_WINDOW_BYTES
        && start >= container_metadata_start(file_size)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackIntent {
    DirectInitial,
    ContainerMetadata,
    DirectSeek,
    DirectSequential,
    DownloadFull,
    DownloadRange,
    Background,
}

impl PlaybackIntent {
    pub fn sequential_after_first_byte(self) -> Self {
        match self {
            Self::DirectInitial | Self::DirectSeek | Self::DirectSequential => {
                Self::DirectSequential
            }
            other => other,
        }
    }
}

/// Classify an incoming HTTP Range request into a `PlaybackIntent`.
pub fn classify_http_intent(
    start: u64,
    requested_len: u64,
    file_size: u64,
    is_download: bool,
    is_partial: bool,
) -> PlaybackIntent {
    if is_download && (!is_partial || requested_len >= file_size.saturating_sub(start)) {
        return PlaybackIntent::DownloadFull;
    }
    if is_download && is_partial {
        return PlaybackIntent::DownloadRange;
    }
    if is_container_metadata_request(start, requested_len, file_size) {
        return PlaybackIntent::ContainerMetadata;
    }
    if start == 0 {
        PlaybackIntent::DirectInitial
    } else {
        PlaybackIntent::DirectSeek
    }
}

pub fn playback_deadline_step_ms(
    piece_length: u64,
    bitrate_bytes_per_sec: Option<u64>,
    download_rate_bytes_per_sec: u64,
) -> i32 {
    let bitrate = bitrate_bytes_per_sec.filter(|rate| *rate > 0);
    let download_rate = (download_rate_bytes_per_sec > 0).then_some(download_rate_bytes_per_sec);
    let effective_rate = match (bitrate, download_rate) {
        (Some(bitrate), Some(download_rate)) => bitrate.min(download_rate),
        (Some(bitrate), None) => bitrate,
        (None, Some(download_rate)) => download_rate,
        (None, None) => 1024 * 1024,
    };
    let step_ms = piece_length
        .saturating_mul(1_000)
        .div_ceil(effective_rate)
        .clamp(MIN_PIECE_DEADLINE_STEP_MS, MAX_PIECE_DEADLINE_STEP_MS);
    step_ms as i32
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryPressure {
    Normal,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PriorityBand {
    Immediate,
    Hot,
    Warm,
    Metadata,
    Background,
}

#[derive(Debug, Clone)]
pub struct PriorityContext {
    pub intent: PlaybackIntent,
    pub current_piece: i32,
    pub first_piece: i32,
    pub last_piece: i32,
    pub piece_length: u64,
    pub file_size: u64,
    pub bitrate_bytes_per_sec: Option<u64>,
    pub download_rate_bytes_per_sec: u64,
    pub peers: u64,
    pub cache_size_bytes: u64,
    pub memory_pressure: MemoryPressure,
    pub consecutive_waits: u32,
    pub first_byte_sent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorityAssignment {
    pub piece_idx: i32,
    pub piece_priority: i32,
    pub deadline: i32,
    pub band: PriorityBand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorityDecision {
    pub assignments: Vec<PriorityAssignment>,
    pub target_window_pieces: i32,
    pub immediate_pieces: i32,
    pub hot_window_pieces: i32,
    pub warm_window_pieces: i32,
    pub reason: String,
}

impl PriorityDecision {
    /// Split assignments into `(urgent, hot, warm)` piece index lists for scheduler tiers.
    pub fn split_tiers(&self) -> (Vec<u32>, Vec<u32>, Vec<u32>) {
        let mut urgent = Vec::new();
        let mut hot = Vec::new();
        let mut warm = Vec::new();
        for a in &self.assignments {
            if a.piece_idx < 0 {
                continue;
            }
            let idx = a.piece_idx as u32;
            match a.band {
                PriorityBand::Immediate | PriorityBand::Metadata => urgent.push(idx),
                PriorityBand::Hot => hot.push(idx),
                PriorityBand::Warm | PriorityBand::Background => warm.push(idx),
            }
        }
        (urgent, hot, warm)
    }
}

pub struct PlaybackPriorityPolicy;

impl PlaybackPriorityPolicy {
    pub fn decide(ctx: PriorityContext) -> PriorityDecision {
        if ctx.piece_length == 0
            || ctx.current_piece < ctx.first_piece
            || ctx.last_piece < ctx.first_piece
        {
            return PriorityDecision {
                assignments: Vec::new(),
                target_window_pieces: 0,
                immediate_pieces: 0,
                hot_window_pieces: 0,
                warm_window_pieces: 0,
                reason: "invalid-context".to_string(),
            };
        }

        let max_cache_pieces = if ctx.cache_size_bytes > 0 {
            (ctx.cache_size_bytes / ctx.piece_length).max(1) as i32
        } else {
            MAX_HOT_PIECES
        };
        let remaining_pieces = ctx.last_piece.saturating_sub(ctx.current_piece) + 1;

        let bitrate_ratio = ctx
            .bitrate_bytes_per_sec
            .filter(|bitrate| *bitrate > 0)
            .map(|bitrate| ctx.download_rate_bytes_per_sec as f64 / bitrate as f64);

        let mut reason = match ctx.intent {
            PlaybackIntent::DirectInitial => "initial".to_string(),
            PlaybackIntent::DirectSeek => "seek".to_string(),
            PlaybackIntent::DirectSequential => "sequential".to_string(),
            PlaybackIntent::DownloadFull => "download-full".to_string(),
            PlaybackIntent::DownloadRange => "download-range".to_string(),
            PlaybackIntent::ContainerMetadata => "container-metadata".to_string(),
            PlaybackIntent::Background => "background".to_string(),
        };

        let (mut immediate, mut hot, mut warm) = match ctx.intent {
            PlaybackIntent::DirectInitial if !ctx.first_byte_sent => {
                let target_bytes = match ctx.bitrate_bytes_per_sec {
                    Some(bitrate) => bitrate.saturating_mul(10).max(MIN_STARTUP_BYTES),
                    None if ctx.file_size > 0 && ctx.file_size <= SMALL_FILE_BYTES => {
                        ctx.file_size.min(MAX_STARTUP_WINDOW_BYTES)
                    }
                    None => MIN_STARTUP_BYTES,
                }
                .max(ctx.piece_length)
                .min(MAX_STARTUP_WINDOW_BYTES.max(ctx.piece_length));
                let pieces = target_bytes.saturating_add(ctx.piece_length.saturating_sub(1))
                    / ctx.piece_length;
                let max_startup_pieces =
                    if ctx.bitrate_bytes_per_sec.is_none() && ctx.file_size <= SMALL_FILE_BYTES {
                        pieces_for_bytes(
                            ctx.file_size.min(MAX_STARTUP_WINDOW_BYTES),
                            ctx.piece_length,
                        )
                        .clamp(1, MAX_SMALL_FILE_STARTUP_PIECES)
                    } else {
                        MAX_STARTUP_PIECES
                    };
                let min_startup_pieces = MIN_STARTUP_PIECES.min(max_startup_pieces).max(1);
                let pieces = (pieces as i32).clamp(min_startup_pieces, max_startup_pieces);
                (pieces.min(MAX_STARTUP_PIECES), pieces, 0)
            }
            PlaybackIntent::DirectInitial | PlaybackIntent::DirectSequential => {
                let hot = dynamic_hot_window(&ctx, bitrate_ratio);
                (2, hot, 32)
            }
            PlaybackIntent::DownloadFull => (2, 16, 0),
            PlaybackIntent::DownloadRange => (1, 4, 0),
            PlaybackIntent::DirectSeek if !ctx.first_byte_sent => {
                reason.push_str("-first-piece-only");
                (1, 1, 0)
            }
            PlaybackIntent::DirectSeek => {
                let mut hot = dynamic_hot_window(&ctx, bitrate_ratio).max(MIN_SEEK_HOT_PIECES);
                let mut immediate = SEEK_IMMEDIATE_PIECES;
                if ctx.consecutive_waits >= 3 {
                    hot = (hot * 2).min(MAX_HOT_PIECES);
                    immediate = (immediate * 2).min(hot);
                    reason.push_str("-blocked-expand");
                }
                (immediate, hot, 32)
            }
            PlaybackIntent::ContainerMetadata => (1, 2, 0),
            PlaybackIntent::Background => (0, 4, 0),
        };

        if matches!(ctx.memory_pressure, MemoryPressure::High) {
            hot = hot.min(MIN_SEEK_HOT_PIECES);
            warm = 0;
            reason.push_str("-memory-clamp");
        }

        if matches!(ctx.intent, PlaybackIntent::Background) {
            warm = 0;
        }

        let original_hot = hot;
        let original_warm = warm;
        hot = cap_pieces_by_bytes(hot, ctx.piece_length, hot_byte_cap(&ctx));
        warm = cap_pieces_by_bytes(warm, ctx.piece_length, warm_byte_cap(ctx.intent));
        if hot < original_hot || warm < original_warm {
            reason.push_str("-byte-cap");
        }

        hot = hot
            .clamp(0, MAX_HOT_PIECES)
            .min(max_cache_pieces)
            .min(remaining_pieces);
        warm = warm
            .clamp(0, MAX_WARM_PIECES)
            .min(max_cache_pieces.saturating_sub(hot))
            .min(remaining_pieces.saturating_sub(hot));
        immediate = immediate.min(hot).max(0);

        let target_window = hot + warm;
        let mut assignments = Vec::with_capacity(target_window as usize);
        for distance in 0..target_window {
            let piece_idx = ctx.current_piece + distance;
            if piece_idx > ctx.last_piece {
                break;
            }
            let (band, piece_priority, deadline) = assignment_for(&ctx, distance, immediate, hot);
            assignments.push(PriorityAssignment {
                piece_idx,
                piece_priority,
                deadline,
                band,
            });
        }

        PriorityDecision {
            assignments,
            target_window_pieces: target_window,
            immediate_pieces: immediate,
            hot_window_pieces: hot,
            warm_window_pieces: warm,
            reason,
        }
    }
}

fn pieces_for_bytes(bytes: u64, piece_length: u64) -> i32 {
    if bytes == 0 || piece_length == 0 {
        return 0;
    }
    let pieces = bytes.saturating_add(piece_length.saturating_sub(1)) / piece_length;
    pieces.clamp(1, i32::MAX as u64) as i32
}

fn cap_pieces_by_bytes(pieces: i32, piece_length: u64, max_bytes: u64) -> i32 {
    if pieces <= 0 || max_bytes == 0 {
        return 0;
    }
    pieces.min(pieces_for_bytes(max_bytes, piece_length).max(1))
}

fn hot_byte_cap(ctx: &PriorityContext) -> u64 {
    match ctx.intent {
        PlaybackIntent::DirectInitial if !ctx.first_byte_sent => MAX_STARTUP_WINDOW_BYTES,
        PlaybackIntent::DirectInitial
        | PlaybackIntent::DirectSeek
        | PlaybackIntent::DirectSequential => MAX_SEEK_HOT_WINDOW_BYTES,
        PlaybackIntent::DownloadFull => MAX_WARM_WINDOW_BYTES,
        PlaybackIntent::DownloadRange => MAX_DOWNLOAD_RANGE_WINDOW_BYTES,
        PlaybackIntent::ContainerMetadata | PlaybackIntent::Background => {
            MAX_CONTAINER_METADATA_WINDOW_BYTES
        }
    }
}

fn warm_byte_cap(intent: PlaybackIntent) -> u64 {
    match intent {
        PlaybackIntent::DirectInitial
        | PlaybackIntent::DirectSeek
        | PlaybackIntent::DirectSequential
        | PlaybackIntent::DownloadFull => MAX_WARM_WINDOW_BYTES,
        PlaybackIntent::DownloadRange
        | PlaybackIntent::ContainerMetadata
        | PlaybackIntent::Background => 0,
    }
}

fn dynamic_hot_window(ctx: &PriorityContext, bitrate_ratio: Option<f64>) -> i32 {
    let mut hot = if let Some(ratio) = bitrate_ratio {
        if ratio >= 3.0 {
            96
        } else if ratio >= 1.5 {
            48
        } else if ratio >= 1.0 {
            32
        } else {
            MIN_SEEK_HOT_PIECES
        }
    } else if ctx.download_rate_bytes_per_sec > 10 * 1024 * 1024 {
        96
    } else if ctx.download_rate_bytes_per_sec > 5 * 1024 * 1024 {
        48
    } else if ctx.download_rate_bytes_per_sec > 1024 * 1024 {
        MIN_SEEK_HOT_PIECES
    } else {
        16
    };

    if let Some(bitrate) = ctx.bitrate_bytes_per_sec.filter(|bitrate| *bitrate > 0) {
        let pieces_for_10s = ((bitrate.saturating_mul(10)) / ctx.piece_length).max(1) as i32;
        hot = hot.max(pieces_for_10s);
    }

    if ctx.peers < 3 {
        hot = hot.min(MIN_SEEK_HOT_PIECES);
    }

    hot
}

fn assignment_for(
    ctx: &PriorityContext,
    distance: i32,
    immediate_pieces: i32,
    hot_pieces: i32,
) -> (PriorityBand, i32, i32) {
    let deadline_step = playback_deadline_step_ms(
        ctx.piece_length,
        ctx.bitrate_bytes_per_sec,
        ctx.download_rate_bytes_per_sec,
    );
    match ctx.intent {
        PlaybackIntent::ContainerMetadata => (PriorityBand::Metadata, 7, distance * deadline_step),
        PlaybackIntent::Background => (
            PriorityBand::Background,
            1,
            30_000 + distance * deadline_step,
        ),
        _ if distance < immediate_pieces => (PriorityBand::Immediate, 7, distance * deadline_step),
        _ if distance < hot_pieces => (PriorityBand::Hot, 4, distance * deadline_step),
        _ => (PriorityBand::Warm, 2, 10_000 + distance * deadline_step),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_ctx(intent: PlaybackIntent) -> PriorityContext {
        PriorityContext {
            intent,
            current_piece: 10,
            first_piece: 0,
            last_piece: 500,
            piece_length: 2 * 1024 * 1024,
            file_size: 1000 * 1024 * 1024,
            bitrate_bytes_per_sec: None,
            download_rate_bytes_per_sec: 4 * 1024 * 1024,
            peers: 12,
            cache_size_bytes: 512 * 1024 * 1024,
            memory_pressure: MemoryPressure::Normal,
            consecutive_waits: 0,
            first_byte_sent: true,
        }
    }

    #[test]
    fn initial_before_first_byte_stays_tight() {
        let mut ctx = base_ctx(PlaybackIntent::DirectInitial);
        ctx.current_piece = 0;
        ctx.first_byte_sent = false;
        let dec = PlaybackPriorityPolicy::decide(ctx);
        assert!(dec.target_window_pieces <= MAX_STARTUP_PIECES);
        assert_eq!(dec.assignments[0].band, PriorityBand::Immediate);
        assert_eq!(dec.assignments[0].deadline, 0);
    }

    #[test]
    fn seek_before_first_byte_focuses_single_piece() {
        let mut ctx = base_ctx(PlaybackIntent::DirectSeek);
        ctx.first_byte_sent = false;
        let dec = PlaybackPriorityPolicy::decide(ctx);
        assert_eq!(dec.target_window_pieces, 1);
        assert_eq!(dec.assignments[0].piece_idx, 10);
        assert_eq!(dec.assignments[0].band, PriorityBand::Immediate);
    }

    #[test]
    fn container_metadata_detection_handles_tail_cues() {
        let size = 500 * 1024 * 1024;
        let tail = container_metadata_start(size);
        assert!(is_container_metadata_request(tail, 64 * 1024, size));
        assert!(!is_container_metadata_request(0, 64 * 1024, size));
        assert_eq!(
            classify_http_intent(tail, 64 * 1024, size, false, true),
            PlaybackIntent::ContainerMetadata
        );
        assert_eq!(
            classify_http_intent(0, size, size, false, false),
            PlaybackIntent::DirectInitial
        );
        assert_eq!(
            classify_http_intent(50 * 1024 * 1024, size - 50 * 1024 * 1024, size, false, true),
            PlaybackIntent::DirectSeek
        );
    }
}
