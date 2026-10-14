use serde::{Deserialize, Serialize};

/// Zestaw edytowalnych pól tagu. `None` = brak wartości.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TagFields {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<String>,
    pub track: Option<u32>,
    pub track_total: Option<u32>,
    pub disc: Option<u32>,
    pub disc_total: Option<u32>,
    pub genre: Option<String>,
    pub composer: Option<String>,
    pub label: Option<String>,
    pub isrc: Option<String>,
    pub comment: Option<String>,
    pub bpm: Option<String>,
    pub mb_recording_id: Option<String>,
    pub mb_release_id: Option<String>,
}

/// Nazwy pól używane przez interfejs (kolejność wyświetlania).
pub const FIELD_NAMES: &[&str] = &[
    "title",
    "artist",
    "album",
    "albumArtist",
    "year",
    "track",
    "trackTotal",
    "disc",
    "discTotal",
    "genre",
    "composer",
    "label",
    "isrc",
    "comment",
    "bpm",
    "mbRecordingId",
    "mbReleaseId",
];

impl TagFields {
    pub fn get(&self, name: &str) -> Option<String> {
        match name {
            "title" => self.title.clone(),
            "artist" => self.artist.clone(),
            "album" => self.album.clone(),
            "albumArtist" => self.album_artist.clone(),
            "year" => self.year.clone(),
            "track" => self.track.map(|v| v.to_string()),
            "trackTotal" => self.track_total.map(|v| v.to_string()),
            "disc" => self.disc.map(|v| v.to_string()),
            "discTotal" => self.disc_total.map(|v| v.to_string()),
            "genre" => self.genre.clone(),
            "composer" => self.composer.clone(),
            "label" => self.label.clone(),
            "isrc" => self.isrc.clone(),
            "comment" => self.comment.clone(),
            "bpm" => self.bpm.clone(),
            "mbRecordingId" => self.mb_recording_id.clone(),
            "mbReleaseId" => self.mb_release_id.clone(),
            _ => None,
        }
    }

    pub fn set(&mut self, name: &str, value: Option<String>) {
        let value = value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let num = |v: &Option<String>| v.as_ref().and_then(|s| s.split('/').next()?.trim().parse::<u32>().ok());
        match name {
            "title" => self.title = value,
            "artist" => self.artist = value,
            "album" => self.album = value,
            "albumArtist" => self.album_artist = value,
            "year" => self.year = value,
            "track" => self.track = num(&value),
            "trackTotal" => self.track_total = num(&value),
            "disc" => self.disc = num(&value),
            "discTotal" => self.disc_total = num(&value),
            "genre" => self.genre = value,
            "composer" => self.composer = value,
            "label" => self.label = value,
            "isrc" => self.isrc = value,
            "comment" => self.comment = value,
            "bpm" => self.bpm = value,
            "mbRecordingId" => self.mb_recording_id = value,
            "mbReleaseId" => self.mb_release_id = value,
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverInfo {
    pub width: u32,
    pub height: u32,
    pub mime: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    pub path: String,
    pub file_name: String,
    pub folder: String,
    pub format: String,
    pub size: u64,
    pub modified: i64,
    pub duration: f64,
    pub bitrate: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    pub tags: TagFields,
    pub cover: Option<CoverInfo>,
    pub lyrics: Option<String>,
    pub lyrics_synced: bool,
    pub lrc_file: bool,
    pub missing: Vec<String>,
    pub score: u8,
    pub status: TrackStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TrackStatus {
    #[default]
    Idle,
    Queued,
    Working,
    Proposed,
    Applied,
    UpToDate,
    NotFound,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldChange {
    pub field: String,
    pub old: Option<String>,
    pub new: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverCandidate {
    pub url: String,
    pub thumb: String,
    pub source: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsResult {
    pub plain: Option<String>,
    pub synced: Option<String>,
    pub source: String,
    pub instrumental: bool,
}

/// Kandydat dopasowania z dowolnego źródła.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub source: String,
    pub fields: TagFields,
    pub duration: Option<f64>,
    pub cover_url: Option<String>,
    pub cover_thumb: Option<String>,
    /// Release-group MB do pobrania okładki z Cover Art Archive.
    pub mb_release_group: Option<String>,
    pub release_type: Option<String>,
    pub secondary_types: Vec<String>,
    pub release_status: Option<String>,
    pub disambiguation: Option<String>,
    /// Identyfikator w zewnętrznym serwisie (np. Deezer track id).
    pub ext_id: Option<u64>,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub track_id: String,
    pub confidence: f64,
    pub method: String,
    pub changes: Vec<FieldChange>,
    pub covers: Vec<CoverCandidate>,
    pub cover_choice: Option<usize>,
    pub cover_enabled: bool,
    pub lyrics: Option<LyricsResult>,
    pub lyrics_enabled: bool,
    pub alternatives: Vec<Candidate>,
    pub chosen: usize,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub done: usize,
    pub total: usize,
    pub current: String,
    pub phase: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackEvent {
    pub track: Track,
    pub proposal: Option<Proposal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub audio_count: usize,
}
