//! Extract audio from video containers to MP3 (for rodio) — symphonia, then libav + libmp3lame.

use std::fs::File;
use std::io::Write;
use std::path::Path;

use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_AAC, CODEC_TYPE_FLAC, CODEC_TYPE_MP3, CODEC_TYPE_NULL, CODEC_TYPE_VORBIS};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

const OUT_RATE: u32 = 44_100;
const OUT_CHANNELS: u16 = 2;

/// Full decoded PCM for a file (shared across layers / seeks).
#[derive(Clone)]
pub struct CachedPcm {
    pub channels: u16,
    pub sample_rate: u32,
    pub samples: std::sync::Arc<Vec<f32>>,
}

pub type AudioPcmCache =
    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<CachedPcm>>>>;

#[derive(Debug)]
pub struct AudioPrepareResult {
    pub channels: u16,
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

pub type ExtractProgress = std::sync::Arc<dyn Fn(f32) + Send + Sync>;

pub fn load_pcm_from_wav(path: &Path) -> Option<CachedPcm> {
    let reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    let samples: Vec<f32> = reader
        .into_samples::<i16>()
        .filter_map(|s| s.ok())
        .map(|s| s as f32 / i16::MAX as f32)
        .collect();
    if samples.is_empty() {
        return None;
    }
    Some(CachedPcm {
        channels: spec.channels.max(1),
        sample_rate: spec.sample_rate,
        samples: std::sync::Arc::new(samples),
    })
}

pub fn load_pcm_from_file(path: &Path) -> Option<CachedPcm> {
    let path_str = path.to_string_lossy();
    // Images / tiny files: never hand to symphonia (probe EOF spam).
    if crate::document::AvClip::path_is_still_image(path_str.as_ref()) {
        return None;
    }
    if path
        .metadata()
        .map(|m| m.len() < 2048)
        .unwrap_or(true)
    {
        return None;
    }
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
    {
        if let Some(pcm) = load_pcm_from_wav(path) {
            return Some(pcm);
        }
    }
    if let Some(pcm) = load_pcm_decoded(path) {
        return Some(pcm);
    }
    load_pcm_via_rodio(path)
}

/// Full-file decode via symphonia, then libav (covers MP4/MOV when rodio cannot stream).
fn load_pcm_decoded(path: &Path) -> Option<CachedPcm> {
    let silent: ExtractProgress = std::sync::Arc::new(|_| {});
    if let Ok(stereo) = decode_audio_stereo_symphonia(path, silent.clone()) {
        return Some(stereo_pcm_to_cached(stereo));
    }
    if let Ok(stereo) = decode_audio_stereo_libav(path, silent) {
        return Some(stereo_pcm_to_cached(stereo));
    }
    None
}

fn stereo_pcm_to_cached(stereo: StereoPcmI16) -> CachedPcm {
    let samples: Vec<f32> = stereo
        .samples
        .iter()
        .map(|s| *s as f32 / i16::MAX as f32)
        .collect();
    CachedPcm {
        channels: OUT_CHANNELS,
        sample_rate: stereo.sample_rate,
        samples: std::sync::Arc::new(samples),
    }
}

fn load_pcm_via_rodio(path: &Path) -> Option<CachedPcm> {
    use rodio::Source;
    let file = File::open(path).ok()?;
    let decoder = rodio::Decoder::try_from(file).ok()?;
    let channels = decoder.channels().get().max(1);
    let sample_rate = decoder.sample_rate().get();
    let samples: Vec<f32> = decoder.collect();
    if samples.is_empty() {
        return None;
    }
    Some(CachedPcm {
        channels,
        sample_rate,
        samples: std::sync::Arc::new(samples),
    })
}

/// In-flight preload keys — prevents spawning a decode thread every UI frame
/// (that OOM'd laptops: N threads × full-song f32 PCM).
fn preload_inflight() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static S: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    S.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// Max full-file PCM entries (each can be 50–200MB for long tracks).
const PCM_CACHE_MAX_ENTRIES: usize = 2;
/// Refuse to fully decode files larger than this many bytes on disk (~prevents RAM bomb).
const PCM_MAX_FILE_BYTES: u64 = 12 * 1024 * 1024; // 12 MiB compressed ≈ still large decoded
/// Cap decoded samples (~90s stereo @ 44.1k) if something still fully decodes.
const PCM_MAX_SAMPLES: usize = 44_100 * 2 * 90;

/// Preload is **opt-in and rate-limited**. Prefer [`stream_file_to_player`] for playback
/// so we never hold whole songs in RAM.
pub fn spawn_preload_pcm(cache: AudioPcmCache, key: String, path: std::path::PathBuf) {
    // Skip huge files — decoded f32 would be many× compressed size.
    if path
        .metadata()
        .map(|m| m.len() > PCM_MAX_FILE_BYTES)
        .unwrap_or(true)
    {
        return;
    }
    if cache.lock().ok().is_some_and(|c| c.contains_key(&key)) {
        return;
    }
    {
        let Ok(mut inflight) = preload_inflight().lock() else {
            return;
        };
        if !inflight.insert(key.clone()) {
            return; // already decoding this path
        }
    }
    std::thread::Builder::new()
        .name("vadadee-audio-pcm-cache".into())
        .spawn(move || {
            let pcm = load_pcm_from_file(&path).map(|mut p| {
                // Hard cap decoded length.
                if p.samples.len() > PCM_MAX_SAMPLES {
                    let mut v = (*p.samples).clone();
                    v.truncate(PCM_MAX_SAMPLES);
                    p.samples = std::sync::Arc::new(v);
                }
                p
            });
            if let Some(pcm) = pcm {
                if let Ok(mut map) = cache.lock() {
                    // LRU-ish: drop oldest if over cap.
                    while map.len() >= PCM_CACHE_MAX_ENTRIES {
                        if let Some(k) = map.keys().next().cloned() {
                            map.remove(&k);
                        } else {
                            break;
                        }
                    }
                    map.insert(key.clone(), std::sync::Arc::new(pcm));
                }
            }
            if let Ok(mut inflight) = preload_inflight().lock() {
                inflight.remove(&key);
            }
        })
        .ok();
}

/// Stream a file into a rodio player (seek + append). **Does not load whole file into RAM.**
pub fn stream_file_to_player(
    player: &rodio::Player,
    path: &Path,
    offset_secs: f32,
    volume: f32,
) -> Result<(), String> {
    use rodio::Source;
    let file = File::open(path).map_err(|e| format!("open audio: {e}"))?;
    let mut decoder =
        rodio::Decoder::try_from(file).map_err(|e| format!("decode audio: {e}"))?;
    if offset_secs > 0.05 {
        let _ = decoder.try_seek(std::time::Duration::from_secs_f32(offset_secs.max(0.0)));
    }
    player.set_volume(volume.clamp(0.0, 4.0));
    player.append(decoder);
    player.play();
    Ok(())
}

/// True when full-file PCM is already in the cache (instant slice, no decode).
pub fn pcm_cache_has(cache: &AudioPcmCache, path: &str) -> bool {
    cache
        .lock()
        .ok()
        .is_some_and(|m| m.contains_key(path))
}

/// Decode / slice audio (uses PCM cache when available). Prefer calling after
/// [`spawn_preload_pcm`] so the first play is a cheap slice.
pub fn prepare_samples_at_offset(
    path: &Path,
    offset_secs: f32,
    cache: &AudioPcmCache,
) -> Option<AudioPrepareResult> {
    let key = path.to_string_lossy().to_string();
    if let Ok(map) = cache.lock() {
        if let Some(cached) = map.get(&key) {
            return Some(slice_cached(cached, offset_secs));
        }
    }
    if let Some(pcm) = load_pcm_from_file(path) {
        let arc = std::sync::Arc::new(pcm);
        if let Ok(mut map) = cache.lock() {
            map.insert(key.clone(), arc.clone());
        }
        return Some(slice_cached(&arc, offset_secs));
    }

    use rodio::Source;
    use std::num::{NonZeroU16, NonZeroU32};
    let file = File::open(path).ok()?;
    let mut decoder = rodio::Decoder::try_from(file).ok()?;
    let channels = NonZeroU16::new(decoder.channels().get())?;
    let sample_rate = NonZeroU32::new(decoder.sample_rate().get())?;
    if offset_secs > 0.0 {
        let _ = decoder.try_seek(std::time::Duration::from_secs_f32(offset_secs));
    }
    let ch = channels.get() as f32;
    let sr = sample_rate.get() as f32;
    let mut samples: Vec<f32> = decoder.collect();
    if offset_secs > 0.0 {
        let skip = (offset_secs * sr * ch).round() as usize;
        if skip < samples.len() {
            samples.drain(0..skip);
        } else {
            samples.clear();
        }
    }
    Some(AudioPrepareResult {
        channels: channels.get(),
        sample_rate: sample_rate.get(),
        samples,
    })
}

fn slice_cached(cached: &CachedPcm, offset_secs: f32) -> AudioPrepareResult {
    let ch = cached.channels.max(1) as f32;
    let skip = (offset_secs.max(0.0) * cached.sample_rate as f32 * ch).round() as usize;
    let skip = skip.min(cached.samples.len());
    // Never clone multi-minute tails into a new Vec for SamplesBuffer — cap ahead window.
    let max_ahead = PCM_MAX_SAMPLES;
    let end = (skip + max_ahead).min(cached.samples.len());
    AudioPrepareResult {
        channels: cached.channels,
        sample_rate: cached.sample_rate,
        samples: cached.samples[skip..end].to_vec(),
    }
}

/// Simple 3-band EQ (bass / mid / treble in “dB-ish” units, −12…+12).
/// Audible bass shelf so LinearBlur-style Param → Equalizer.bass is felt.
pub fn apply_eq_stereo_inplace(
    samples: &mut [f32],
    channels: u16,
    bass: f32,
    mid: f32,
    treble: f32,
) {
    if samples.is_empty() {
        return;
    }
    let ch = channels.max(1) as usize;
    let gb = 10f32.powf((bass.clamp(-18.0, 18.0)) / 20.0);
    let gm = 10f32.powf((mid.clamp(-18.0, 18.0)) / 20.0);
    let gt = 10f32.powf((treble.clamp(-18.0, 18.0)) / 20.0);
    // Unity → no work.
    if (gb - 1.0).abs() < 0.02 && (gm - 1.0).abs() < 0.02 && (gt - 1.0).abs() < 0.02 {
        return;
    }
    // One-pole low / high state per channel (≈200 Hz low, ≈3 kHz high at 44.1k).
    let a_low = 0.04_f32;
    let a_high = 0.25_f32;
    let mut low = vec![0.0_f32; ch];
    let mut high_lp = vec![0.0_f32; ch];
    for (i, s) in samples.iter_mut().enumerate() {
        let c = i % ch;
        let x = *s;
        low[c] += a_low * (x - low[c]);
        high_lp[c] += a_high * (x - high_lp[c]);
        let low_b = low[c];
        let high_b = x - high_lp[c];
        let mid_b = x - low_b - high_b;
        *s = (low_b * gb + mid_b * gm + high_b * gt).clamp(-1.0, 1.0);
    }
}

/// Decode video/container audio directly to a lossless WAV file (perfect quality, zero compression artifacts).
pub fn extract_audio_to_wav(
    input: &Path,
    output: &Path,
    report: ExtractProgress,
) -> Result<std::path::PathBuf, String> {
    report(0.01);
    let stereo = match decode_audio_stereo_symphonia(input, report.clone()) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("[audio] symphonia decode failed ({}), trying libav…", e);
            report(0.12);
            decode_audio_stereo_libav(input, report.clone())?
        }
    };
    report(0.86);
    let wav_path = output.with_extension("wav");
    write_wav(&wav_path, &stereo.samples, stereo.sample_rate, OUT_CHANNELS)?;
    report(1.0);
    Ok(wav_path)
}

