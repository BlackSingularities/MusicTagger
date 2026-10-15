//! Silnik dopasowywania: z tagów, nazwy pliku i odcisku audio buduje zapytania,
//! zbiera kandydatów z wielu źródeł, ocenia ich i tworzy propozycję zmian.

use crate::hints::{self, Hint};
use crate::models::*;
use crate::settings::Settings;
use crate::sources::{acoustid, deezer, http::HttpError, itunes, lyrics, musicbrainz};
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct Query {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<String>,
    pub track: Option<u32>,
    pub duration: f64,
    pub versions: Vec<&'static str>,
    /// Czy tytuł/wykonawca pochodzą z istniejących tagów (większe zaufanie).
    pub from_tags: bool,
}

/// Łączy istniejące tagi z informacjami z nazwy pliku.
pub fn build_query(track: &Track) -> Query {
    let hint: Hint = hints::parse_path(Path::new(&track.path));
    let t = &track.tags;
    let good_title = t.title.clone().filter(|x| !hints::is_garbage_title(x));
    let good_artist = t.artist.clone().filter(|x| !hints::is_garbage_artist(x));
    let good_album = t.album.clone().filter(|x| !hints::is_garbage_album(x));
    let mut q = Query {
        from_tags: good_title.is_some() && good_artist.is_some(),
        duration: track.duration,
        ..Default::default()
    };
    // Gdy tytuł w tagu zawiera „Artysta - Tytuł” (częste przy plikach z sieci).
    let (mut title, mut artist) = (good_title.clone(), good_artist.clone());
    if let (Some(tt), None) = (&title, &artist) {
        if let Some((a, b)) = tt.split_once(" - ") {
            artist = Some(a.trim().to_string());
            title = Some(b.trim().to_string());
        }
    }
    q.title = title.map(|x| hints::strip_noise(&x)).or(hint.title.clone());
    q.artist = artist.or(hint.artist.clone());
    q.album = good_album.or(hint.album.clone());
    q.year = t.year.clone().or(hint.year.clone());
    q.track = t.track.or(hint.track);
    q.versions = q.title.as_deref().map(hints::version_tags).unwrap_or_default();
    q
}

/// Ocena kandydata względem zapytania (0..1).
pub fn score_candidate(c: &Candidate, q: &Query) -> f64 {
    let ct = c.fields.title.as_deref().unwrap_or("");
    let ca = c.fields.artist.as_deref().unwrap_or("");
    let ts = q.title.as_deref().map(|t| hints::title_similarity(t, ct)).unwrap_or(0.5);
    let as_ = q.artist.as_deref().map(|a| hints::artist_similarity(a, ca)).unwrap_or(0.5);
    let ds = match c.duration {
        Some(d) if q.duration > 0.0 => hints::duration_score(q.duration, d),
        _ => 0.55,
    };
    let mut s = ts * 0.42 + as_ * 0.33 + ds * 0.25;
    // Brak artysty w zapytaniu: tytuł i czas muszą się zgadzać mocniej.
    if q.artist.is_none() {
        s = ts * 0.6 + ds * 0.4 - 0.08;
    }
    // Różne wersje (live/remix…) – kara.
    let mut cv = hints::version_tags(ct);
    if let Some(d) = &c.disambiguation {
        cv.extend(hints::version_tags(d));
    }
    for v in &cv {
        if !q.versions.contains(v) {
            s -= 0.12;
        }
    }
    for v in &q.versions {
        if !cv.contains(v) {
            s -= 0.06;
        }
    }
    if ts < 0.6 {
        s -= 0.15;
    }
    if q.artist.is_some() && as_ < 0.5 {
        s -= 0.2;
    }
    s.clamp(0.0, 1.0)
}

