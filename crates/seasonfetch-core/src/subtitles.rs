use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleSearchQuery {
    #[serde(default)]
    pub imdb_id: Option<String>,
    #[serde(default)]
    pub media_type: Option<String>, // "movie" | "series"
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub season: Option<u32>,
    #[serde(default)]
    pub episode: Option<u32>,
    #[serde(default)]
    pub release_name: Option<String>,
    #[serde(default)]
    pub local_file_path: Option<String>,
    #[serde(default)]
    pub language: Option<String>, // e.g. "eng", "ara", "any"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleCandidate {
    pub id: String,
    pub provider: String, // "opensubtitles-addon" | "opensubtitles-api" | "subdl" | "custom-addon"
    pub language: String,
    pub language_label: String,
    pub release_name: String,
    pub format: String, // "srt" | "ass" | "ssa" | "vtt"
    pub download_url: String,
    #[serde(default)]
    pub file_id: Option<i64>,
    #[serde(default)]
    pub download_count: Option<u64>,
    #[serde(default)]
    pub hearing_impaired: bool,
    #[serde(default)]
    pub hash_matched: bool,
    pub score: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleSearchResponse {
    pub ok: bool,
    pub candidates: Vec<SubtitleCandidate>,
    #[serde(default)]
    pub file_hash: Option<String>,
    #[serde(default)]
    pub providers_queried: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleDownloadRequest {
    pub candidate: SubtitleCandidate,
    #[serde(default)]
    pub imdb_id: Option<String>,
    #[serde(default)]
    pub local_video_path: Option<String>,
    #[serde(default)]
    pub save_alongside_local: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleDownloadResult {
    pub ok: bool,
    #[serde(default)]
    pub local_path: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// Normalize ISO-639-1 / ISO-639-2 / English names to canonical 3-letter ISO-639-2 codes.
pub fn normalize_language_code(raw: &str) -> String {
    let lower = raw.trim().to_ascii_lowercase();
    match lower.as_str() {
        "en" | "eng" | "english" => "eng".to_string(),
        "ar" | "ara" | "arabic" => "ara".to_string(),
        "es" | "spa" | "spanish" => "spa".to_string(),
        "fr" | "fre" | "fra" | "french" => "fre".to_string(),
        "de" | "ger" | "deu" | "german" => "ger".to_string(),
        "it" | "ita" | "italian" => "ita".to_string(),
        "pt" | "por" | "portuguese" => "por".to_string(),
        "pob" | "pt-br" | "pt_br" => "pob".to_string(),
        "ja" | "jpn" | "japanese" => "jpn".to_string(),
        "ko" | "kor" | "korean" => "kor".to_string(),
        "zh" | "chi" | "zho" | "chinese" => "chi".to_string(),
        "ru" | "rus" | "russian" => "rus".to_string(),
        "tr" | "tur" | "turkish" => "tur".to_string(),
        "hi" | "hin" | "hindi" => "hin".to_string(),
        "pl" | "pol" | "polish" => "pol".to_string(),
        "nl" | "dut" | "nld" | "dutch" => "dut".to_string(),
        "any" | "all" | "" => "any".to_string(),
        other => other.to_string(),
    }
}

pub fn language_label(code: &str) -> String {
    match normalize_language_code(code).as_str() {
        "eng" => "English".to_string(),
        "ara" => "Arabic".to_string(),
        "spa" => "Spanish".to_string(),
        "fre" => "French".to_string(),
        "ger" => "German".to_string(),
        "ita" => "Italian".to_string(),
        "por" => "Portuguese".to_string(),
        "pob" => "Portuguese (Brazil)".to_string(),
        "jpn" => "Japanese".to_string(),
        "kor" => "Korean".to_string(),
        "chi" => "Chinese".to_string(),
        "rus" => "Russian".to_string(),
        "tur" => "Turkish".to_string(),
        "hin" => "Hindi".to_string(),
        "pol" => "Polish".to_string(),
        "dut" => "Dutch".to_string(),
        other => other.to_uppercase(),
    }
}

pub fn iso639_2_to_1(code: &str) -> &'static str {
    match normalize_language_code(code).as_str() {
        "eng" => "en",
        "ara" => "ar",
        "spa" => "es",
        "fre" => "fr",
        "ger" => "de",
        "ita" => "it",
        "por" | "pob" => "pt",
        "jpn" => "ja",
        "kor" => "ko",
        "chi" => "zh",
        "rus" => "ru",
        "tur" => "tr",
        "hin" => "hi",
        "pol" => "pl",
        "dut" => "nl",
        _ => "en",
    }
}

/// Compute the standard 64-bit OpenSubtitles media file hash (`size + sum_u64_le(first 64KiB) + sum_u64_le(last 64KiB)`).
pub fn compute_opensubtitles_hash(path: &Path) -> Result<(String, u64), String> {
    let mut file = File::open(path).map_err(|e| format!("Failed to open file for hashing: {e}"))?;
    let file_size = file
        .metadata()
        .map_err(|e| format!("Failed to read file metadata: {e}"))?
        .len();
    const CHUNK: usize = 65536;
    if file_size < CHUNK as u64 {
        return Err("File is smaller than 64 KiB; cannot compute OpenSubtitles hash.".to_string());
    }

    let mut hash: u64 = file_size;
    let mut buf = vec![0u8; CHUNK];

    file.seek(SeekFrom::Start(0))
        .map_err(|e| format!("Seek start failed: {e}"))?;
    file.read_exact(&mut buf)
        .map_err(|e| format!("Read head failed: {e}"))?;
    for chunk in buf.chunks_exact(8) {
        let word = u64::from_le_bytes(chunk.try_into().unwrap());
        hash = hash.wrapping_add(word);
    }

    file.seek(SeekFrom::Start(file_size - CHUNK as u64))
        .map_err(|e| format!("Seek tail failed: {e}"))?;
    file.read_exact(&mut buf)
        .map_err(|e| format!("Read tail failed: {e}"))?;
    for chunk in buf.chunks_exact(8) {
        let word = u64::from_le_bytes(chunk.try_into().unwrap());
        hash = hash.wrapping_add(word);
    }

    Ok((format!("{hash:016x}"), file_size))
}

/// Score a subtitle candidate against preferred language and active release name tokens.
pub fn score_subtitle_candidate(
    cand: &SubtitleCandidate,
    preferred_lang: &str,
    release_name: Option<&str>,
) -> i64 {
    let mut score: i64 = 0;
    let pref = normalize_language_code(preferred_lang);
    let cand_lang = normalize_language_code(&cand.language);

    if pref != "any" && cand_lang == pref {
        score += 500;
    } else if cand_lang == "eng" {
        score += 120;
    }

    if cand.hash_matched {
        score += 1000;
    }

    // Prefer ASS/SSA slightly for anime styling, otherwise SRT
    match cand.format.as_str() {
        "ass" | "ssa" => score += 40,
        "srt" => score += 35,
        "vtt" => score += 20,
        _ => {}
    }

    if !cand.hearing_impaired {
        score += 15;
    }

    if let Some(dl) = cand.download_count {
        score += (dl.min(10_000) / 100) as i64;
    }

    // Release compatibility token overlap (resolution, source, group, codec)
    if let Some(rel) = release_name {
        let rel_tokens: Vec<String> = rel
            .to_ascii_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|t| t.len() >= 3)
            .map(str::to_string)
            .collect();
        let sub_lower = cand.release_name.to_ascii_lowercase();
        let mut overlap = 0i64;
        for tok in &rel_tokens {
            if sub_lower.contains(tok.as_str()) {
                overlap += 35;
            }
        }
        score += overlap.min(350);
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_deterministic_opensubtitles_hash() {
        let tmp = std::env::temp_dir().join("sf-os-hash-test.bin");
        let data = vec![0x42u8; 131_072];
        std::fs::write(&tmp, &data).unwrap();
        let (h1, sz1) = compute_opensubtitles_hash(&tmp).unwrap();
        let (h2, sz2) = compute_opensubtitles_hash(&tmp).unwrap();
        assert_eq!(sz1, 131_072);
        assert_eq!(sz1, sz2);
        assert_eq!(h1.len(), 16);
        assert_eq!(h1, h2);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn ranks_hash_matched_and_release_compatible_subtitles_first() {
        let c1 = SubtitleCandidate {
            id: "1".into(),
            provider: "opensubtitles-addon".into(),
            language: "eng".into(),
            language_label: "English".into(),
            release_name: "Death.Note.S01E01.1080p.BluRay.Judas".into(),
            format: "ass".into(),
            download_url: "https://example.test/1.ass".into(),
            file_id: None,
            download_count: Some(500),
            hearing_impaired: false,
            hash_matched: false,
            score: 0,
        };
        let c2 = SubtitleCandidate {
            id: "2".into(),
            provider: "opensubtitles-api".into(),
            language: "eng".into(),
            language_label: "English".into(),
            release_name: "Other.Release".into(),
            format: "srt".into(),
            download_url: "https://example.test/2.srt".into(),
            file_id: None,
            download_count: Some(10),
            hearing_impaired: false,
            hash_matched: true,
            score: 0,
        };
        let s1 = score_subtitle_candidate(&c1, "eng", Some("[Judas] Death Note S01E01 1080p BluRay"));
        let s2 = score_subtitle_candidate(&c2, "eng", Some("[Judas] Death Note S01E01 1080p BluRay"));
        assert!(s2 > s1);
        assert!(s1 > 600);
    }
}
