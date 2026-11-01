//! Aktualizacje: wykrycie rodzaju instalacji i pobranie instalatora z wydania.
//! Na Windows wydawany jest wyłącznie instalator – także starsze wersje przenośne
//! aktualizują się przez niego.

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
        .filter_map(std::env::var_os)
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
        "portable" | "installed" => "setup.exe",
        _ => ".AppImage",
    }
}

async fn download(url: &str, magic: &[u8]) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("OpenMusicTagger/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;
    let r = client.get(url).send().await.map_err(|e| format!("Pobieranie aktualizacji: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("Pobieranie aktualizacji: HTTP {}", r.status()));
    }
    let b = r.bytes().await.map_err(|e| format!("Pobieranie aktualizacji: {e}"))?;
    // Plik wykonywalny Windows zaczyna się od „MZ”, a APK (ZIP) od „PK”.
    if b.len() < 1_000_000 || !b.starts_with(magic) {
        return Err("Pobrany plik aktualizacji jest uszkodzony".into());
    }
    Ok(b.to_vec())
}

fn file_name_of(url: &str) -> Option<String> {
    let name = url.split(['?', '#']).next()?.rsplit('/').next()?;
    let name = urlencoding::decode(name).ok()?.into_owned();
    (!name.is_empty() && !name.contains(['\\', '/', ':'])).then_some(name)
}

/// Pobiera APK do folderu aktualizacji w pamięci podręcznej aplikacji (Android).
/// Instalację uruchamia potem MainActivity przez systemowy instalator pakietów.
pub async fn download_apk(url: &str, cache_dir: &Path) -> Result<PathBuf, String> {
    let data = download(url, b"PK").await?;
    let dir = cache_dir.join("updates");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Zapis aktualizacji: {e}"))?;
    let path = dir.join("OpenMusicTagger.apk");
    std::fs::write(&path, &data).map_err(|e| format!("Zapis aktualizacji: {e}"))?;
    Ok(path)
}

/// Pobiera aktualizację i ją uruchamia. Zwraca ścieżkę programu/instalatora, który trzeba uruchomić
/// po zamknięciu bieżącej wersji.
pub async fn install(url: &str) -> Result<PathBuf, String> {
    let kind = install_kind();
    if kind != "portable" && kind != "installed" {
        return Err("Automatyczna aktualizacja nie jest dostępna na tym systemie".into());
    }
    let data = download(url, b"MZ").await?;
    let path = std::env::temp_dir().join(file_name_of(url).unwrap_or_else(|| "OpenMusicTagger-setup.exe".into()));
    std::fs::write(&path, &data).map_err(|e| format!("Zapis instalatora: {e}"))?;
    Ok(path)
}

/// Usuwa pozostałości po dawnej samodzielnej podmianie wersji przenośnej.
pub fn cleanup_old() {
    let Ok(exe) = std::env::current_exe() else { return };
    let Some(dir) = exe.parent().map(Path::to_path_buf) else { return };
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(4));
        let Ok(rd) = std::fs::read_dir(&dir) else { return };
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_lowercase();
            if ((n.starts_with("openmusictagger") || n.starts_with("musictagger")) && n.ends_with(".exe.old"))
                || n == "openmusictagger.update.tmp"
                || n == "musictagger.update.tmp"
            {
                let _ = std::fs::remove_file(e.path());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_names() {
        assert_eq!(
            file_name_of("https://github.com/o/r/releases/download/v1.0.12/OpenMusicTagger_1.0.12_x64-setup.exe").as_deref(),
            Some("OpenMusicTagger_1.0.12_x64-setup.exe")
        );
        assert!("OpenMusicTagger_1.0.12_x64-setup.exe".ends_with(asset_suffix("portable")));
        assert!("OpenMusicTagger_1.0.12_x64-setup.exe".ends_with(asset_suffix("installed")));
    }
}
