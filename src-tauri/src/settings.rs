use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub language: String,
    pub theme: String,
    pub accent: String,
    pub recent_folders: Vec<String>,
    pub recursive: bool,

    // Źródła
    pub use_musicbrainz: bool,
    pub use_itunes: bool,
    pub use_deezer: bool,
    pub use_lrclib: bool,
    pub use_acoustid: bool,
    pub acoustid_key: String,
    pub itunes_country: String,

    // Zachowanie
    /// "missing" – tylko uzupełnia puste pola, "overwrite" – nadpisuje różniące się.
    pub write_mode: String,
    pub auto_apply: bool,
    pub auto_apply_threshold: f64,
    pub concurrency: usize,
    pub prefer_original_release: bool,
    pub fill_album_artist: bool,
    pub fill_genre: bool,
    pub write_mb_ids: bool,

    // Okładki
    pub fetch_covers: bool,
    pub replace_small_covers: bool,
    pub min_cover_size: u32,
    pub max_cover_size: u32,
    pub cover_quality: u8,
    pub save_folder_jpg: bool,

    // Teksty
    pub fetch_lyrics: bool,
    pub prefer_synced: bool,
    pub embed_lyrics: bool,
    pub write_lrc_file: bool,

    // Porządkowanie nazw
    pub rename_pattern: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "auto".into(),
            theme: "dark".into(),
            accent: "violet".into(),
            recent_folders: vec![],
            recursive: true,
            use_musicbrainz: true,
            use_itunes: true,
            use_deezer: true,
            use_lrclib: true,
            use_acoustid: true,
            acoustid_key: String::new(),
            itunes_country: "PL".into(),
            write_mode: "missing".into(),
            auto_apply: false,
            auto_apply_threshold: 0.9,
            concurrency: 3,
            prefer_original_release: true,
            fill_album_artist: true,
            fill_genre: true,
            write_mb_ids: true,
            fetch_covers: true,
            replace_small_covers: true,
            min_cover_size: 500,
            max_cover_size: 1200,
            cover_quality: 90,
            save_folder_jpg: false,
            fetch_lyrics: true,
            prefer_synced: true,
            embed_lyrics: true,
            write_lrc_file: false,
            rename_pattern: "%artist% - %title%".into(),
        }
    }
}

impl Settings {
    pub fn file(dir: &Path) -> PathBuf {
        dir.join("settings.json")
    }

    pub fn load(dir: &Path) -> Self {
        std::fs::read_to_string(Self::file(dir))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(Self::file(dir), json).map_err(|e| e.to_string())
    }

    pub fn sanitized(mut self) -> Self {
        self.concurrency = self.concurrency.clamp(1, 8);
        self.auto_apply_threshold = self.auto_apply_threshold.clamp(0.5, 1.0);
        self.max_cover_size = self.max_cover_size.clamp(300, 3000);
        self.min_cover_size = self.min_cover_size.clamp(100, self.max_cover_size);
        self.cover_quality = self.cover_quality.clamp(50, 100);
        self.recent_folders.truncate(8);
        self
    }
}