/// Preferencja wydania (album studyjny > singiel > składanka, oficjalne > bootleg…).
pub fn release_preference(c: &Candidate, q: &Query, settings: &Settings, min_year: Option<u32>) -> f64 {
    let mut p = 0.0;
    if let (Some(qa), Some(ca)) = (&q.album, &c.fields.album) {
        let s = hints::title_similarity(qa, ca);
        if s > 0.85 {
            p += 0.9 * s;
        }
    }
    match c.release_status.as_deref() {
        Some("Official") => p += 0.2,
        Some("Bootleg") => p -= 0.7,
        Some("Promotion") => p -= 0.1,
        Some("Pseudo-Release") => p -= 0.3,
        None => p -= 0.05,
        _ => {}
    }
    match c.release_type.as_deref() {
        Some("Album") => p += 0.25,
        Some("EP") => p += 0.12,
        Some("Single") => p += 0.08,
        Some("Broadcast") => p -= 0.2,
        _ => {}
    }
    for st in &c.secondary_types {
        p -= match st.as_str() {
            "Compilation" => 0.32,
            "Live" if !q.versions.contains(&"live") => 0.45,
            "Remix" if !q.versions.contains(&"remix") => 0.35,
            "DJ-mix" | "Mixtape/Street" => 0.4,
            "Demo" => 0.3,
            "Soundtrack" => 0.05,
            "Karaoke" => 0.6,
            _ => 0.1,
        };
    }
    if let Some(d) = &c.disambiguation {
        if !hints::version_tags(d).is_empty() {
            p -= 0.15;
        }
    }
    let alb = c.fields.album.as_deref().map(hints::fold).unwrap_or_default();
    for bad in ["deluxe", "box set", "anniversary", "collection", "greatest hits", "best of", "hits", "the essential", "karaoke", "tribute"] {
        if alb.contains(bad) && !q.album.as_deref().map(hints::fold).unwrap_or_default().contains(bad) {
            p -= 0.12;
        }
    }
    if c.fields.disc.unwrap_or(1) > 1 {
        p -= 0.06;
    }
    if settings.prefer_original_release {
        if let (Some(min), Some(y)) = (min_year, c.fields.year.as_deref().and_then(|y| y.parse::<u32>().ok())) {
            p += match y.saturating_sub(min) {
                0 => 0.2,
                1 => 0.1,
                2..=3 => 0.0,
                _ => -0.1,
            };
        }
    }
    if let (Some(qy), Some(cy)) = (&q.year, &c.fields.year) {
        if qy.get(0..4) == cy.get(0..4) {
            p += 0.1;
        }
    }
    if c.cover_url.is_some() {
        p += 0.02;
    }
    p
}

pub struct Analysis {
    pub proposal: Option<Proposal>,
    pub warnings: Vec<String>,
    /// Żadne źródło nie odpowiedziało z powodu braku sieci.
    pub offline: bool,
}

fn net_warn(warnings: &mut Vec<String>, e: &HttpError) {
    match e {
        HttpError::NotFound => {}
        other => warnings.push(other.to_string()),
    }
}

fn all_offline(w: &[String]) -> bool {
    !w.is_empty() && w.iter().all(|x| x.starts_with("brak połączenia"))
}

