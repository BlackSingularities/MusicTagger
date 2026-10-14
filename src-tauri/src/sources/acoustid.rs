//! AcoustID – rozpoznawanie utworu po „odcisku” dźwięku (gdy brak tagów i sensownej nazwy).

use super::http::{get_json, HttpError};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct AcoustMatch {
    pub score: f64,
    pub recording_ids: Vec<String>,
}

pub async fn lookup(key: &str, fingerprint: &str, duration: f64) -> Result<Vec<AcoustMatch>, HttpError> {
    let url = format!(
        "https://api.acoustid.org/v2/lookup?client={}&meta=recordingids&duration={}&fingerprint={}",
        urlencoding::encode(key),
        duration.round() as u64,
        fingerprint
    );
    let v: Value = get_json("acoustid", &url).await?;
    if v.get("status").and_then(|s| s.as_str()) != Some("ok") {
        let msg = v
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("błąd AcoustID")
            .to_string();
        return Err(HttpError::Other(format!("AcoustID: {msg}")));
    }
    let mut out = vec![];
    for r in v.get("results").and_then(|r| r.as_array()).cloned().unwrap_or_default() {
        let score = r.get("score").and_then(|s| s.as_f64()).unwrap_or(0.0);
        let ids: Vec<String> = r
            .get("recordings")
            .and_then(|r| r.as_array())
            .map(|a| a.iter().filter_map(|x| x.get("id").and_then(|i| i.as_str()).map(String::from)).collect())
            .unwrap_or_default();
        if !ids.is_empty() {
            out.push(AcoustMatch { score, recording_ids: ids });
        }
    }
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    Ok(out)
}
