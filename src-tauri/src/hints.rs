//! Heurystyki: odczyt informacji z nazw plików i folderów, normalizacja tekstu
//! oraz miary podobieństwa używane przy dopasowywaniu.

use once_cell::sync::Lazy;
use regex::Regex;
use std::path::Path;
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hint {
    pub artist: Option<String>,
    pub title: Option<String>,
    pub album: Option<String>,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub year: Option<String>,
}

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: Lazy<Regex> = Lazy::new(|| Regex::new($pat).unwrap());
    };
}

// Szum typowy dla plików z YouTube / serwisów.
re!(
    NOISE_BRACKETS,
    r"(?i)[\(\[\{][^\)\]\}]*(official|video|audio|lyric|lyrics|tekst|teledysk|hd|hq|4k|1080p|720p|visuali[sz]er|clip|klip|mv|explicit|clean|free download|out now|premiere|full album)[^\)\]\}]*[\)\]\}]"
);
re!(
    NOISE_TAIL,
    r"(?i)\s*[-|–]\s*(official (music )?video|official audio|lyrics?( video)?|teledysk|audio|hq|hd)\s*$"
);
re!(YT_ID, r"(?:\s*[\[\(]?\s*|-)([A-Za-z0-9_-]{11})[\]\)]?$");
re!(LEAD_NUM, r"^\s*(?:(?:cd|disc|disk)\s*(\d+)\s*[-_. ]+)?(\d{1,3})(?:\s*[-._)]\s*|\s+)(.+)$");
re!(DISC_DIR, r"(?i)^(?:cd|disc|disk|dysk)\s*[-_ ]?(\d{1,2})\b");
re!(YEAR_IN, r"(?:^|[\s\(\[\-_])((?:19|20)\d{2})(?:$|[\s\)\]\-_])");
re!(YEAR_ALONE, r"^\s*[\(\[]?((?:19|20)\d{2})[\)\]]?\s*$");
re!(
    GARBAGE,
    r"(?i)^(?:track|utw[oó]r|audio ?track|piste|pista|titel|unknown|untitled|nieznany|bez tytu[lł]u|new recording|nagranie|audio|recording|ścieżka|sciezka)?\s*[-_#.]?\s*\d{0,4}$"
);
re!(FEAT, r"(?i)\s*[\(\[]?\s*(?:feat\.?|ft\.?|featuring|with)\s+([^\)\]]+)[\)\]]?");
re!(MULTISPACE, r"\s+");
re!(SPLIT_ARTISTS, r"(?i)\s*(?:,|&|\band\b|\bi\b|\bx\b|\bvs\.?\b|/|;|\+)\s*");

/// Usuwa diakrytyki (w tym ł/Ł) i zamienia na małe litery.
pub fn fold(s: &str) -> String {
    s.nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .map(|c| match c {
            'ł' => 'l',
            'Ł' => 'l',
            'ß' => 's',
            'ø' | 'Ø' => 'o',
            'æ' | 'Æ' => 'a',
            '’' | '`' | '´' => '\'',
            _ => c,
        })
        .collect::<String>()
        .to_lowercase()
}

/// Normalizacja do porównań: bez diakrytyków, interpunkcji, „feat.”, dopisków.
pub fn normalize(s: &str) -> String {
    let s = FEAT.replace_all(s, " ");
    let s = strip_noise(&s);
    let s = fold(&s).replace('&', " and ");
    let s: String = s.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    let s = s
        .split_whitespace()
        .filter(|w| !matches!(*w, "the" | "a" | "an"))
        .collect::<Vec<_>>()
        .join(" ");
    s.trim().to_string()
}

/// Normalizacja tytułu – dodatkowo bez nawiasów (np. „(Remastered 2011)”).
pub fn normalize_title(s: &str) -> String {
    static PAREN: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\(\[][^\)\]]*[\)\]]").unwrap());
    static DASH_SUFFIX: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)\s+-\s+.*(remaster|version|edit|mix|mono|stereo|live|wersja).*$").unwrap());
    let base = DASH_SUFFIX.replace(s, "");
    let base = PAREN.replace_all(&base, " ");
    let n = normalize(&base);
    if n.is_empty() {
        normalize(s)
    } else {
        n
    }
}

pub fn strip_noise(s: &str) -> String {
    let s = NOISE_BRACKETS.replace_all(s, " ");
    let s = NOISE_TAIL.replace_all(&s, "");
    MULTISPACE.replace_all(s.trim(), " ").trim_matches(|c: char| c == '-' || c == '_' || c.is_whitespace()).to_string()
}

