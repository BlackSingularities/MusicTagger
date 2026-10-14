//! Odcisk audio (Chromaprint w czystym Rust) – dekodowanie przez Symphonię.

use base64::Engine;
use rusty_chromaprint::{Configuration, FingerprintCompressor, Fingerprinter};
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Maksymalna długość analizowanego fragmentu (AcoustID używa ~120 s).
const MAX_SECONDS: u64 = 120;

/// Zwraca (odcisk skompresowany base64-url, czas trwania w sekundach).
pub fn compute(path: &Path) -> Result<(String, f64), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("Nieobsługiwany format audio: {e}"))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("Brak ścieżki audio")?
        .clone();
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("Brak dekodera: {e}"))?;
    let sample_rate = track.codec_params.sample_rate.ok_or("Nieznana częstotliwość próbkowania")?;
    let channels = track.codec_params.channels.map(|c| c.count() as u32).unwrap_or(2);
    let total_duration = match (track.codec_params.n_frames, track.codec_params.time_base) {
        (Some(n), Some(tb)) => {
            let t = tb.calc_time(n);
            t.seconds as f64 + t.frac
        }
        (Some(n), None) => n as f64 / sample_rate as f64,
        _ => 0.0,
    };

    let config = Configuration::preset_test2();
    let mut printer = Fingerprinter::new(&config);
    printer.start(sample_rate, channels).map_err(|e| format!("{e:?}"))?;

    let limit = sample_rate as u64 * MAX_SECONDS * channels as u64;
    let mut consumed: u64 = 0;
    let mut decoded_frames: u64 = 0;
    let mut buf: Option<SampleBuffer<i16>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(_)) | Err(SymError::ResetRequired) => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id() != track.id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio) => {
                if buf.is_none() {
                    buf = Some(SampleBuffer::<i16>::new(audio.capacity() as u64, *audio.spec()));
                }
                if let Some(b) = buf.as_mut() {
                    if b.capacity() < audio.capacity() * audio.spec().channels.count() {
                        *b = SampleBuffer::<i16>::new(audio.capacity() as u64, *audio.spec());
                    }
                    b.copy_interleaved_ref(audio);
                    let samples = b.samples();
                    decoded_frames += (samples.len() as u64) / channels.max(1) as u64;
                    if consumed < limit {
                        let take = ((limit - consumed) as usize).min(samples.len());
                        printer.consume(&samples[..take]);
                        consumed += take as u64;
                    } else if total_duration > 0.0 {
                        break;
                    }
                }
            }
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    printer.finish();
    let fp = printer.fingerprint();
    if fp.is_empty() {
        return Err("Za krótki fragment audio".into());
    }
    let compressed = FingerprintCompressor::from(&config).compress(fp);
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(compressed);
    let duration = if total_duration > 0.0 { total_duration } else { decoded_frames as f64 / sample_rate as f64 };
    Ok((encoded, duration))
}