/// Główna funkcja – analizuje utwór i zwraca propozycję zmian.
pub async fn analyze(track: &Track, settings: &Settings, manual: Option<(String, Option<String>)>) -> Analysis {
    let mut warnings = vec![];
    let mut q = build_query(track);
    let is_manual = manual.is_some();
    if let Some((t, a)) = manual {
        q.title = Some(t.clone());
        q.artist = a.filter(|a| !a.trim().is_empty());
        q.versions = hints::version_tags(&t);
        q.from_tags = true;
    }
    let mut method = String::from("search");
    let mut pool: Vec<Candidate> = vec![];

    // 1) Znany identyfikator MusicBrainz – najpewniejsza ścieżka.
    if settings.use_musicbrainz {
        if let Some(id) = track.tags.mb_recording_id.as_ref().filter(|_| !is_manual) {
            match musicbrainz::lookup_recording(id).await {
                Ok(rec) => {
                    pool.extend(musicbrainz::candidates_from_recording(&rec, 1.0));
                    method = "mbid".into();
                }
                Err(e) => net_warn(&mut warnings, &e),
            }
        }
    }

    // 2) Wyszukiwanie tekstowe w kilku źródłach równolegle.
    if pool.is_empty() {
        if let Some(title) = q.title.clone() {
            let artist = q.artist.clone();
            let search_artist = artist.as_deref().map(hints::primary_artist);
            let clean_title = hints::split_feat(&title).0;
            let mb = async {
                if !settings.use_musicbrainz {
                    return Ok(vec![]);
                }
                let mut r = musicbrainz::search(&clean_title, search_artist.as_deref()).await?;
                if r.is_empty() {
                    let core = hints::normalize_title(&clean_title);
                    if core != hints::normalize(&clean_title) && !core.is_empty() {
                        r = musicbrainz::search(&core, search_artist.as_deref()).await?;
                    }
                }
                if r.is_empty() && artist.is_none() {
                    r = musicbrainz::search_free(&clean_title).await?;
                }
                Ok::<_, HttpError>(r)
            };
            let dz = async {
                if !settings.use_deezer {
                    return Ok(vec![]);
                }
                deezer::search(search_artist.as_deref(), &clean_title).await
            };
            let it = async {
                if !settings.use_itunes {
                    return Ok(vec![]);
                }
                let term = format!("{} {}", search_artist.clone().unwrap_or_default(), clean_title);
                itunes::search(term.trim(), &settings.itunes_country).await
            };
            let (mb, dz, it) = futures::join!(mb, dz, it);
            match mb {
                Ok(recs) => {
                    for (rec, s) in recs {
                        pool.extend(musicbrainz::candidates_from_recording(&rec, s));
                    }
                }
                Err(e) => net_warn(&mut warnings, &e),
            }
            match dz {
                Ok(list) => {
                    pool.extend(list.into_iter().map(|(c, _)| c));
                }
                Err(e) => net_warn(&mut warnings, &e),
            }
            match it {
                Ok(list) => pool.extend(list),
                Err(HttpError::RateLimited) => {}
                Err(e) => net_warn(&mut warnings, &e),
            }
        }
    }

    let score_pool = |pool: &mut Vec<(f64, Candidate)>, cands: &[Candidate], q: &Query| {
        for c in cands {
            pool.push((score_candidate(c, q), c.clone()));
        }
    };
    let mut scored: Vec<(f64, Candidate)> = vec![];
    if method == "mbid" {
        for c in &pool {
            scored.push((0.99, c.clone()));
        }
    } else {
        score_pool(&mut scored, &pool, &q);
    }
    let best_text = scored.iter().map(|(s, _)| *s).fold(0.0, f64::max);

    // 3) Odcisk audio, gdy wyszukiwanie tekstowe nie wystarcza.
    let need_fp = !is_manual && (best_text < 0.78 || q.title.is_none() || q.artist.is_none());
    if need_fp && settings.use_acoustid && !settings.acoustid_key.trim().is_empty() && settings.use_musicbrainz {
        let path = std::path::PathBuf::from(&track.path);
        let fp = tokio::task::spawn_blocking(move || crate::fingerprint::compute(&path)).await;
        match fp {
            Ok(Ok((fp, dur))) => match acoustid::lookup(settings.acoustid_key.trim(), &fp, dur).await {
                Ok(matches) => {
                    let mut taken = 0;
                    for m in matches.iter().filter(|m| m.score > 0.5).take(2) {
                        for id in m.recording_ids.iter().take(2) {
                            if taken >= 3 {
                                break;
                            }
                            taken += 1;
                            match musicbrainz::lookup_recording(id).await {
                                Ok(rec) => {
                                    for c in musicbrainz::candidates_from_recording(&rec, m.score) {
                                        // Odcisk: wysoka pewność niezależnie od nazwy pliku.
                                        let text = score_candidate(&c, &q);
                                        let dur_ok = c.duration.map(|d| hints::duration_score(d, dur)).unwrap_or(0.6);
                                        let s = (m.score * 0.75 + dur_ok * 0.15 + text * 0.1).min(0.99);
                                        scored.push((s, c));
                                    }
                                }
                                Err(e) => net_warn(&mut warnings, &e),
                            }
                        }
                    }
                    if !matches.is_empty() && scored.iter().any(|(s, _)| *s > best_text) {
                        method = "fingerprint".into();
                    }
                }
                Err(e) => net_warn(&mut warnings, &e),
            },
            Ok(Err(e)) => warnings.push(format!("Odcisk audio: {e}")),
            Err(e) => warnings.push(e.to_string()),
        }
    }

    if scored.is_empty() {
        return Analysis { offline: all_offline(&warnings), proposal: None, warnings };
    }

    // 4) Wybór najlepszego nagrania, potem najlepszego wydania w jego obrębie.
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let best_score = scored[0].0;
    if best_score < 0.55 {
        return Analysis { offline: all_offline(&warnings), proposal: None, warnings };
    }
    let best_title = hints::normalize_title(scored[0].1.fields.title.as_deref().unwrap_or(""));
    let best_artist = scored[0].1.fields.artist.clone().unwrap_or_default();
    let same = |c: &Candidate| {
        hints::sim_norm(&hints::normalize_title(c.fields.title.as_deref().unwrap_or("")), &best_title) > 0.9
            && hints::artist_similarity(c.fields.artist.as_deref().unwrap_or(""), &best_artist) > 0.8
    };
    // Gdy odcisk zidentyfikował utwór, zapytanie uzupełniamy jego danymi.
    if method == "fingerprint" {
        q.title = q.title.or(scored[0].1.fields.title.clone());
        q.artist = q.artist.or(scored[0].1.fields.artist.clone());
    }
    let group: Vec<(f64, Candidate)> =
        scored.iter().filter(|(s, c)| *s >= best_score - 0.12 && same(c)).cloned().collect();
    let min_year = group
        .iter()
        .filter(|(_, c)| c.source == "MusicBrainz" && c.release_status.as_deref() == Some("Official"))
        .filter_map(|(_, c)| c.fields.year.as_deref().and_then(|y| y.parse::<u32>().ok()))
        .min();
    let mut ranked: Vec<(f64, f64, Candidate)> = group
        .iter()
        .map(|(s, c)| {
            let src_bonus = match c.source.as_str() {
                "MusicBrainz" => 0.15,
                "Deezer" => 0.05,
                _ => 0.0,
            };
            (s * 1.5 + release_preference(c, &q, settings, min_year) + src_bonus, *s, c.clone())
        })
        .collect();
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    // Alternatywy: różne albumy/źródła (najpierw najlepiej ocenione wydania), max 10.
    let mut alternatives: Vec<Candidate> = vec![];
    let mut seen = std::collections::HashSet::new();
    for (s, c) in ranked.iter().map(|(_, s, c)| (*s, c)).chain(scored.iter().map(|(s, c)| (*s, c))) {
        let key = format!(
            "{}|{}|{}|{}",
            c.source,
            c.fields.album.as_deref().map(hints::normalize).unwrap_or_default(),
            hints::normalize(c.fields.title.as_deref().unwrap_or("")),
            hints::normalize(c.fields.artist.as_deref().unwrap_or(""))
        );
        if !seen.insert(key) {
            continue;
        }
        let mut c = c.clone();
        c.score = (s * 100.0).round() / 100.0;
        alternatives.push(c);
        if alternatives.len() >= 10 {
            break;
        }
    }

    let chosen = ranked[0].2.clone();
    let mut proposal = Proposal {
        track_id: track.id.clone(),
        confidence: (best_score * 100.0).round() / 100.0,
        method,
        changes: vec![],
        covers: vec![],
        cover_choice: None,
        cover_enabled: false,
        lyrics: None,
        lyrics_enabled: false,
        alternatives,
        chosen: 0,
        notes: warnings.clone(),
    };
    let supporting: Vec<Candidate> = scored.iter().filter(|(_, c)| same(c)).map(|(_, c)| c.clone()).collect();
    finalize(track, &mut proposal, chosen, &supporting, settings, &q).await;
    Analysis { proposal: Some(proposal), warnings, offline: false }
}