pub fn is_garbage_title(s: &str) -> bool {
    let t = s.trim();
    if YEAR_ALONE.is_match(t) && t.len() == 4 {
        return false; // np. „1999” (Prince)
    }
    t.is_empty() || GARBAGE.is_match(t) || t.chars().all(|c| !c.is_alphanumeric())
}

pub fn is_garbage_artist(s: &str) -> bool {
    let f = fold(s.trim());
    f.is_empty()
        || matches!(
            f.as_str(),
            "unknown" | "unknown artist" | "nieznany" | "nieznany wykonawca" | "various" | "artist" | "va" | "<unknown>"
        )
}

pub fn is_garbage_album(s: &str) -> bool {
    let f = fold(s.trim());
    f.is_empty()
        || matches!(f.as_str(), "unknown" | "unknown album" | "nieznany album" | "album" | "music" | "muzyka" | "download" | "downloads" | "pobrane")
}

/// Wersja (live/remix/acoustic…) wykryta w tytule lub dopisku.
pub fn version_tags(s: &str) -> Vec<&'static str> {
    let f = fold(s);
    let mut v = vec![];
    for (k, name) in [
        ("live", "live"),
        ("na zywo", "live"),
        ("koncert", "live"),
        ("remix", "remix"),
        ("rmx", "remix"),
        ("acoustic", "acoustic"),
        ("akustycz", "acoustic"),
        ("unplugged", "acoustic"),
        ("instrumental", "instrumental"),
        ("karaoke", "karaoke"),
        ("cover", "cover"),
        ("demo", "demo"),
        ("radio edit", "edit"),
        ("extended", "extended"),
        ("sped up", "sped"),
        ("slowed", "slowed"),
        ("nightcore", "sped"),
    ] {
        if f.contains(k) && !v.contains(&name) {
            v.push(name);
        }
    }
    v
}

/// Główny wykonawca (bez „feat.” i współwykonawców) – lepszy do wyszukiwania.
pub fn primary_artist(s: &str) -> String {
    let s = FEAT.replace_all(s, "");
    SPLIT_ARTISTS.split(s.trim()).next().unwrap_or("").trim().to_string()
}

pub fn artist_tokens(s: &str) -> Vec<String> {
    let s = FEAT.replace_all(s, ", ");
    SPLIT_ARTISTS.split(&s).map(normalize).filter(|x| !x.is_empty()).collect()
}

/// Podobieństwo 0..1 dwóch tekstów po normalizacji.
pub fn similarity(a: &str, b: &str) -> f64 {
    let (a, b) = (normalize(a), normalize(b));
    sim_norm(&a, &b)
}

pub fn title_similarity(a: &str, b: &str) -> f64 {
    let full = similarity(a, b);
    let core = sim_norm(&normalize_title(a), &normalize_title(b));
    full.max(core * 0.97)
}

pub fn sim_norm(a: &str, b: &str) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    let ac: String = a.chars().filter(|c| !c.is_whitespace()).collect();
    let bc: String = b.chars().filter(|c| !c.is_whitespace()).collect();
    if ac == bc {
        return 0.99;
    }
    let jw = strsim::jaro_winkler(a, b);
    let lev = strsim::normalized_levenshtein(&ac, &bc);
    // Podobieństwo zbiorów słów (odporne na kolejność).
    let ta: std::collections::HashSet<&str> = a.split(' ').collect();
    let tb: std::collections::HashSet<&str> = b.split(' ').collect();
    let inter = ta.intersection(&tb).count() as f64;
    let token = inter / (ta.len().max(tb.len()) as f64);
    let contain = if (ac.len() >= 4 && bc.contains(&ac)) || (bc.len() >= 4 && ac.contains(&bc)) {
        0.88
    } else {
        0.0
    };
    jw.min(lev.max(token)).max(lev).max(token).max(contain).max(jw * 0.92)
}

pub fn artist_similarity(a: &str, b: &str) -> f64 {
    let base = similarity(a, b);
    let ta = artist_tokens(a);
    let tb = artist_tokens(b);
    let mut best: f64 = 0.0;
    for x in &ta {
        for y in &tb {
            best = best.max(sim_norm(x, y));
        }
    }
    // Zgodność głównego wykonawcy liczy się prawie tak jak pełna.
    base.max(best * 0.95)
}

