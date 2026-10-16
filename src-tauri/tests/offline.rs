//! Testy zapisu/odczytu na prawdziwych plikach audio (bez sieci).
//! Pliki testowe generowane są przez ffmpeg – test jest pomijany, gdy go brak.

use music_tagger_lib::{history::History, models::*, ops, tags};
use std::path::{Path, PathBuf};
use std::process::Command;

fn ffmpeg_available() -> bool {
    Command::new("ffmpeg").arg("-version").output().map(|o| o.status.success()).unwrap_or(false)
}

fn make(dir: &Path, name: &str, args: &[&str]) -> PathBuf {
    let p = dir.join(name);
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=8", "-ac", "1"]);
    cmd.args(args).arg(&p);
    assert!(cmd.status().unwrap().success(), "ffmpeg {name}");
    p
}

fn png_bytes(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x % 255) as u8, (y % 255) as u8, 120]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mt-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

const LRC: &str = "[00:01.00] Pierwsza linia\n[00:03.50] Druga linia – zażółć gęślą jaźń\n[00:06.00] Trzecia linia";

#[test]
fn write_read_undo_all_formats() {
    if !ffmpeg_available() {
        eprintln!("ffmpeg niedostępny – pomijam");
        return;
    }
    let dir = tmpdir("formats");
    let files = vec![
        make(&dir, "a.mp3", &["-ar", "22050", "-b:a", "32k"]),
        make(&dir, "b.flac", &["-ar", "22050"]),
        make(&dir, "c.m4a", &["-ar", "22050", "-c:a", "aac", "-b:a", "32k"]),
        make(&dir, "d.ogg", &["-ar", "22050", "-c:a", "libvorbis"]),
        make(&dir, "e.opus", &["-ar", "24000", "-c:a", "libopus", "-b:a", "24k"]),
        make(&dir, "f.wav", &["-ar", "22050"]),
        make(&dir, "g.aiff", &["-ar", "22050"]),
    ];
    let hist = History::open(&dir.join("data"));
    let (cover, _, _) = tags::process_cover(&png_bytes(1600, 1600), 1200, 90).unwrap();
    for f in &files {
        let before = tags::read_track(f);
        assert!(before.error.is_none(), "{f:?}: {:?}", before.error);
        assert!(before.duration > 7.0);
        assert!(before.missing.contains(&"title".to_string()));

        hist.record("b1", f, "tag", vec!["title".into()]).expect("snapshot");
        let req = tags::WriteRequest {
            fields: vec![
                ("title".into(), Some("Małomiasteczkowy".into())),
                ("artist".into(), Some("Dawid Podsiadło".into())),
                ("album".into(), Some("Małomiasteczkowy".into())),
                ("albumArtist".into(), Some("Dawid Podsiadło".into())),
                ("year".into(), Some("2018".into())),
                ("track".into(), Some("1".into())),
                ("trackTotal".into(), Some("11".into())),
                ("genre".into(), Some("Pop".into())),
                ("label".into(), Some("Sony Music".into())),
                ("isrc".into(), Some("PLA301800001".into())),
                ("mbRecordingId".into(), Some("00000000-0000-0000-0000-000000000001".into())),
            ],
            cover: Some(cover.clone()),
            remove_cover: false,
            lyrics: Some(Some(LRC.into())),
        };
        tags::write(f, &req).unwrap_or_else(|e| panic!("{f:?}: {e}"));
        let after = tags::read_track(f);
        let tg = &after.tags;
        assert_eq!(tg.title.as_deref(), Some("Małomiasteczkowy"), "{f:?}");
        assert_eq!(tg.artist.as_deref(), Some("Dawid Podsiadło"), "{f:?}");
        assert_eq!(tg.album_artist.as_deref(), Some("Dawid Podsiadło"), "{f:?}");
        assert_eq!(tg.year.as_deref().and_then(|y| y.get(0..4)), Some("2018"), "{f:?}");
        assert_eq!(tg.track, Some(1), "{f:?}");
        assert_eq!(tg.track_total, Some(11), "{f:?}");
        assert_eq!(tg.genre.as_deref(), Some("Pop"), "{f:?}");
        assert_eq!(tg.mb_recording_id.as_deref(), Some("00000000-0000-0000-0000-000000000001"), "{f:?}");
        let c = after.cover.as_ref().unwrap_or_else(|| panic!("brak okładki {f:?}"));
        assert_eq!((c.width, c.height), (1200, 1200), "{f:?}");
        assert!(after.lyrics_synced, "{f:?}");
        assert!(after.lyrics.as_deref().unwrap().contains("zażółć"), "{f:?}");
        assert!(after.score >= 95, "{f:?} score {}", after.score);
        if f.extension().unwrap() == "mp3" {
            let raw = std::fs::read(f).unwrap();
            assert!(raw.windows(4).any(|w| w == b"SYLT"), "MP3 powinien mieć ramkę SYLT");
            assert!(raw.windows(4).any(|w| w == b"USLT"), "MP3 powinien mieć ramkę USLT");
        }
        assert!(!tags::lrc_path(f).exists(), "nie tworzymy pliku .lrc przy zapisie w tagu");
        assert!(after.missing.is_empty(), "{f:?} {:?}", after.missing);
    }
    // Cofnięcie całej partii przywraca stan sprzed zmian.
    let n = hist.undo(&["b1".into()]).unwrap();
    assert_eq!(n.len(), files.len());
    for f in &files {
        let t = tags::read_track(f);
        assert!(t.tags.title.is_none(), "{f:?} po cofnięciu: {:?}", t.tags.title);
        assert!(t.cover.is_none(), "{f:?} okładka po cofnięciu");
        assert!(t.lyrics.is_none(), "{f:?} tekst po cofnięciu");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn id3v1_only_file_keeps_data() {
    if !ffmpeg_available() {
        return;
    }
    let dir = tmpdir("id3v1");
    let f = make(&dir, "v1.mp3", &["-ar", "22050", "-b:a", "32k", "-id3v2_version", "0"]);
    // Ręcznie dopisany tag ID3v1 (128 bajtów na końcu pliku).
    let mut v1 = b"TAG".to_vec();
    for (s, n) in [("Old Title", 30), ("Old Artist", 30), ("Old Album", 30), ("1999", 4), ("", 30)] {
        let mut b = s.as_bytes().to_vec();
        b.resize(n, 0);
        v1.extend(b);
    }
    v1.push(17);
    let mut data = std::fs::read(&f).unwrap();
    data.extend(v1);
    std::fs::write(&f, data).unwrap();
    let before = tags::read_track(&f);
    assert_eq!(before.tags.title.as_deref(), Some("Old Title"));
    tags::write(&f, &tags::WriteRequest { fields: vec![("genre".into(), Some("Rock".into()))], ..Default::default() }).unwrap();
    let after = tags::read_track(&f);
    assert_eq!(after.tags.title.as_deref(), Some("Old Title"));
    assert_eq!(after.tags.artist.as_deref(), Some("Old Artist"));
    assert_eq!(after.tags.genre.as_deref(), Some("Rock"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rename_with_lrc_and_undo() {
    if !ffmpeg_available() {
        return;
    }
    let dir = tmpdir("rename");
    let f = make(&dir, "01 track.mp3", &["-ar", "22050", "-b:a", "32k", "-metadata", "title=Bohemian Rhapsody", "-metadata", "artist=Queen", "-metadata", "track=11"]);
    std::fs::write(tags::lrc_path(&f), LRC).unwrap();
    let t = tags::read_track(&f);
    assert!(t.lrc_file && t.lyrics_synced);
    let name = ops::render_pattern("%track% - %artist% - %title%", &t).unwrap();
    assert_eq!(name, "11 - Queen - Bohemian Rhapsody.mp3");
    let hist = History::open(&dir.join("data"));
    let new = ops::rename_file(&f, &name).unwrap();
    hist.record_rename("r1", &f, &new);
    assert!(new.exists() && !f.exists());
    assert!(tags::lrc_path(&new).exists(), "plik .lrc powinien przejść razem z utworem");
    // Znaki niedozwolone w Windows są zastępowane.
    let mut t2 = t.clone();
    t2.tags.title = Some("What? Why: \"Now\"".into());
    assert_eq!(ops::render_pattern("%artist% - %title%", &t2).unwrap(), "Queen - What_ Why_ _Now.mp3");
    hist.undo(&["r1".into()]).unwrap();
    assert!(f.exists() && !new.exists());
    assert!(tags::lrc_path(&f).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lrc_header_and_changes() {
    let l = tags::parse_lrc(LRC);
    assert_eq!(l.len(), 3);
    assert_eq!(l[1].0, 3500);
    let f = TagFields { title: Some("T".into()), artist: Some("A".into()), ..Default::default() };
    let s = ops::lrc_with_header(LRC, &f, 125.0);
    assert!(s.starts_with("[ar:A]\n[ti:T]\n"));
    assert!(s.contains("[length:02:05]"));

    let mut settings = music_tagger_lib::settings::Settings::default();
    let old = TagFields { title: Some("Track 07".into()), artist: Some("Adele".into()), year: Some("2015".into()), ..Default::default() };
    let new = TagFields { title: Some("Hello".into()), artist: Some("ADELE".into()), album: Some("25".into()), year: Some("2015-10-23".into()), ..Default::default() };
    let ch = music_tagger_lib::engine::build_changes(&old, &new, &settings);
    let fields: Vec<&str> = ch.iter().map(|c| c.field.as_str()).collect();
    // „Track 07” to śmieciowy tytuł – zastępowany nawet w trybie „tylko brakujące”.
    assert_eq!(fields, vec!["title", "album"]);
    settings.write_mode = "overwrite".into();
    let ch = music_tagger_lib::engine::build_changes(&old, &new, &settings);
    let fields: Vec<&str> = ch.iter().map(|c| c.field.as_str()).collect();
    assert_eq!(fields, vec!["title", "artist", "album"]);
}

/// WAV z istniejącym blokiem RIFF INFO (np. z Eksploratora Windows) – po zapisie oba bloki mają nowe dane.
#[test]
fn wav_with_riff_info() {
    if !ffmpeg_available() {
        return;
    }
    let dir = tmpdir("wav");
    let f = make(&dir, "x.wav", &["-ar", "22050", "-metadata", "title=Stary tytuł", "-metadata", "artist=Stary wykonawca"]);
    let before = tags::read_track(&f);
    assert_eq!(before.tags.title.as_deref(), Some("Stary tytuł"));
    let hist = History::open(&dir.join("data"));
    hist.record("w1", &f, "tag", vec!["title".into()]).expect("snapshot");
    let (cover, _, _) = tags::process_cover(&png_bytes(600, 600), 1200, 90).unwrap();
    let req = tags::WriteRequest {
        fields: vec![("title".into(), Some("Nowy tytuł – żółć".into())), ("album".into(), Some("Album".into()))],
        cover: Some(cover),
        lyrics: Some(Some(LRC.into())),
        ..Default::default()
    };
    tags::write(&f, &req).unwrap();
    let after = tags::read_track(&f);
    assert!(after.error.is_none(), "{:?}", after.error);
    assert_eq!(after.tags.title.as_deref(), Some("Nowy tytuł – żółć"));
    assert_eq!(after.tags.artist.as_deref(), Some("Stary wykonawca"));
    assert!(after.cover.is_some() && after.lyrics_synced);
    assert!(after.duration > 7.0);
    use lofty::prelude::*;
    let file = lofty::read_from_path(&f).unwrap();
    let info = file.tag(lofty::tag::TagType::RiffInfo).expect("RIFF INFO");
    assert_eq!(info.title().as_deref(), Some("Nowy tytuł – żółć"));
    assert_eq!(info.album().as_deref(), Some("Album"));
    // Ponowny zapis nie psuje pliku.
    tags::write(&f, &tags::WriteRequest { fields: vec![("title".into(), Some("Trzeci".into()))], ..Default::default() }).unwrap();
    assert_eq!(tags::read_track(&f).tags.title.as_deref(), Some("Trzeci"));
    // Cofnięcie przywraca dane z RIFF INFO sprzed zmian.
    hist.undo(&["w1".into()]).unwrap();
    let back = tags::read_track(&f);
    assert_eq!(back.tags.title.as_deref(), Some("Stary tytuł"));
    assert_eq!(back.tags.album, None);
    assert!(back.cover.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
