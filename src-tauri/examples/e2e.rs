//! Test end-to-end silnika: `cargo run --example e2e -- <folder> [--apply] [--dump plik.json]`
use music_tagger_lib::{engine, history::History, models::*, ops, settings::Settings, tags};
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = PathBuf::from(args.get(1).expect("folder"));
    let apply = args.iter().any(|a| a == "--apply");
    let dump = args.iter().position(|a| a == "--dump").and_then(|i| args.get(i + 1)).cloned();
    let mut settings = Settings::default();
    settings.acoustid_key = std::env::var("ACOUSTID_KEY").unwrap_or_default();
    let hist = History::open(&std::env::temp_dir().join("mt-e2e"));
    let mut out: Vec<TrackEvent> = vec![];
    for e in walkdir::WalkDir::new(&dir).into_iter().flatten() {
        if !e.file_type().is_file() || !tags::is_audio(e.path()) {
            continue;
        }
        let t = tags::read_track(e.path());
        println!("\n=== {} ({:.0}s, score {})", t.file_name, t.duration, t.score);
        let q = engine::build_query(&t);
        println!("  query: {:?} / {:?} / album {:?}", q.artist, q.title, q.album);
        let a = engine::analyze(&t, &settings, None).await;
        if !a.warnings.is_empty() {
            println!("  warnings: {:?}", a.warnings);
        }
        match &a.proposal {
            None => println!("  -> NIE ZNALEZIONO"),
            Some(p) => {
                println!("  -> conf {:.2} via {} ({} alt.)", p.confidence, p.method, p.alternatives.len());
                for c in &p.changes {
                    println!("     {:<14} {:?} -> {:?}", c.field, c.old, c.new);
                }
                println!("     cover: {} {:?}", p.cover_enabled, p.covers.iter().map(|c| c.source.clone()).collect::<Vec<_>>());
                if let Some(l) = &p.lyrics {
                    println!("     lyrics: synced={} plain={} enabled={}", l.synced.is_some(), l.plain.is_some(), p.lyrics_enabled);
                }
                if apply {
                    match ops::apply_proposal(&t, p, &settings, &hist, "e2e").await {
                        Ok(nt) => println!("  APPLIED -> score {} missing {:?}", nt.score, nt.missing),
                        Err(e) => println!("  APPLY ERROR {e}"),
                    }
                }
            }
        }
        out.push(TrackEvent { track: t, proposal: a.proposal });
    }
    if let Some(d) = dump {
        std::fs::write(d, serde_json::to_string_pretty(&out).unwrap()).unwrap();
    }
}