/// Ocena zgodności czasu trwania (1.0 = idealnie, 0 = bardzo różne).
pub fn duration_score(a: f64, b: f64) -> f64 {
    if a <= 0.0 || b <= 0.0 {
        return 0.5;
    }
    let d = (a - b).abs();
    if d <= 2.5 {
        1.0
    } else if d <= 6.0 {
        0.85
    } else if d <= 15.0 {
        0.55
    } else if d <= 40.0 {
        0.2
    } else {
        0.0
    }
}

fn clean_part(s: &str) -> String {
    let s = s.replace('_', " ");
    let s = strip_noise(&s);
    MULTISPACE.replace_all(s.trim(), " ").to_string()
}

fn strip_youtube_id(s: &str) -> String {
    if let Some(c) = YT_ID.captures(s) {
        let id = c.get(1).unwrap().as_str();
        // ID z YouTube ma mieszane litery/cyfry – nie usuwamy zwykłych słów.
        let has_digit = id.chars().any(|c| c.is_ascii_digit());
        let has_upper = id.chars().any(|c| c.is_ascii_uppercase());
        let has_lower = id.chars().any(|c| c.is_ascii_lowercase());
        if (has_digit as u8 + has_upper as u8 + has_lower as u8) >= 2 && (has_digit || id.contains('-') || id.contains('_')) {
            return s[..c.get(0).unwrap().start()].trim().to_string();
        }
    }
    s.to_string()
}

/// Wyciąga wskazówki z nazwy pliku i folderów nadrzędnych.
pub fn parse_path(path: &Path) -> Hint {
    let mut h = Hint::default();
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let mut stem = strip_youtube_id(&stem.replace('_', " "));
    stem = MULTISPACE.replace_all(stem.trim(), " ").to_string();

    if let Some(c) = LEAD_NUM.captures(&stem) {
        let num: u32 = c.get(2).unwrap().as_str().parse().unwrap_or(0);
        let rest = c.get(3).unwrap().as_str();
        // Unikamy pomyłki z tytułami typu „7 Rings” – numer tylko gdy jest separator lub zero wiodące.
        let raw = c.get(0).unwrap().as_str();
        let sep_like = raw.trim_start().len() > c.get(2).unwrap().as_str().len()
            && raw.trim_start()[c.get(2).unwrap().as_str().len()..]
                .trim_start()
                .starts_with(|ch: char| "-._)".contains(ch));
        if (num > 0 && num < 200) && (sep_like || c.get(2).unwrap().as_str().starts_with('0') || rest.contains(" - ")) {
            h.track = Some(num);
            if let Some(d) = c.get(1) {
                h.disc = d.as_str().parse().ok();
            }
            stem = rest.to_string();
        }
    }

    let parts: Vec<&str> = if stem.contains(" - ") {
        stem.split(" - ").collect()
    } else if stem.contains(" – ") {
        stem.split(" – ").collect()
    } else if stem.matches('-').count() == 1 && !stem.contains(' ') {
        stem.split('-').collect()
    } else {
        vec![stem.as_str()]
    };
    let parts: Vec<String> = parts.iter().map(|p| clean_part(p)).filter(|p| !p.is_empty()).collect();
    match parts.len() {
        0 => {}
        1 => h.title = Some(parts[0].clone()),
        2 => {
            h.artist = Some(parts[0].clone());
            h.title = Some(parts[1].clone());
        }
        _ => {
            // „Artist - Album - 05 - Title” lub „Artist - Title - Extra”
            if let Some(idx) = parts.iter().position(|p| p.chars().all(|c| c.is_ascii_digit()) && p.len() <= 3) {
                h.track = h.track.or(parts[idx].parse().ok());
                h.artist = Some(parts[0].clone());
                if idx >= 2 {
                    h.album = Some(parts[1..idx].join(" - "));
                }
                h.title = parts.get(idx + 1).cloned().or_else(|| parts.last().cloned());
            } else {
                h.artist = Some(parts[0].clone());
                h.title = Some(parts[1..].join(" - "));
            }
        }
    }
    if let Some(t) = &h.title {
        // Wiodący numer w tytule po „Artist - 03 Title”.
        if let Some(c) = LEAD_NUM.captures(t) {
            if h.track.is_none() && c.get(2).unwrap().as_str().starts_with('0') {
                h.track = c.get(2).unwrap().as_str().parse().ok();
                h.title = Some(c.get(3).unwrap().as_str().to_string());
            }
        }
    }

    // Foldery: …/Artist/Album (Year)/CD1/file
    let mut dirs: Vec<String> = path
        .ancestors()
        .skip(1)
        .take(3)
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .collect();
    if let Some(first) = dirs.first() {
        if let Some(c) = DISC_DIR.captures(first) {
            h.disc = h.disc.or(c.get(1).and_then(|m| m.as_str().parse().ok()));
            dirs.remove(0);
        }
    }
    if let Some(album_dir) = dirs.first() {
        let mut name = album_dir.replace('_', " ");
        if let Some(c) = YEAR_IN.captures(&name) {
            h.year = Some(c.get(1).unwrap().as_str().to_string());
            let m = c.get(1).unwrap();
            name = format!("{}{}", &name[..m.start()], &name[m.end()..]);
        }
        let name = name.replace("()", "").replace("[]", "");
        let name = clean_part(&name);
        let (art, alb) = match name.split_once(" - ") {
            Some((a, b)) => (Some(a.trim().to_string()), b.trim().to_string()),
            None => (None, name.clone()),
        };
        if !is_garbage_album(&alb) && !YEAR_ALONE.is_match(&alb) && alb.chars().any(|c| c.is_alphabetic()) {
            if h.album.is_none() {
                h.album = Some(alb);
            }
            if h.artist.is_none() {
                if let Some(a) = art {
                    h.artist = Some(a);
                } else if let Some(artist_dir) = dirs.get(1) {
                    let a = clean_part(artist_dir);
                    if !is_garbage_album(&a) && !is_garbage_artist(&a) {
                        h.artist = Some(a);
                    }
                }
            }
        }
    }
    if h.title.as_deref().map(is_garbage_title).unwrap_or(false) {
        h.title = None;
    }
    if h.artist.as_deref().map(is_garbage_artist).unwrap_or(false) {
        h.artist = None;
    }
    h
}