/// Uzupełnia wybranego kandydata (szczegóły wydania, okładki, tekst) i liczy zmiany.
pub async fn finalize(
    track: &Track,
    proposal: &mut Proposal,
    chosen: Candidate,
    supporting: &[Candidate],
    settings: &Settings,
    q: &Query,
) {
    let mut fields = chosen.fields.clone();
    let mut covers: Vec<CoverCandidate> = vec![];

    // Szczegóły z MusicBrainz (wytwórnia, liczba płyt, gatunek, okładka).
    let mut mb_front = false;
    if chosen.source == "MusicBrainz" {
        if let Some(rel) = &chosen.fields.mb_release_id {
            if let Some(d) = musicbrainz::release_details(rel).await {
                fields.label = fields.label.or(d.label);
                fields.genre = fields.genre.or(d.genre);
                if let Some(dt) = d.disc_total {
                    fields.disc_total = Some(dt);
                    if dt <= 1 {
                        fields.disc = None;
                        fields.disc_total = None;
                    }
                }
                mb_front = d.has_front;
                if settings.prefer_original_release {
                    if let (Some(oy), Some(y)) = (&d.original_year, &fields.year) {
                        // Reedycja tego samego albumu – rok oryginału.
                        if oy < y && chosen.secondary_types.is_empty() {
                            fields.year = Some(oy.clone());
                        }
                    }
                }
            }
        }
    } else if chosen.source == "Deezer" {
        let mut c = chosen.clone();
        deezer::enrich(&mut c).await;
        fields = c.fields;
    }

    // Łączenie źródeł: gdy żadne inne źródło nie opisuje tego utworu, dopytujemy Deezer wprost
    // o wybrany tytuł i wykonawcę (szybki, bez limitów) – stamtąd okładka, gatunek, ISRC, BPM.
    let mut supporting: Vec<Candidate> = supporting.to_vec();
    if settings.mix_sources && settings.use_deezer && chosen.source != "Deezer" && !supporting.iter().any(|c| c.source == "Deezer") {
        if let (Some(t), Some(a)) = (&fields.title, &fields.artist) {
            let clean_title = hints::split_feat(t).0;
            if let Ok(list) = deezer::search(Some(&hints::primary_artist(a)), &clean_title).await {
                let probe = Query { title: Some(t.clone()), artist: Some(a.clone()), duration: chosen.duration.unwrap_or(q.duration), versions: q.versions.clone(), ..Default::default() };
                supporting.extend(list.into_iter().map(|(c, _)| c).filter(|c| score_candidate(c, &probe) >= 0.75));
            }
        }
    }

    // Uzupełnianie luk z innych źródeł opisujących ten sam utwór.
    let mut album_norm = fields.album.as_deref().map(hints::normalize_title).unwrap_or_default();
    let mut deezer_enriched = chosen.source == "Deezer";
    let mut other_covers: Vec<CoverCandidate> = vec![];
    // Najpierw kandydaci z tego samego albumu – ich dane są najbardziej wiarygodne.
    let album_of = |c: &Candidate, norm: &str| {
        c.fields.album.as_deref().map(|a| hints::sim_norm(&hints::normalize_title(a), norm) > 0.85).unwrap_or(false)
    };
    supporting.sort_by_key(|c| !album_of(c, &album_norm));
    for c in supporting.iter().filter(|c| c.source != chosen.source) {
        if settings.mix_sources && fields.album.is_none() {
            if let Some(a) = &c.fields.album {
                fields.album = Some(a.clone());
                album_norm = hints::normalize_title(a);
            }
        }
        let same_album = album_of(c, &album_norm);
        let mut c = c.clone();
        if c.source == "Deezer" && !deezer_enriched && (fields.genre.is_none() || fields.isrc.is_none() || same_album) {
            deezer::enrich(&mut c).await;
            deezer_enriched = true;
        }
        if fields.genre.is_none() {
            fields.genre = c.fields.genre.clone();
        }
        if fields.isrc.is_none() {
            fields.isrc = c.fields.isrc.clone();
        }
        if fields.bpm.is_none() {
            fields.bpm = c.fields.bpm.clone();
        }
        if same_album {
            if fields.label.is_none() {
                fields.label = c.fields.label.clone();
            }
            if fields.track.is_none() {
                fields.track = c.fields.track;
                fields.track_total = fields.track_total.or(c.fields.track_total);
            }
            if settings.mix_sources {
                fields.year = fields.year.take().or(c.fields.year.clone());
                fields.album_artist = fields.album_artist.take().or(c.fields.album_artist.clone());
                fields.track_total = fields.track_total.or(c.fields.track_total);
                if fields.disc.is_none() && c.fields.disc_total.unwrap_or(1) > 1 {
                    fields.disc = c.fields.disc;
                    fields.disc_total = c.fields.disc_total;
                }
            }
        }
        if settings.mix_sources {
            fields.composer = fields.composer.take().or(c.fields.composer.clone());
        }
        // Pisownia z polskimi znakami (MB czasem ma wersję bez diakrytyków).
        for (mine, theirs) in [
            (&mut fields.title, &c.fields.title),
            (&mut fields.artist, &c.fields.artist),
        ] {
            if let (Some(m), Some(t)) = (mine.as_ref(), theirs) {
                if hints::normalize(m) == hints::normalize(t) && hints::diacritic_count(t) > hints::diacritic_count(m) {
                    *mine = Some(t.clone());
                }
            }
        }
        if let Some(url) = &c.cover_url {
            let cc = CoverCandidate {
                url: url.clone(),
                thumb: c.cover_thumb.clone().unwrap_or(url.clone()),
                source: c.source.clone(),
                width: 1400,
                height: 1400,
            };
            if same_album {
                covers.push(cc);
            } else {
                // Okładka innego wydania (np. singla) – tylko w ostateczności.
                other_covers.push(CoverCandidate {
                    source: format!("{} · {}", c.source, c.fields.album.clone().unwrap_or_default()),
                    ..cc
                });
            }
        }
    }
    // Okładka wybranego kandydata.
    if chosen.source == "MusicBrainz" {
        if mb_front {
            if let Some(u) = &chosen.cover_url {
                covers.push(CoverCandidate {
                    url: u.clone(),
                    thumb: chosen.cover_thumb.clone().unwrap_or(u.clone()),
                    source: "Cover Art Archive".into(),
                    width: 1200,
                    height: 1200,
                });
            }
        } else if let Some(rg) = &chosen.mb_release_group {
            covers.push(CoverCandidate {
                url: format!("https://coverartarchive.org/release-group/{rg}/front-1200"),
                thumb: format!("https://coverartarchive.org/release-group/{rg}/front-250"),
                source: "Cover Art Archive".into(),
                width: 1200,
                height: 1200,
            });
        }
    } else if let Some(u) = &chosen.cover_url {
        covers.insert(
            0,
            CoverCandidate {
                url: u.clone(),
                thumb: chosen.cover_thumb.clone().unwrap_or(u.clone()),
                source: chosen.source.clone(),
                width: 1400,
                height: 1400,
            },
        );
    }
    // Łączenie źródeł: brak pewnej okładki (albo tylko niepewna z CAA) → szukamy albumu w Deezer/iTunes.
    let has_store_cover = covers.iter().any(|c| c.source == "iTunes" || c.source == "Deezer");
    if settings.mix_sources && !has_store_cover {
        if let (Some(album), Some(artist)) = (fields.album.clone(), fields.album_artist.clone().or(fields.artist.clone())) {
            let artist = hints::primary_artist(&artist);
            covers.extend(album_cover_search(&artist, &album, settings, covers.is_empty()).await);
        }
    }
    if settings.mix_sources && covers.is_empty() {
        covers.extend(other_covers.into_iter().take(3));
    }
    // Kolejność: iTunes/Deezer (stała jakość) przed skanami z CAA.
    covers.sort_by_key(|c| match c.source.as_str() {
        "iTunes" => 0,
        "Deezer" => 1,
        _ => 2,
    });
    covers.dedup_by(|a, b| a.url == b.url);
    if !settings.fill_album_artist {
        fields.album_artist = None;
    } else if fields.album_artist.is_none() {
        fields.album_artist = fields.artist.as_deref().map(hints::primary_artist);
    }
    if !settings.fill_genre {
        fields.genre = None;
    }
    if !settings.write_mb_ids {
        fields.mb_recording_id = None;
        fields.mb_release_id = None;
    }

    proposal.changes = build_changes(&track.tags, &fields, settings);

    let need_cover = match &track.cover {
        None => true,
        Some(c) => settings.replace_small_covers && c.width > 0 && c.width < settings.min_cover_size,
    };
    proposal.cover_enabled = settings.fetch_covers && need_cover && !covers.is_empty();
    proposal.cover_choice = if covers.is_empty() { None } else { Some(0) };
    proposal.covers = covers;

    // Tekst (najlepiej z czasami). Plik .lrc obok utworu → przeniesienie do środka pliku.
    let path = Path::new(&track.path);
    let from_lrc = (settings.merge_lrc_files && track.lrc_file && !track.lyrics_in_tag)
        .then(|| crate::tags::read_lrc(path))
        .flatten()
        .map(|text| {
            let synced = crate::tags::is_synced_lyrics(&text);
            LyricsResult {
                synced: synced.then(|| text.clone()),
                plain: (!synced).then_some(text),
                source: crate::ops::LRC_SOURCE.into(),
                instrumental: false,
            }
        });
    let has_synced = track.lyrics_synced || from_lrc.as_ref().map(|l| l.synced.is_some()).unwrap_or(false);
    let has_any = track.lyrics.is_some() || from_lrc.is_some();
    let need_online = !has_any || (settings.prefer_synced && !has_synced);
    if settings.fetch_lyrics && need_online {
        let artist = fields.artist.clone().or(track.tags.artist.clone()).or(q.artist.clone());
        let title = fields.title.clone().or(track.tags.title.clone()).or(q.title.clone());
        if let (Some(a), Some(t)) = (artist, title) {
            let lq = lyrics::LyricsQuery { artist: &a, title: &t, album: fields.album.as_deref(), duration: track.duration };
            let (found, errs) = lyrics::find(&lq, settings, has_any).await;
            match found {
                Some(l) => {
                    let useful = if has_any { l.synced.is_some() } else { l.plain.is_some() || l.synced.is_some() };
                    proposal.lyrics_enabled = useful && !l.instrumental;
                    if l.instrumental {
                        proposal.notes.push("instrumental".into());
                    }
                    if useful || l.instrumental || from_lrc.is_none() {
                        proposal.lyrics = Some(l);
                    }
                }
                None => proposal.notes.extend(errs),
            }
        }
    }
    if !proposal.lyrics_enabled {
        if let Some(l) = from_lrc {
            proposal.lyrics = Some(l);
            proposal.lyrics_enabled = true;
        }
    }
}

