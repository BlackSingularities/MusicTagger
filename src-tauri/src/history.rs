//! Historia zmian – każda operacja zapisuje migawkę pliku sprzed zmiany,
//! dzięki czemu można ją cofnąć (pojedynczo lub całą partią).

use crate::tags::{self, Snapshot};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_ENTRIES: usize = 5000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub batch: String,
    pub time: i64,
    pub path: String,
    /// Ścieżka przed zmianą nazwy (jeśli była).
    pub old_path: Option<String>,
    pub action: String,
    pub summary: Vec<String>,
    pub undone: bool,
}

pub struct History {
    dir: PathBuf,
    entries: Mutex<Vec<Entry>>,
}

impl History {
    pub fn open(data_dir: &Path) -> Self {
        let dir = data_dir.join("history");
        let _ = std::fs::create_dir_all(&dir);
        let entries = std::fs::read_to_string(dir.join("index.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { dir, entries: Mutex::new(entries) }
    }

    fn persist(&self, entries: &[Entry]) {
        if let Ok(s) = serde_json::to_string(entries) {
            let tmp = self.dir.join("index.json.tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(tmp, self.dir.join("index.json"));
            }
        }
    }

    /// Zapamiętuje stan pliku przed zmianą. Zwraca id wpisu.
    pub fn record(&self, batch: &str, path: &Path, action: &str, summary: Vec<String>) -> Option<String> {
        let snap = tags::snapshot(path)?;
        let id = uuid::Uuid::new_v4().to_string();
        let json = serde_json::to_vec(&snap).ok()?;
        std::fs::write(self.dir.join(format!("{id}.json")), json).ok()?;
        let entry = Entry {
            id: id.clone(),
            batch: batch.to_string(),
            time: chrono::Utc::now().timestamp(),
            path: path.to_string_lossy().to_string(),
            old_path: None,
            action: action.to_string(),
            summary,
            undone: false,
        };
        let mut e = self.entries.lock().unwrap();
        e.push(entry);
        if e.len() > MAX_ENTRIES {
            let excess = e.len() - MAX_ENTRIES;
            let drop: Vec<Entry> = e.drain(..excess).collect();
            for d in drop {
                let _ = std::fs::remove_file(self.dir.join(format!("{}.json", d.id)));
            }
        }
        self.persist(&e);
        Some(id)
    }

    pub fn record_rename(&self, batch: &str, old: &Path, new: &Path) {
        let mut e = self.entries.lock().unwrap();
        e.push(Entry {
            id: uuid::Uuid::new_v4().to_string(),
            batch: batch.to_string(),
            time: chrono::Utc::now().timestamp(),
            path: new.to_string_lossy().to_string(),
            old_path: Some(old.to_string_lossy().to_string()),
            action: "rename".into(),
            summary: vec![format!(
                "{} → {}",
                old.file_name().unwrap_or_default().to_string_lossy(),
                new.file_name().unwrap_or_default().to_string_lossy()
            )],
            undone: false,
        });
        self.persist(&e);
    }

    /// Po zmianie nazwy pliku aktualizuje ścieżki we wcześniejszych wpisach.
    pub fn repath(&self, old: &str, new: &str) {
        let mut e = self.entries.lock().unwrap();
        for x in e.iter_mut().filter(|x| x.path == old && x.action != "rename") {
            x.path = new.to_string();
        }
        self.persist(&e);
    }

    pub fn list(&self) -> Vec<Entry> {
        let e = self.entries.lock().unwrap();
        e.iter().rev().take(1500).cloned().collect()
    }

    /// Cofa wpisy (od najnowszego). Zwraca listę dotkniętych ścieżek (stara, nowa).
    pub fn undo(&self, ids: &[String]) -> Result<Vec<(String, String)>, String> {
        // Najnowsze najpierw (kolejność w indeksie = kolejność wykonania).
        let ordered: Vec<Entry> = {
            let e = self.entries.lock().unwrap();
            e.iter()
                .rev()
                .filter(|x| !x.undone && (ids.contains(&x.id) || ids.contains(&x.batch)))
                .cloned()
                .collect()
        };
        let mut touched = vec![];
        let mut errors = vec![];
        for entry in &ordered {
            let res = if entry.action == "rename" {
                let old = entry.old_path.clone().unwrap_or_default();
                if Path::new(&old).exists() {
                    Err(format!("Plik już istnieje: {old}"))
                } else {
                    let r = std::fs::rename(&entry.path, &old).map_err(|e| e.to_string());
                    if r.is_ok() {
                        let lrc_new = tags::lrc_path(Path::new(&entry.path));
                        if lrc_new.exists() {
                            let _ = std::fs::rename(lrc_new, tags::lrc_path(Path::new(&old)));
                        }
                        self.repath(&entry.path, &old);
                        touched.push((entry.path.clone(), old.clone()));
                    }
                    r
                }
            } else {
                let snap: Result<Snapshot, String> = std::fs::read(self.dir.join(format!("{}.json", entry.id)))
                    .map_err(|e| e.to_string())
                    .and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string()));
                let path = {
                    // Ścieżka mogła się zmienić po późniejszym przemianowaniu.
                    let e = self.entries.lock().unwrap();
                    e.iter().find(|x| x.id == entry.id).map(|x| x.path.clone()).unwrap_or(entry.path.clone())
                };
                snap.and_then(|s| tags::restore(Path::new(&path), &s)).map(|_| {
                    touched.push((path.clone(), path.clone()));
                })
            };
            match res {
                Ok(()) => {
                    let mut e = self.entries.lock().unwrap();
                    if let Some(x) = e.iter_mut().find(|x| x.id == entry.id) {
                        x.undone = true;
                    }
                    self.persist(&e);
                }
                Err(err) => errors.push(format!("{}: {err}", entry.path)),
            }
        }
        if !errors.is_empty() && touched.is_empty() {
            return Err(errors.join("\n"));
        }
        Ok(touched)
    }

    pub fn clear(&self) {
        let mut e = self.entries.lock().unwrap();
        for x in e.iter() {
            let _ = std::fs::remove_file(self.dir.join(format!("{}.json", x.id)));
        }
        e.clear();
        self.persist(&e);
    }
}