/// Usuwa „feat.” z tytułu i zwraca listę gości.
pub fn split_feat(title: &str) -> (String, Option<String>) {
    if let Some(c) = FEAT.captures(title) {
        let guests = c.get(1).map(|m| m.as_str().trim().to_string());
        let t = FEAT.replace(title, "").trim().to_string();
        return (t, guests);
    }
    (title.to_string(), None)
}

/// Polskie / dowolne znaki diakrytyczne – czy tekst ma „bogatszą” pisownię.
pub fn diacritic_count(s: &str) -> usize {
    s.chars().filter(|c| !c.is_ascii() && c.is_alphabetic()).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> Hint {
        parse_path(&PathBuf::from(s))
    }

    #[test]
    fn parses_common_patterns() {
        let h = p("/m/Queen/A Night at the Opera (1975)/11 - Bohemian Rhapsody.mp3");
        assert_eq!(h.track, Some(11));
        assert_eq!(h.title.as_deref(), Some("Bohemian Rhapsody"));
        assert_eq!(h.album.as_deref(), Some("A Night at the Opera"));
        assert_eq!(h.artist.as_deref(), Some("Queen"));
        assert_eq!(h.year.as_deref(), Some("1975"));

        let h = p("/x/Downloads/Dawid Podsiadło - Małomiasteczkowy (Official Video) [4K].mp3");
        assert_eq!(h.artist.as_deref(), Some("Dawid Podsiadło"));
        assert_eq!(h.title.as_deref(), Some("Małomiasteczkowy"));
        assert_eq!(h.album, None);

        let h = p("/x/Music/Daft Punk - Get Lucky-5NV6Rdv1a3I.m4a");
        assert_eq!(h.title.as_deref(), Some("Get Lucky"));

        let h = p("/x/Music/Track 07.mp3");
        assert_eq!(h.title, None);

        let h = p("/x/Some Artist - Great Album/CD2/03. Song Name.flac");
        assert_eq!(h.disc, Some(2));
        assert_eq!(h.track, Some(3));
        assert_eq!(h.album.as_deref(), Some("Great Album"));
        assert_eq!(h.artist.as_deref(), Some("Some Artist"));

        let h = p("/x/Music/7 Rings.mp3");
        assert_eq!(h.title.as_deref(), Some("7 Rings"));
    }

    #[test]
    fn similarity_works() {
        assert!(similarity("Małomiasteczkowy", "Malomiasteczkowy") > 0.98);
        assert!(title_similarity("Bohemian Rhapsody - Remastered 2011", "Bohemian Rhapsody") > 0.9);
        assert!(artist_similarity("Queen & David Bowie", "Queen") > 0.85);
        assert!(similarity("Hello", "Yesterday") < 0.6);
        assert!(is_garbage_title("Track 07"));
        assert!(is_garbage_title("01"));
        assert!(!is_garbage_title("1999"));
        assert_eq!(primary_artist("Taco Hemingway feat. Dawid Podsiadło"), "Taco Hemingway");
    }
}