/// Szuka okładki albumu w Deezer (i w iTunes, gdy `with_itunes`) – dopasowanie po tytule albumu i wykonawcy.
async fn album_cover_search(artist: &str, album: &str, settings: &Settings, with_itunes: bool) -> Vec<CoverCandidate> {
    let term = format!("{artist} {}", hints::split_feat(album).0);
    let dz = async {
        if settings.use_deezer {
            deezer::album_covers(&term).await.unwrap_or_default()
        } else {
            vec![]
        }
    };
    let it = async {
        if settings.use_itunes && with_itunes {
            itunes::album_covers(&term, &settings.itunes_country).await.unwrap_or_default()
        } else {
            vec![]
        }
    };
    let (dz, it) = futures::join!(dz, it);
    let want = hints::normalize_title(album);
    let mut out = vec![];
    for (list, src) in [(dz, "Deezer"), (it, "iTunes")] {
        let best = list
            .into_iter()
            .map(|(a, al, c)| (hints::sim_norm(&hints::normalize_title(&al), &want) * 0.6 + hints::artist_similarity(&a, artist) * 0.4, a, al, c))
            .filter(|(s, a, al, _)| *s > 0.85 && hints::artist_similarity(a, artist) > 0.7 && hints::sim_norm(&hints::normalize_title(al), &want) > 0.85)
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        if let Some((_, _, _, mut c)) = best {
            c.source = src.into();
            out.push(c);
        }
    }
    out
}

