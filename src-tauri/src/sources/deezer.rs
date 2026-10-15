//! Deezer – szeroki katalog (także polska muzyka), okładki 1000+ px, ISRC, BPM.

use super::http::{get_json, HttpError};
use crate::models::{Candidate, TagFields};
use serde_json::Value;

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|x| x.trim().to_string()).filter(|x| !x.is_empty())
}

pub async fn search(artist: Option<&str>, title: &str) -> Result<Vec<(Candidate, u64)>, HttpError> {
    let q = match artist.filter(|a| !a.is_empty()) {
        Some(a) => format!("artist:\"{a}\" track:\"{title}\""),
        None => title.to_string(),
    };
    let url = format!("https://api.deezer.com/search?q={}&limit=12", urlencoding::encode(&q));
    let v: Value = get_json("deezer", &url).await?;
    let mut data = v.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default();
    if data.is_empty() && artist.is_some() {
        // Luźniejsze zapytanie.
        let url = format!(
            "https://api.deezer.com/search?q={}&limit=12",
            urlencoding::encode(&format!("{} {}", artist.unwrap_or(""), title))
        );
        let v: Value = get_json("deezer", &url).await?;
        data = v.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default();
    }
    Ok(data
        .iter()
        .map(|r| {
            let album = r.get("album").cloned().unwrap_or(Value::Null);
            let cover = s(&album, "cover_xl");
            let id = r.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
            (
                Candidate {
                    source: "Deezer".into(),
                    fields: TagFields {
                        title: s(r, "title"),
                        artist: r.get("artist").and_then(|a| s(a, "name")),
                        album: s(&album, "title"),
                        ..Default::default()
                    },
                    duration: r.get("duration").and_then(|d| d.as_f64()),
                    cover_url: cover.as_ref().map(|c| c.replace("1000x1000", "1400x1400")),
                    cover_thumb: s(&album, "cover_medium"),
                    release_status: Some("Official".into()),
                    ext_id: Some(id),
                    ..Default::default()
                },
                id,
            )
        })
        .collect())
}

/// Uzupełnia kandydata szczegółami utworu i albumu.
pub async fn enrich(c: &mut Candidate) {
    let Some(track_id) = c.ext_id.filter(|i| *i > 0) else { return };
    let Ok(t) = get_json::<Value>("deezer", &format!("https://api.deezer.com/track/{track_id}")).await else {
        return;
    };
    let f = &mut c.fields;
    f.track = t.get("track_position").and_then(|x| x.as_u64()).map(|x| x as u32).or(f.track);
    f.disc = t.get("disk_number").and_then(|x| x.as_u64()).map(|x| x as u32).or(f.disc);
    f.isrc = s(&t, "isrc").or(f.isrc.take());
    f.bpm = t
        .get("bpm")
        .and_then(|x| x.as_f64())
        .filter(|b| *b > 30.0)
        .map(|b| format!("{}", b.round() as u32))
        .or(f.bpm.take());
    f.year = s(&t, "release_date").and_then(|d| d.get(0..4).map(String::from)).or(f.year.take());
    if let Some(contrib) = t.get("contributors").and_then(|c| c.as_array()) {
        let names: Vec<String> = contrib.iter().filter_map(|c| s(c, "name")).collect();
        if names.len() > 1 && f.artist.as_deref() == names.first().map(|s| s.as_str()) {
            f.artist = Some(names.join(", "));
        }
    }
    let album_id = t.get("album").and_then(|a| a.get("id")).and_then(|i| i.as_u64());
    if let Some(aid) = album_id {
        if let Ok(a) = get_json::<Value>("deezer", &format!("https://api.deezer.com/album/{aid}")).await {
            f.album_artist = a.get("artist").and_then(|x| s(x, "name")).or(f.album_artist.take());
            f.label = s(&a, "label").or(f.label.take());
            f.track_total = a.get("nb_tracks").and_then(|x| x.as_u64()).map(|x| x as u32).or(f.track_total);
            f.year = s(&a, "release_date").and_then(|d| d.get(0..4).map(String::from)).or(f.year.take());
            f.genre = a
                .get("genres")
                .and_then(|g| g.get("data"))
                .and_then(|d| d.as_array())
                .and_then(|d| d.first())
                .and_then(|g| s(g, "name"))
                .or(f.genre.take());
            let rt = s(&a, "record_type").unwrap_or_default();
            c.release_type = Some(match rt.as_str() {
                "single" => "Single",
                "ep" => "EP",
                "compile" => "Album",
                _ => "Album",
            }
            .into());
            if rt == "compile" {
                c.secondary_types.push("Compilation".into());
            }
        }
    }
}

/// Wyszukiwanie albumów – do ręcznego wyboru okładki.
pub async fn search_albums(term: &str) -> Result<Vec<crate::models::CoverCandidate>, HttpError> {
    Ok(album_covers(term).await?.into_iter().map(|(_, _, c)| c).collect())
}

/// Albumy z okładkami: (wykonawca, tytuł albumu, okładka).
pub async fn album_covers(term: &str) -> Result<Vec<(String, String, crate::models::CoverCandidate)>, HttpError> {
    let url = format!("https://api.deezer.com/search/album?q={}&limit=12", urlencoding::encode(term));
    let v: Value = get_json("deezer", &url).await?;
    Ok(v.get("data")
        .and_then(|d| d.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    let xl = s(r, "cover_xl")?;
                    let artist = r.get("artist").and_then(|x| s(x, "name")).unwrap_or_default();
                    let album = s(r, "title").unwrap_or_default();
                    let c = crate::models::CoverCandidate {
                        url: xl.replace("1000x1000", "1400x1400"),
                        thumb: s(r, "cover_medium").unwrap_or(xl.clone()),
                        source: format!("Deezer · {artist} – {album}"),
                        width: 1400,
                        height: 1400,
                    };
                    Some((artist, album, c))
                })
                .collect()
        })
        .unwrap_or_default())
}
