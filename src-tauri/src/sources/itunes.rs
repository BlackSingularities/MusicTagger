//! iTunes Search API – świetne okładki (do 3000 px) i gatunki.

use super::http::{get_json, HttpError};
use crate::models::{Candidate, TagFields};
use serde_json::Value;

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|x| x.trim().to_string()).filter(|x| !x.is_empty())
}
fn n(v: &Value, k: &str) -> Option<u32> {
    v.get(k).and_then(|x| x.as_u64()).map(|x| x as u32).filter(|x| *x > 0)
}

pub fn clean_album(a: &str) -> String {
    a.trim_end_matches(" - Single").trim_end_matches(" - EP").trim().to_string()
}

pub async fn search(term: &str, country: &str) -> Result<Vec<Candidate>, HttpError> {
    let url = format!(
        "https://itunes.apple.com/search?term={}&media=music&entity=song&limit=12&country={}",
        urlencoding::encode(term),
        if country.is_empty() { "US" } else { country }
    );
    let v: Value = get_json("itunes", &url).await?;
    let mut out = vec![];
    for r in v.get("results").and_then(|r| r.as_array()).cloned().unwrap_or_default() {
        let art = s(&r, "artworkUrl100");
        let album = s(&r, "collectionName");
        let is_single = album.as_deref().map(|a| a.ends_with(" - Single")).unwrap_or(false);
        let is_ep = album.as_deref().map(|a| a.ends_with(" - EP")).unwrap_or(false);
        let compilation = r.get("collectionArtistName").and_then(|x| x.as_str()) == Some("Various Artists");
        out.push(Candidate {
            source: "iTunes".into(),
            fields: TagFields {
                title: s(&r, "trackName"),
                artist: s(&r, "artistName"),
                album: album.as_deref().map(clean_album),
                album_artist: s(&r, "collectionArtistName").or_else(|| s(&r, "artistName")),
                year: s(&r, "releaseDate").and_then(|d| d.get(0..4).map(String::from)),
                track: n(&r, "trackNumber"),
                track_total: n(&r, "trackCount"),
                disc: n(&r, "discNumber"),
                disc_total: n(&r, "discCount"),
                genre: s(&r, "primaryGenreName").filter(|g| g != "Music"),
                ..Default::default()
            },
            duration: r.get("trackTimeMillis").and_then(|x| x.as_f64()).map(|x| x / 1000.0),
            cover_url: art.as_ref().map(|a| a.replace("100x100bb", "1400x1400bb")),
            cover_thumb: art.as_ref().map(|a| a.replace("100x100bb", "300x300bb")),
            release_type: Some(if is_single { "Single" } else if is_ep { "EP" } else { "Album" }.into()),
            secondary_types: if compilation { vec!["Compilation".into()] } else { vec![] },
            release_status: Some("Official".into()),
            ..Default::default()
        });
    }
    Ok(out)
}

/// Wyszukiwanie albumów – do ręcznego wyboru okładki.
pub async fn search_albums(term: &str, country: &str) -> Result<Vec<crate::models::CoverCandidate>, HttpError> {
    let url = format!(
        "https://itunes.apple.com/search?term={}&media=music&entity=album&limit=12&country={}",
        urlencoding::encode(term),
        if country.is_empty() { "US" } else { country }
    );
    let v: Value = get_json("itunes", &url).await?;
    Ok(v.get("results")
        .and_then(|r| r.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    let art = s(r, "artworkUrl100")?;
                    Some(crate::models::CoverCandidate {
                        url: art.replace("100x100bb", "1400x1400bb"),
                        thumb: art.replace("100x100bb", "300x300bb"),
                        source: format!(
                            "iTunes · {} – {}",
                            s(r, "artistName").unwrap_or_default(),
                            clean_album(&s(r, "collectionName").unwrap_or_default())
                        ),
                        width: 1400,
                        height: 1400,
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}