/// Porównuje bieżące tagi z nowymi wartościami według trybu zapisu.
pub fn build_changes(old: &TagFields, new: &TagFields, settings: &Settings) -> Vec<FieldChange> {
    let overwrite = settings.write_mode == "overwrite";
    let mut out = vec![];
    for name in FIELD_NAMES {
        if *name == "comment" {
            continue;
        }
        let o = old.get(name);
        let n = new.get(name);
        let Some(nv) = n.clone() else { continue };
        let garbage_old = match (*name, &o) {
            ("title", Some(v)) => hints::is_garbage_title(v) || v.contains(" - ") && hints::normalize(v).contains(&hints::normalize(&nv)),
            ("artist" | "albumArtist", Some(v)) => hints::is_garbage_artist(v),
            ("album", Some(v)) => hints::is_garbage_album(v),
            _ => false,
        };
        let include = match &o {
            None => true,
            Some(ov) if garbage_old => ov != &nv,
            Some(ov) => {
                if !overwrite {
                    false
                } else if matches!(*name, "track" | "trackTotal" | "disc" | "discTotal") {
                    ov.split('/').next().map(|x| x.trim().trim_start_matches('0')) != Some(nv.trim_start_matches('0'))
                } else if *name == "year" {
                    ov.get(0..4) != nv.get(0..4)
                } else {
                    ov.trim() != nv.trim()
                }
            }
        };
        if include {
            out.push(FieldChange { field: name.to_string(), old: o, new: Some(nv), enabled: true });
        }
    }
    out
}