pub struct StereoPcmI16 {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
}

fn decode_audio_stereo_symphonia(
    input: &Path,
    report: ExtractProgress,
) -> Result<StereoPcmI16, String> {
    let file = File::open(input).map_err(|e| e.to_string())?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = input.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| e.to_string())?;

    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| is_audio_track(t))
        .ok_or_else(|| "No audio track".to_string())?;

    let track_id = track.id;
    let total_frames = track.codec_params.n_frames;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| e.to_string())?;

    let mut pcm: Vec<i16> = Vec::new();
    let mut src_rate = track.codec_params.sample_rate.unwrap_or(OUT_RATE);
    let mut src_channels = track
        .codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(2)
        .max(1) as u16;
    let mut decoded_frames: u64 = 0;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(Error::ResetRequired) => break,
            Err(Error::IoError(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                decoded_frames += decoded.frames() as u64;
                append_interleaved_i16(&mut pcm, decoded);
                if let Some(total) = total_frames {
                    if total > 0 {
                        let p = (decoded_frames as f32 / total as f32).clamp(0.0, 1.0);
                        report(0.02 + p * 0.82);
                    }
                }
            }
            Err(Error::IoError(_)) => break,
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(e.to_string()),
        }
        if let Some(rate) = decoder.codec_params().sample_rate {
            src_rate = rate;
        }
        if let Some(ch) = decoder.codec_params().channels {
            src_channels = ch.count().max(1) as u16;
        }
    }

    if pcm.is_empty() {
        return Err("No audio samples decoded".into());
    }

    let stereo = resample_interleaved_to_stereo(&pcm, src_rate, src_channels, OUT_RATE);
    Ok(StereoPcmI16 {
        sample_rate: OUT_RATE,
        samples: stereo,
    })
}

