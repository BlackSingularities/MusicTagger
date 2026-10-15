//! lyrics.ovh – prosty, darmowy serwis z tekstami (bez czasów); ostatnia deska ratunku.

use super::http::{get_json, HttpError};
use serde_json::Value;

pub async fn lyrics(artist: &str, title: &str) -> Result<Option<String>, HttpError> {
    let url = format!(
        "https://api.lyrics.ovh/v1/{}/{}",
        urlencoding::encode(artist),
        urlencoding::encode(title)
    );
    let v: Value = get_json("lyricsovh", &url).await?;
    let text = v.get("lyrics").and_then(|x| x.as_str()).unwrap_or("").replace("\r\n", "\n").replace('\r', "\n");
    // Serwis dokleja czasem francuski nagłówek „Paroles de la chanson … par …”.
    let text = match text.split_once('\n') {
        Some((first, rest)) if first.starts_with("Paroles de la chanson") => rest.to_string(),
        _ => text,
    };
    let text = super::html::tidy_lines(&text);
    Ok((!text.trim().is_empty()).then_some(text))
}