/// Ponownie liczy propozycję dla alternatywnego kandydata wybranego przez użytkownika.
pub async fn rebuild_with(track: &Track, proposal: &mut Proposal, idx: usize, settings: &Settings) {
    let Some(c) = proposal.alternatives.get(idx).cloned() else { return };
    let q = build_query(track);
    proposal.chosen = idx;
    proposal.confidence = c.score;
    proposal.method = "manual".into();
    let supporting: Vec<Candidate> = proposal
        .alternatives
        .iter()
        .filter(|a| {
            hints::title_similarity(a.fields.title.as_deref().unwrap_or(""), c.fields.title.as_deref().unwrap_or("")) > 0.9
        })
        .cloned()
        .collect();
    finalize(track, proposal, c, &supporting, settings, &q).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(album: &str, year: &str, rtype: &str, sec: &[&str], status: &str, dur: f64) -> Candidate {
        Candidate {
            source: "MusicBrainz".into(),
            fields: TagFields {
                title: Some("Bohemian Rhapsody".into()),
                artist: Some("Queen".into()),
                album: Some(album.into()),
                year: Some(year.into()),
                ..Default::default()
            },
            duration: Some(dur),
            release_type: Some(rtype.into()),
            secondary_types: sec.iter().map(|s| s.to_string()).collect(),
            release_status: Some(status.into()),
            ..Default::default()
        }
    }

    #[test]
    fn prefers_original_studio_album() {
        let settings = Settings::default();
        let q = Query { title: Some("Bohemian Rhapsody".into()), artist: Some("Queen".into()), duration: 355.0, ..Default::default() };
        let list = vec![
            cand("Live at Wembley '86", "1992", "Album", &["Live"], "Official", 360.0),
            cand("Greatest Hits", "1981", "Album", &["Compilation"], "Official", 355.0),
            cand("Rock Montreal", "1981", "Album", &["Live"], "Bootleg", 356.0),
            cand("A Night at the Opera", "1975", "Album", &[], "Official", 355.0),
            cand("Bohemian Rhapsody", "1975", "Single", &[], "Official", 355.0),
        ];
        let best = list
            .iter()
            .max_by(|a, b| {
                let sa = score_candidate(a, &q) * 1.5 + release_preference(a, &q, &settings, Some(1975));
                let sb = score_candidate(b, &q) * 1.5 + release_preference(b, &q, &settings, Some(1975));
                sa.partial_cmp(&sb).unwrap()
            })
            .unwrap();
        assert_eq!(best.fields.album.as_deref(), Some("A Night at the Opera"));
    }

    #[test]
    fn album_hint_wins_and_versions_penalized() {
        let settings = Settings::default();
        let q = Query {
            title: Some("Bohemian Rhapsody".into()),
            artist: Some("Queen".into()),
            album: Some("Greatest Hits".into()),
            duration: 355.0,
            ..Default::default()
        };
        let a = cand("A Night at the Opera", "1975", "Album", &[], "Official", 355.0);
        let b = cand("Greatest Hits", "1981", "Album", &["Compilation"], "Official", 355.0);
        let (pa, pb) = (release_preference(&a, &q, &settings, Some(1975)), release_preference(&b, &q, &settings, Some(1975)));
        assert!(pb > pa, "hint {pb} vs studio {pa}");

        let mut live = cand("X", "1990", "Album", &[], "Official", 355.0);
        live.fields.title = Some("Bohemian Rhapsody (Live)".into());
        let studio = cand("X", "1990", "Album", &[], "Official", 355.0);
        let (ss, ls) = (score_candidate(&studio, &q), score_candidate(&live, &q));
        assert!(ss > ls + 0.1, "studio {ss} live {ls}");

        let mut wrong = cand("X", "1990", "Album", &[], "Official", 200.0);
        wrong.fields.artist = Some("Panic! at the Disco".into());
        let ws = score_candidate(&wrong, &q);
        assert!(ws < 0.55, "wrong artist score {ws}");
    }

    #[test]
    fn query_from_messy_file() {
        let t = Track {
            path: "/m/Downloads/Daft Punk - Get Lucky (Official Audio) ft. Pharrell Williams-5NV6Rdv1a3I.m4a".into(),
            duration: 369.0,
            tags: TagFields { title: Some("Track 01".into()), artist: Some("Unknown Artist".into()), ..Default::default() },
            ..Default::default()
        };
        let q = build_query(&t);
        assert_eq!(q.artist.as_deref(), Some("Daft Punk"));
        assert_eq!(hints::split_feat(q.title.as_deref().unwrap()).0, "Get Lucky");
        assert!(!q.from_tags);
    }
}