fn decode_audio_stereo_libav(input: &Path, report: ExtractProgress) -> Result<StereoPcmI16, String> {
    let path = input.to_str().ok_or("bad path")?;
    let (interleaved, rate) = crate::video_decode::decode_audio_to_stereo_i16_libav(path, |p| {
        report(0.12 + p * 0.72);
    })?;
    let stereo = resample_interleaved_to_stereo(&interleaved, rate, 2, OUT_RATE);
    Ok(StereoPcmI16 {
        sample_rate: OUT_RATE,
        samples: stereo,
    })
}

fn extract_audio_symphonia(input: &Path, output: &Path) -> Result<(), String> {
    let file = File::open(input).map_err(|e| e.to_string())?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = input.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| e.to_string())?;

    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| is_audio_track(t))
        .ok_or_else(|| "No audio track".to_string())?;

    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| e.to_string())?;

    let mut pcm: Vec<i16> = Vec::new();
    let mut src_rate = track.codec_params.sample_rate.unwrap_or(OUT_RATE);
    let mut src_channels = track
        .codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(2)
        .max(1) as u16;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(Error::ResetRequired) => break,
            Err(Error::IoError(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => append_interleaved_i16(&mut pcm, decoded),
            Err(Error::IoError(_)) => break,
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(e.to_string()),
        }
        if let Some(rate) = decoder.codec_params().sample_rate {
            src_rate = rate;
        }
        if let Some(ch) = decoder.codec_params().channels {
            src_channels = ch.count().max(1) as u16;
        }
    }

    if pcm.is_empty() {
        return Err("No audio samples decoded".into());
    }

    let stereo = resample_interleaved_to_stereo(&pcm, src_rate, src_channels, OUT_RATE);
    write_wav(output, &stereo, OUT_RATE, OUT_CHANNELS)?;
    Ok(())
}

