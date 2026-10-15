//! Aktualizacje: wykrycie rodzaju instalacji (instalator / wersja przenośna),
//! wybór właściwego pliku z wydania i samodzielna podmiana programu.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Rodzaj instalacji: "installed" (instalator NSIS/MSI), "portable", "android", "other".
pub fn install_kind() -> &'static str {
    if cfg!(target_os = "android") {
        return "android";
    }
    if !cfg!(windows) {
        return "other";
    }
    let Ok(exe) = std::env::current_exe() else { return "installed" };
    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    // Instalator NSIS zostawia deinstalator obok programu, MSI instaluje do Program Files.
    if dir.join("uninstall.exe").exists() || dir.join("Uninstall.exe").exists() {
        return "installed";
    }
    let in_program_files = ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
        .iter()
        .filter_map(|k| std::env::var_os(k))
        .any(|pf| exe.starts_with(PathBuf::from(pf)));
    if in_program_files {
        "installed"
    } else {
        "portable"
    }
}

/// Końcówka nazwy pliku w wydaniu odpowiadająca rodzajowi instalacji.
pub fn asset_suffix(kind: &str) -> &'static str {
    match kind {
        "android" => ".apk",
        "portable" => "_portable.exe",
        "installed" => "setup.exe",
        _ => ".AppImage",
    }
}

async fn download(url: &str) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("MusicTagger/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;
    let r = client.get(url).send().await.map_err(|e| format!("Pobieranie aktualizacji: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("Pobieranie aktualizacji: HTTP {}", r.status()));
    }
    let b = r.bytes().await.map_err(|e| format!("Pobieranie aktualizacji: {e}"))?;
    // Plik wykonywalny Windows zaczyna się od „MZ”.
    if b.len() < 1_000_000 || !b.starts_with(b"MZ") {
        return Err("Pobrany plik aktualizacji jest uszkodzony".into());
    }
    Ok(b.to_vec())
}

fn file_name_of(url: &str) -> Option<String> {
    let name = url.split(['?', '#']).next()?.rsplit('/').next()?;
    let name = urlencoding::decode(name).ok()?.into_owned();
    (!name.is_empty() && !name.contains(['\\', '/', ':'])).then_some(name)
}

/// Kopia zapasowa starej wersji przenośnej (usuwana przy następnym uruchomieniu).
const OLD_EXT: &str = "exe.old";

/// Podmienia uruchomiony plik wersji przenośnej na nowy i zwraca ścieżkę do uruchomienia.
/// Windows pozwala zmienić nazwę działającego programu, więc stary plik odkładamy na bok.
fn replace_portable(exe: &Path, data: &[u8], new_name: Option<String>) -> Result<PathBuf, String> {
    let dir = exe.parent().ok_or("Nieznany folder programu")?;
    let tmp = dir.join("MusicTagger.update.tmp");
    std::fs::write(&tmp, data).map_err(|e| format!("Brak zapisu w folderze programu ({}): {e}", dir.display()))?;
    // Nazwa z numerem wersji („MusicTagger_1.0.9_portable.exe”) – zmieniamy na nową; inna nazwa zostaje.
    let cur_name = exe.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let versioned = cur_name.to_lowercase().starts_with("musictagger_") && cur_name.to_lowercase().ends_with("_portable.exe");
    let target = match new_name {
        Some(n) if versioned => dir.join(n),
        _ => exe.to_path_buf(),
    };
    let backup = exe.with_extension(OLD_EXT);
    let _ = std::fs::remove_file(&backup);
    if let Err(e) = std::fs::rename(exe, &backup) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Nie można podmienić programu: {e}"));
    }
    if target != exe && target.exists() {
        let _ = std::fs::remove_file(&target);
    }
    if let Err(e) = std::fs::rename(&tmp, &target) {
        let _ = std::fs::rename(&backup, exe);
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Nie można podmienić programu: {e}"));
    }
    Ok(target)
}

/// Pobiera aktualizację i ją uruchamia. Zwraca ścieżkę programu/instalatora, który trzeba uruchomić
/// po zamknięciu bieżącej wersji.
pub async fn install(url: &str) -> Result<PathBuf, String> {
    let kind = install_kind();
    if kind != "portable" && kind != "installed" {
        return Err("Automatyczna aktualizacja nie jest dostępna na tym systemie".into());
    }
    let data = download(url).await?;
    let name = file_name_of(url);
    if kind == "portable" {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        return replace_portable(&exe, &data, name);
    }
    let path = std::env::temp_dir().join(name.unwrap_or_else(|| "MusicTagger-setup.exe".into()));
    std::fs::write(&path, &data).map_err(|e| format!("Zapis instalatora: {e}"))?;
    Ok(path)
}

/// Usuwa kopię poprzedniej wersji przenośnej (gdy stary proces już się zamknął).
pub fn cleanup_old() {
    let Ok(exe) = std::env::current_exe() else { return };
    let Some(dir) = exe.parent().map(Path::to_path_buf) else { return };
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(4));
        let Ok(rd) = std::fs::read_dir(&dir) else { return };
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_lowercase();
            if n.starts_with("musictagger") && n.ends_with(".exe.old") || n == "musictagger.update.tmp" {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let own = exe.with_extension(OLD_EXT);
        let _ = std::fs::remove_file(own);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_names() {
        assert_eq!(
            file_name_of("https://github.com/o/r/releases/download/v1.0.12/MusicTagger_1.0.12_portable.exe").as_deref(),
            Some("MusicTagger_1.0.12_portable.exe")
        );
        assert!("MusicTagger_1.0.12_portable.exe".ends_with(asset_suffix("portable")));
        assert!("MusicTagger_1.0.12_x64-setup.exe".ends_with(asset_suffix("installed")));
    }

    #[test]
    fn portable_swap() {
        let dir = std::env::temp_dir().join(format!("mt-upd-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("MusicTagger_1.0.1_portable.exe");
        std::fs::write(&exe, b"old").unwrap();
        let new = replace_portable(&exe, b"new", Some("MusicTagger_1.0.2_portable.exe".into())).unwrap();
        assert_eq!(new, dir.join("MusicTagger_1.0.2_portable.exe"));
        assert_eq!(std::fs::read(&new).unwrap(), b"new");
        assert!(!exe.exists());
        assert_eq!(std::fs::read(exe.with_extension(OLD_EXT)).unwrap(), b"old");

        let plain = dir.join("tagger.exe");
        std::fs::write(&plain, b"old").unwrap();
        let p = replace_portable(&plain, b"new", Some("MusicTagger_1.0.2_portable.exe".into())).unwrap();
        assert_eq!(p, plain);
        assert_eq!(std::fs::read(&plain).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
