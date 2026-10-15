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
    pub use_netease: bool,
    pub use_genius: bool,
    pub use_tekstowo: bool,
    pub use_lyricsovh: bool,
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
    /// Łączenie źródeł: braki (pola, okładka, tekst) uzupełniane z innych serwisów.
    pub mix_sources: bool,

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
    /// Tworzone pliki .lrc są ukryte (Windows).
    pub hide_lrc_files: bool,
    /// Wykryte pliki .lrc są przenoszone do środka pliku muzycznego.
    pub merge_lrc_files: bool,
    /// Co zrobić z plikiem .lrc po przeniesieniu: "keep", "hide", "delete".
    pub lrc_after_merge: String,

    // Porządkowanie nazw
    pub rename_pattern: String,

    /// Wersja schematu ustawień (do jednorazowych migracji). Brak w pliku = 0.
    #[serde(default)]
    pub settings_rev: u32,
}

pub const MAX_CONCURRENCY: usize = 8;
const SETTINGS_REV: u32 = 2;

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
            use_netease: true,
            use_genius: true,
            use_tekstowo: true,
            use_lyricsovh: true,
            use_acoustid: true,
            acoustid_key: String::new(),
            itunes_country: "PL".into(),
            write_mode: "missing".into(),
            auto_apply: false,
            auto_apply_threshold: 0.9,
            concurrency: MAX_CONCURRENCY,
            prefer_original_release: true,
            fill_album_artist: true,
            fill_genre: true,
            write_mb_ids: true,
            mix_sources: true,
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
            hide_lrc_files: true,
            merge_lrc_files: true,
            lrc_after_merge: "hide".into(),
            rename_pattern: "%artist% - %title%".into(),
            settings_rev: SETTINGS_REV,
        }
    }
}

impl Settings {
    pub fn file(dir: &Path) -> PathBuf {
        dir.join("settings.json")
    }

    pub fn load(dir: &Path) -> Self {
        let s: Self = std::fs::read_to_string(Self::file(dir))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        s.migrated()
    }

    /// Jednorazowe zmiany domyślnych wartości dla starszych ustawień.
    fn migrated(mut self) -> Self {
        if self.settings_rev < 2 {
            // Od wersji 2: maksymalna szybkość domyślnie, teksty w pliku zamiast w .lrc.
            self.concurrency = MAX_CONCURRENCY;
            self.mix_sources = true;
        }
        self.settings_rev = SETTINGS_REV;
        self
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(Self::file(dir), json).map_err(|e| e.to_string())
    }

    pub fn sanitized(mut self) -> Self {
        self.concurrency = self.concurrency.clamp(1, MAX_CONCURRENCY);
        if !matches!(self.lrc_after_merge.as_str(), "keep" | "hide" | "delete") {
            self.lrc_after_merge = "hide".into();
        }
        self.auto_apply_threshold = self.auto_apply_threshold.clamp(0.5, 1.0);
        self.max_cover_size = self.max_cover_size.clamp(300, 3000);
        self.min_cover_size = self.min_cover_size.clamp(100, self.max_cover_size);
        self.cover_quality = self.cover_quality.clamp(50, 100);
        self.recent_folders.truncate(8);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_old_settings() {
        let old: Settings = serde_json::from_str(r#"{"concurrency":3,"mixSources":false}"#).unwrap();
        let m = old.migrated();
        assert_eq!(m.concurrency, MAX_CONCURRENCY);
        assert!(m.mix_sources && m.use_netease);
        let cur: Settings = serde_json::from_str(r#"{"concurrency":2,"settingsRev":2}"#).unwrap();
        assert_eq!(cur.migrated().concurrency, 2);
    }
}