/// Media duration via symphonia (audio files and many containers).
pub fn probe_media_duration_symphonia(path: &str) -> Option<f32> {
    // Never probe still images / non-media — causes ERROR spam ("probe reach EOF").
    if crate::document::AvClip::path_is_still_image(path) {
        return None;
    }
    let path = Path::new(path);
    let meta = std::fs::metadata(path).ok()?;
    // Tiny files are never useful media (broken extracts, icons, etc.).
    if meta.len() < 2048 {
        return None;
    }
    let file = File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;
    let format = probed.format;
    let mut best_secs = 0.0_f64;
    for track in format.tracks() {
        let params = &track.codec_params;
        let Some(n_frames) = params.n_frames else {
            continue;
        };
        if n_frames == 0 {
            continue;
        }
        let secs = if let Some(tb) = params.time_base {
            let t = tb.calc_time(n_frames);
            t.seconds as f64 + t.frac
        } else if let Some(rate) = params.sample_rate {
            if rate > 0 {
                n_frames as f64 / rate as f64
            } else {
                continue;
            }
        } else {
            continue;
        };
        if secs.is_finite() && secs > best_secs {
            best_secs = secs;
        }
    }
    if best_secs > 0.05 {
        Some(best_secs as f32)
    } else {
        None
    }
}

