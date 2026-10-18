//! Wspólny klient HTTP z limitem zapytań i ponawianiem.

use once_cell::sync::Lazy;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

pub const USER_AGENT: &str = concat!(
    "OpenMusicTagger/",
    env!("CARGO_PKG_VERSION"),
    " ( https://github.com/BlackSingularities/MusicTagger )"
);

pub static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(25))
        .connect_timeout(Duration::from_secs(10))
        .gzip(true)
        .build()
        .expect("klient HTTP")
});

/// Prosty limiter – minimalny odstęp między zapytaniami do danego serwisu
/// oraz możliwość czasowego „wyłączenia” serwisu (np. po HTTP 403/429).
pub struct Limiter {
    state: Mutex<HashMap<&'static str, (Instant, Option<Instant>)>>,
}

pub static LIMITER: Lazy<Limiter> = Lazy::new(|| Limiter { state: Mutex::new(HashMap::new()) });

fn interval(service: &str) -> Duration {
    match service {
        "musicbrainz" => Duration::from_millis(1050),
        "acoustid" => Duration::from_millis(340),
        "itunes" => Duration::from_millis(3100),
        "deezer" => Duration::from_millis(120),
        "lrclib" => Duration::from_millis(100),
        "netease" => Duration::from_millis(250),
        "genius" => Duration::from_millis(400),
        "tekstowo" => Duration::from_millis(500),
        "lyricsovh" => Duration::from_millis(200),
        "caa" => Duration::from_millis(50),
        _ => Duration::from_millis(50),
    }
}

impl Limiter {
    /// Czeka na swoją kolej. Zwraca `false`, jeśli serwis jest chwilowo zablokowany.
    pub async fn acquire(&self, service: &'static str) -> bool {
        let wait = {
            let mut st = self.state.lock().await;
            let now = Instant::now();
            let entry = st.entry(service).or_insert((now - interval(service), None));
            if let Some(until) = entry.1 {
                if now < until {
                    return false;
                }
                entry.1 = None;
            }
            let next = entry.0 + interval(service);
            if next <= now {
                entry.0 = now;
                None
            } else {
                // Rezerwujemy slot i czekamy poza blokadą.
                entry.0 = next;
                Some(next - now)
            }
        };
        if let Some(d) = wait {
            tokio::time::sleep(d).await;
        }
        true
    }

    pub async fn block(&self, service: &'static str, dur: Duration) {
        let mut st = self.state.lock().await;
        let now = Instant::now();
        let entry = st.entry(service).or_insert((now, None));
        entry.1 = Some(now + dur);
        log::warn!("{service}: limit zapytań – wstrzymano na {}s", dur.as_secs());
    }

    pub async fn is_blocked(&self, service: &'static str) -> bool {
        let st = self.state.lock().await;
        st.get(service).and_then(|e| e.1).map(|u| Instant::now() < u).unwrap_or(false)
    }
}

#[derive(Debug)]
pub enum HttpError {
    NotFound,
    RateLimited,
    /// Brak połączenia (DNS, odmowa, timeout) – nie ma sensu ponawiać wielokrotnie.
    Network(String),
    Other(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::NotFound => write!(f, "nie znaleziono"),
            HttpError::RateLimited => write!(f, "limit zapytań"),
            HttpError::Network(s) => write!(f, "brak połączenia z {s}"),
            HttpError::Other(s) => write!(f, "{s}"),
        }
    }
}

/// Serwisy, z którymi chwilowo nie ma połączenia – kolejne zapytania kończą się od razu.
static OFFLINE: Lazy<std::sync::Mutex<HashMap<&'static str, Instant>>> = Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

fn offline_now(service: &'static str) -> bool {
    OFFLINE.lock().unwrap().get(service).map(|t| Instant::now() < *t).unwrap_or(false)
}

fn mark_offline(service: &'static str) {
    OFFLINE.lock().unwrap().insert(service, Instant::now() + Duration::from_secs(20));
}

/// GET z limiterem, ponawianiem przy 503/błędach sieci i parsowaniem JSON.
pub async fn get_json<T: DeserializeOwned>(service: &'static str, url: &str) -> Result<T, HttpError> {
    get_json_h(service, url, &[]).await
}

/// Jak [`get_json`], z dodatkowymi nagłówkami (np. Referer).
pub async fn get_json_h<T: DeserializeOwned>(service: &'static str, url: &str, headers: &[(&str, &str)]) -> Result<T, HttpError> {
    let text = get_text_h(service, url, "application/json", headers).await?;
    serde_json::from_str::<T>(&text).map_err(|e| HttpError::Other(format!("{service}: {e}")))
}

/// Pobiera stronę HTML jako tekst (dla serwisów bez API).
pub async fn get_html(service: &'static str, url: &str) -> Result<String, HttpError> {
    get_text_h(
        service,
        url,
        "text/html,application/xhtml+xml",
        &[("User-Agent", BROWSER_UA), ("Accept-Language", "pl,en;q=0.8")],
    )
    .await
}

/// Niektóre serwisy z tekstami odrzucają nietypowe nazwy klientów.
pub const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

async fn get_text_h(service: &'static str, url: &str, accept: &str, headers: &[(&str, &str)]) -> Result<String, HttpError> {
    if offline_now(service) {
        return Err(HttpError::Network(service.to_string()));
    }
    let mut last = String::new();
    for attempt in 0..3u32 {
        if !LIMITER.acquire(service).await {
            return Err(HttpError::RateLimited);
        }
        let mut req = CLIENT.get(url).header("Accept", accept);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    return resp.text().await.map_err(|e| HttpError::Other(format!("{service}: {e}")));
                }
                match status.as_u16() {
                    404 => return Err(HttpError::NotFound),
                    403 | 429 if matches!(service, "itunes" | "genius" | "tekstowo" | "netease") => {
                        LIMITER.block(service, Duration::from_secs(90)).await;
                        return Err(HttpError::RateLimited);
                    }
                    429 | 503 | 502 | 500 => {
                        last = format!("{service}: HTTP {status}");
                        tokio::time::sleep(Duration::from_millis(1200 * (attempt as u64 + 1))).await;
                    }
                    _ => return Err(HttpError::Other(format!("{service}: HTTP {status}"))),
                }
            }
            Err(e) => {
                if (e.is_connect() || e.is_timeout()) && attempt >= 1 {
                    mark_offline(service);
                    return Err(HttpError::Network(service.to_string()));
                }
                last = format!("{service}: {e}");
                tokio::time::sleep(Duration::from_millis(500 * (attempt as u64 + 1))).await;
            }
        }
    }
    Err(HttpError::Other(last))
}

/// Pobiera surowe bajty (np. okładkę).
pub async fn get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let service = if url.contains("coverartarchive") { "caa" } else { "img" };
    LIMITER.acquire(service).await;
    let mut last = String::new();
    for attempt in 0..3u32 {
        match CLIENT.get(url).send().await {
            Ok(r) if r.status().is_success() => {
                return r.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string());
            }
            Ok(r) if r.status().as_u16() == 404 => return Err("404".into()),
            Ok(r) => last = format!("HTTP {}", r.status()),
            Err(e) => last = e.to_string(),
        }
        tokio::time::sleep(Duration::from_millis(700 * (attempt as u64 + 1))).await;
    }
    Err(last)
}

/// Escaping dla zapytań Lucene (MusicBrainz).
pub fn lucene_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "+-&|!(){}[]^\"~*?:\\/".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