fn is_audio_track(t: &symphonia::core::formats::Track) -> bool {
    let c = t.codec_params.codec;
    if c == CODEC_TYPE_NULL {
        return false;
    }
    if t.codec_params.sample_rate.is_some() {
        return true;
    }
    matches!(
        c,
        CODEC_TYPE_AAC | CODEC_TYPE_MP3 | CODEC_TYPE_VORBIS | CODEC_TYPE_FLAC
    )
}

/// Build a rodio source starting at `offset_secs` (no tight `next()` skip loop).
pub fn rodio_source_from_path(
    path: &Path,
    offset_secs: f32,
) -> Option<rodio::buffer::SamplesBuffer> {
    use rodio::Source;
    use std::num::{NonZeroU16, NonZeroU32};

    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
    {
        let reader = hound::WavReader::open(path).ok()?;
        let spec = reader.spec();
        let channels = NonZeroU16::new(spec.channels.max(1))?;
        let rate = NonZeroU32::new(spec.sample_rate)?;
        let ch = spec.channels as usize;
        let skip = (offset_secs.max(0.0) * spec.sample_rate as f32 * ch as f32).round() as usize;
        let samples: Vec<f32> = reader
            .into_samples::<i16>()
            .skip(skip)
            .filter_map(|s| s.ok())
            .map(|s| s as f32 / i16::MAX as f32)
            .collect();
        return Some(rodio::buffer::SamplesBuffer::new(channels, rate, samples));
    }

    let file = File::open(path).ok()?;
    let mut decoder = rodio::Decoder::try_from(file).ok()?;
    let channels = NonZeroU16::new(decoder.channels().get())?;
    let sample_rate = NonZeroU32::new(decoder.sample_rate().get())?;
    if offset_secs > 0.0 {
        let seek = std::time::Duration::from_secs_f32(offset_secs);
        if decoder.try_seek(seek).is_ok() {
            let samples: Vec<f32> = decoder.collect();
            return Some(rodio::buffer::SamplesBuffer::new(
                channels,
                sample_rate,
                samples,
            ));
        }
    }
    let ch = channels.get() as f32;
    let sr = sample_rate.get() as f32;
    let mut samples: Vec<f32> = decoder.collect();
    if offset_secs > 0.0 {
        let skip = (offset_secs * sr * ch).round() as usize;
        if skip < samples.len() {
            samples.drain(0..skip);
        } else {
            samples.clear();
        }
    }
    Some(rodio::buffer::SamplesBuffer::new(
        channels,
        sample_rate,
        samples,
    ))
}

pub fn write_mono_f32_as_wav(mono: &[f32], src_rate: u32, output: &Path) -> Result<(), String> {
    let pcm: Vec<i16> = mono
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect();
    let stereo = resample_interleaved_to_stereo(&pcm, src_rate, 1, OUT_RATE);
    write_wav(output, &stereo, OUT_RATE, OUT_CHANNELS)
}

fn append_interleaved_i16(out: &mut Vec<i16>, buf: AudioBufferRef<'_>) {
    match buf {
        AudioBufferRef::F32(b) => {
            let ch = b.spec().channels.count();
            let frames = b.frames();
            for f in 0..frames {
                for c in 0..ch {
                    let s = b.chan(c)[f].clamp(-1.0, 1.0);
                    out.push((s * i16::MAX as f32) as i16);
                }
            }
        }
        AudioBufferRef::S16(b) => {
            let ch = b.spec().channels.count();
            let frames = b.frames();
            for f in 0..frames {
                for c in 0..ch {
                    out.push(b.chan(c)[f]);
                }
            }
        }
        AudioBufferRef::S32(b) => {
            let ch = b.spec().channels.count();
            let frames = b.frames();
            for f in 0..frames {
                for c in 0..ch {
                    let s = (b.chan(c)[f] as f32) / (i32::MAX as f32);
                    out.push((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                }
            }
        }
        _ => {}
    }
}

/// Resample interleaved PCM to stereo at `out_rate` (keeps L/R; mono is duplicated).
fn resample_interleaved_to_stereo(
    interleaved: &[i16],
    src_rate: u32,
    src_channels: u16,
    out_rate: u32,
) -> Vec<i16> {
    let ch = src_channels.max(1) as usize;
    let frame_count = interleaved.len() / ch.max(1);
    if frame_count == 0 {
        return Vec::new();
    }

    let out_frames =
        ((frame_count as f64) * (out_rate as f64) / (src_rate.max(1) as f64)).round() as usize;
    let mut out = Vec::with_capacity(out_frames * 2);

    let sample_at = |frame: usize, channel: usize| -> f32 {
        let idx = frame * ch + channel.min(ch - 1);
        interleaved
            .get(idx)
            .map(|s| *s as f32 / i16::MAX as f32)
            .unwrap_or(0.0)
    };

    for i in 0..out_frames {
        let src_pos = (i as f64) * (src_rate as f64) / (out_rate as f64);
        let idx = src_pos.floor() as usize;
        let frac = (src_pos - idx as f64) as f32;
        for out_ch in 0..2usize {
            let src_ch = if ch == 1 { 0 } else { out_ch.min(ch - 1) };
            let s0 = sample_at(idx, src_ch);
            let s1 = sample_at(idx + 1, src_ch);
            let s = s0 + (s1 - s0) * frac;
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            out.push(v);
        }
    }
    out
}

fn write_wav(path: &Path, samples: &[i16], rate: u32, channels: u16) -> Result<(), String> {
    let data_len = (samples.len() * 2) as u32;
    let mut f = File::create(path).map_err(|e| e.to_string())?;
    let byte_rate = rate * channels as u32 * 2;
    let block_align = channels * 2;
    f.write_all(b"RIFF").map_err(|e| e.to_string())?;
    f.write_all(&(36 + data_len).to_le_bytes())
        .map_err(|e| e.to_string())?;
    f.write_all(b"WAVE").map_err(|e| e.to_string())?;
    f.write_all(b"fmt ").map_err(|e| e.to_string())?;
    f.write_all(&16u32.to_le_bytes()).map_err(|e| e.to_string())?;
    f.write_all(&1u16.to_le_bytes()).map_err(|e| e.to_string())?;
    f.write_all(&channels.to_le_bytes()).map_err(|e| e.to_string())?;
    f.write_all(&rate.to_le_bytes()).map_err(|e| e.to_string())?;
    f.write_all(&byte_rate.to_le_bytes()).map_err(|e| e.to_string())?;
    f.write_all(&block_align.to_le_bytes()).map_err(|e| e.to_string())?;
    f.write_all(&16u16.to_le_bytes()).map_err(|e| e.to_string())?;
    f.write_all(b"data").map_err(|e| e.to_string())?;
    f.write_all(&data_len.to_le_bytes()).map_err(|e| e.to_string())?;
    for s in samples {
        f.write_all(&s.to_le_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(())
}