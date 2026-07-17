//! Extract audio from video containers to WAV (for rodio) — symphonia, then libav.
//! Playback uses streaming WAV reads so the UI never loads multi‑minute PCM.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{
    DecoderOptions, CODEC_TYPE_AAC, CODEC_TYPE_FLAC, CODEC_TYPE_MP3, CODEC_TYPE_NULL,
    CODEC_TYPE_VORBIS,
};
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

// ── Extract once per output path (process lifetime) ───────────────────────────

fn extract_global_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// One demux per output WAV path for the whole process. Concurrent callers wait
/// on the same `OnceLock` — never N parallel re-extracts of the same file.
fn extract_once_map() -> &'static Mutex<HashMap<String, Arc<OnceLock<Result<PathBuf, String>>>>> {
    static M: OnceLock<Mutex<HashMap<String, Arc<OnceLock<Result<PathBuf, String>>>>>> =
        OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Ensure `output` WAV exists for `input` video/audio. Safe to call from many
/// threads / every frame — demuxes **at most once** per output path while the
/// file is present. If a previous extract's WAV was deleted from cache, re-runs.
pub fn ensure_extracted_wav(
    input: &Path,
    output: &Path,
    report: ExtractProgress,
) -> Result<PathBuf, String> {
    let wav_path = normalize_wav_path(output);
    let key = wav_path.to_string_lossy().to_string();

    // Hot path: already on disk and valid — no lock contention.
    if wav_is_playable(&wav_path) {
        return Ok(wav_path);
    }

    // Stale OnceLock: a prior success/failure is useless if the WAV is gone (or
    // extract failed and we want a retry after cache wipe / visibility churn).
    {
        let mut map = extract_once_map().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cell) = map.get(&key) {
            match cell.get() {
                Some(Ok(p)) if !wav_is_playable(p) => {
                    map.remove(&key);
                }
                Some(Err(_)) => {
                    map.remove(&key);
                }
                _ => {}
            }
        }
    }

    let cell = {
        let mut map = extract_once_map().lock().unwrap_or_else(|e| e.into_inner());
        map.entry(key.clone())
            .or_insert_with(|| Arc::new(OnceLock::new()))
            .clone()
    };

    // Exactly one thread runs the closure; others block until it finishes.
    let report_once = report.clone();
    let result = cell
        .get_or_init(|| {
            // Re-check after waiting for another thread.
            if wav_is_playable(&wav_path) {
                return Ok(wav_path.clone());
            }
            extract_audio_to_wav_inner(input, &wav_path, report_once)
        })
        .clone();

    // If another thread "succeeded" but the file vanished mid-flight, force one retry.
    match &result {
        Ok(p) if wav_is_playable(p) => result,
        Ok(_) | Err(_) => {
            {
                let mut map = extract_once_map().lock().unwrap_or_else(|e| e.into_inner());
                map.remove(&key);
            }
            if wav_is_playable(&wav_path) {
                return Ok(wav_path);
            }
            extract_audio_to_wav_inner(input, &wav_path, report)
        }
    }
}

fn normalize_wav_path(output: &Path) -> PathBuf {
    if output
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
    {
        output.to_path_buf()
    } else {
        output.with_extension("wav")
    }
}

/// Decode video/container audio directly to a lossless WAV file.
/// Prefer [`ensure_extracted_wav`] so the same path is never demuxed twice.
pub fn extract_audio_to_wav(
    input: &Path,
    output: &Path,
    report: ExtractProgress,
) -> Result<PathBuf, String> {
    ensure_extracted_wav(input, output, report)
}

fn extract_audio_to_wav_inner(
    input: &Path,
    wav_path: &Path,
    report: ExtractProgress,
) -> Result<PathBuf, String> {
    let _serialize = extract_global_lock();
    report(0.01);

    if wav_is_playable(wav_path) {
        report(1.0);
        return Ok(wav_path.to_path_buf());
    }
    if !input.is_file() {
        return Err(format!("input missing: {}", input.display()));
    }

    let stereo = match decode_audio_stereo_symphonia(input, report.clone()) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("[audio] symphonia decode failed ({e}), trying libav…");
            report(0.12);
            decode_audio_stereo_libav(input, report.clone())?
        }
    };
    if stereo.samples.is_empty() {
        return Err("decoded audio is empty".into());
    }
    let mut samples = stereo.samples;
    let ch = OUT_CHANNELS as usize;
    if ch > 1 && samples.len() % ch != 0 {
        samples.resize(samples.len() + (ch - samples.len() % ch), 0);
    }
    report(0.86);
    if let Some(parent) = wav_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }
    let tmp = {
        let mut t = wav_path.as_os_str().to_os_string();
        t.push(format!(".tmp.{}", std::process::id()));
        PathBuf::from(t)
    };
    write_wav_hound(&tmp, &samples, stereo.sample_rate, OUT_CHANNELS).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e
    })?;
    let tmp_sz = tmp.metadata().map(|m| m.len()).unwrap_or(0);
    if tmp_sz < 1024 {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("wav write too small ({tmp_sz} bytes)"));
    }
    std::fs::rename(&tmp, wav_path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("rename wav: {e}")
    })?;
    if !wav_is_playable(wav_path) {
        let _ = std::fs::remove_file(wav_path);
        return Err(format!("wav not playable after write: {}", wav_path.display()));
    }
    let sz = wav_path.metadata().map(|m| m.len()).unwrap_or(0);
    report(1.0);
    log::info!(
        "[audio] extracted {} samples → {} ({sz} bytes)",
        samples.len(),
        wav_path.display()
    );
    Ok(wav_path.to_path_buf())
}

/// Cheap check: exists, large enough, RIFF/WAVE header.
pub fn wav_is_playable(path: &Path) -> bool {
    use std::io::Read;
    let meta = match path.metadata() {
        Ok(m) => m,
        Err(_) => return false,
    };
    if meta.len() < 1024 {
        return false;
    }
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut hdr = [0u8; 12];
    if f.read_exact(&mut hdr).is_err() {
        return false;
    }
    &hdr[0..4] == b"RIFF" && &hdr[8..12] == b"WAVE"
}

// ── Playback ──────────────────────────────────────────────────────────────────

/// Stream a file into a rodio player (seek + append).
pub fn stream_file_to_player(
    player: &rodio::Player,
    path: &Path,
    offset_secs: f32,
    volume: f32,
) -> Result<(), String> {
    stream_file_to_player_rate(player, path, offset_secs, volume, 1.0)
}

/// Stream WAV (or pure audio) at `playback_rate`×. **Never** loads multi‑minute PCM on the UI thread.
pub fn stream_file_to_player_rate(
    player: &rodio::Player,
    path: &Path,
    offset_secs: f32,
    volume: f32,
    playback_rate: f32,
) -> Result<(), String> {
    use std::num::{NonZeroU16, NonZeroU32};

    player.set_volume(volume.clamp(0.0, 4.0));
    let playback_rate = if playback_rate.is_finite() && playback_rate > 0.0 {
        playback_rate.clamp(0.05, 16.0)
    } else {
        1.0
    };

    let path_str = path.to_string_lossy();
    if crate::document::AvClip::path_is_video_container(path_str.as_ref()) {
        return Err("video container cannot stream directly — wait for WAV extract".into());
    }

    let is_wav = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"));

    // Streaming first — O(1) open + seek, no multi‑MB UI stall on second Play.
    if is_wav {
        match StreamingWavSource::open(path, offset_secs, playback_rate) {
            Ok(src) => {
                player.append(src);
                player.play();
                return Ok(());
            }
            Err(e) => {
                log::warn!("streaming wav failed {}: {e}", path.display());
            }
        }
        // Tiny fallback (≤8s) only if streaming fails.
        if let Some(buf) =
            rodio_source_from_path_capped_rate(path, offset_secs, 8.0, playback_rate)
        {
            player.append(buf);
            player.play();
            return Ok(());
        }
        return Err(format!("decode audio: cannot read WAV {}", path.display()));
    }

    // Pure audio (mp3/aac/…): streaming rodio decoder at rate≈1.
    if let Ok(file) = File::open(path) {
        match rodio::Decoder::try_from(file) {
            Ok(mut decoder) => {
                use rodio::Source;
                if offset_secs > 0.05 {
                    let _ = decoder
                        .try_seek(std::time::Duration::from_secs_f32(offset_secs.max(0.0)));
                }
                if (playback_rate - 1.0).abs() <= 0.02 {
                    player.append(decoder);
                    player.play();
                    return Ok(());
                }
            }
            Err(e) => {
                log::debug!("rodio decode failed for {}: {e}", path.display());
            }
        }
    }

    if let Some(pcm) = load_pcm_from_file(path) {
        let ch = NonZeroU16::new(pcm.channels.max(1))
            .ok_or_else(|| "bad channel count".to_string())?;
        let base_rate = pcm.sample_rate.max(1) as f32;
        let out_rate = ((base_rate * playback_rate).round() as u32).max(1);
        let rate = NonZeroU32::new(out_rate).ok_or_else(|| "bad sample rate".to_string())?;
        let skip = (offset_secs.max(0.0) * pcm.sample_rate as f32 * pcm.channels as f32)
            .round() as usize;
        let samples: Vec<f32> = if skip >= pcm.samples.len() {
            Vec::new()
        } else {
            pcm.samples[skip..].to_vec()
        };
        const MAX: usize = 44_100 * 2 * 30; // 30s max
        let samples = if samples.len() > MAX {
            samples[..MAX].to_vec()
        } else {
            samples
        };
        player.append(rodio::buffer::SamplesBuffer::new(ch, rate, samples));
        player.play();
        return Ok(());
    }

    Err(format!(
        "decode audio: cannot play {} (unsupported or missing audio track)",
        path.display()
    ))
}

/// Streaming PCM WAV source for rodio.
pub struct StreamingWavSource {
    samples: hound::WavIntoSamples<std::io::BufReader<File>, i16>,
    channels: std::num::NonZeroU16,
    sample_rate: std::num::NonZeroU32,
}

impl StreamingWavSource {
    pub fn open(path: &Path, offset_secs: f32, playback_rate: f32) -> Result<Self, String> {
        let mut reader = hound::WavReader::open(path).map_err(|e| format!("open wav: {e}"))?;
        let spec = reader.spec();
        if spec.channels == 0 || spec.sample_rate == 0 {
            return Err("invalid wav header".into());
        }
        let rate_f = playback_rate.clamp(0.05, 16.0);
        let out_hz = ((spec.sample_rate as f32) * rate_f).round() as u32;
        let channels = std::num::NonZeroU16::new(spec.channels.max(1))
            .ok_or_else(|| "bad channels".to_string())?;
        let sample_rate =
            std::num::NonZeroU32::new(out_hz.max(1)).ok_or_else(|| "bad rate".to_string())?;
        let total_frames = reader.duration();
        if total_frames == 0 {
            return Err("wav has zero frames".into());
        }
        let mut frame = (offset_secs.max(0.0) * spec.sample_rate as f32).floor() as u32;
        if frame >= total_frames {
            frame = total_frames.saturating_sub(spec.sample_rate.max(1));
        }
        if frame > 0 {
            reader.seek(frame).map_err(|e| format!("seek wav: {e}"))?;
        }
        Ok(Self {
            samples: reader.into_samples::<i16>(),
            channels,
            sample_rate,
        })
    }
}

impl Iterator for StreamingWavSource {
    type Item = f32;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.samples.next() {
                Some(Ok(s)) => return Some(s as f32 / i16::MAX as f32),
                Some(Err(_)) => continue,
                None => return None,
            }
        }
    }
}

impl rodio::Source for StreamingWavSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        self.channels
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.sample_rate
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

pub fn rodio_source_from_path_capped(
    path: &Path,
    offset_secs: f32,
    max_secs: f32,
) -> Option<rodio::buffer::SamplesBuffer> {
    rodio_source_from_path_capped_rate(path, offset_secs, max_secs, 1.0)
}

pub fn rodio_source_from_path_capped_rate(
    path: &Path,
    offset_secs: f32,
    max_secs: f32,
    playback_rate: f32,
) -> Option<rodio::buffer::SamplesBuffer> {
    use std::num::{NonZeroU16, NonZeroU32};

    if !path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
    {
        return None;
    }
    let reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    let channels = NonZeroU16::new(spec.channels.max(1))?;
    let rate_f = playback_rate.clamp(0.05, 16.0);
    let out_hz = ((spec.sample_rate as f32) * rate_f).round() as u32;
    let rate = NonZeroU32::new(out_hz.max(1))?;
    let ch = spec.channels as usize;
    let skip = (offset_secs.max(0.0) * spec.sample_rate as f32 * ch as f32).round() as usize;
    let take = ((max_secs.max(1.0) * spec.sample_rate as f32 * ch as f32).round() as usize)
        .max(ch * spec.sample_rate as usize);
    let samples: Vec<f32> = reader
        .into_samples::<i16>()
        .skip(skip)
        .take(take)
        .filter_map(|s| s.ok())
        .map(|s| s as f32 / i16::MAX as f32)
        .collect();
    if samples.is_empty() {
        return None;
    }
    Some(rodio::buffer::SamplesBuffer::new(channels, rate, samples))
}

pub fn rodio_source_from_path(
    path: &Path,
    offset_secs: f32,
) -> Option<rodio::buffer::SamplesBuffer> {
    rodio_source_from_path_capped(path, offset_secs, 30.0)
}

// ── PCM helpers (legacy / export paths) ───────────────────────────────────────

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

fn preload_inflight() -> &'static Mutex<std::collections::HashSet<String>> {
    static S: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

const PCM_CACHE_MAX_ENTRIES: usize = 2;
const PCM_MAX_FILE_BYTES: u64 = 12 * 1024 * 1024;
const PCM_MAX_SAMPLES: usize = 44_100 * 2 * 90;

pub fn spawn_preload_pcm(cache: AudioPcmCache, key: String, path: PathBuf) {
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
            return;
        }
    }
    std::thread::Builder::new()
        .name("vadadee-audio-pcm-cache".into())
        .spawn(move || {
            let pcm = load_pcm_from_file(&path).map(|mut p| {
                if p.samples.len() > PCM_MAX_SAMPLES {
                    let mut v = (*p.samples).clone();
                    v.truncate(PCM_MAX_SAMPLES);
                    p.samples = std::sync::Arc::new(v);
                }
                p
            });
            if let Some(pcm) = pcm {
                if let Ok(mut map) = cache.lock() {
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

pub fn pcm_cache_has(cache: &AudioPcmCache, path: &str) -> bool {
    cache
        .lock()
        .ok()
        .is_some_and(|m| m.contains_key(path))
}

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
    None
}

fn slice_cached(cached: &CachedPcm, offset_secs: f32) -> AudioPrepareResult {
    let ch = cached.channels.max(1) as f32;
    let skip = (offset_secs.max(0.0) * cached.sample_rate as f32 * ch).round() as usize;
    let skip = skip.min(cached.samples.len());
    let max_ahead = PCM_MAX_SAMPLES;
    let end = (skip + max_ahead).min(cached.samples.len());
    AudioPrepareResult {
        channels: cached.channels,
        sample_rate: cached.sample_rate,
        samples: cached.samples[skip..end].to_vec(),
    }
}

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
    if (gb - 1.0).abs() < 0.02 && (gm - 1.0).abs() < 0.02 && (gt - 1.0).abs() < 0.02 {
        return;
    }
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

pub fn probe_media_duration_symphonia(path: &str) -> Option<f32> {
    if crate::document::AvClip::path_is_still_image(path) {
        return None;
    }
    let path = Path::new(path);
    let meta = std::fs::metadata(path).ok()?;
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
        let rate = params.sample_rate.unwrap_or(0);
        if rate > 0 {
            best_secs = best_secs.max(n_frames as f64 / rate as f64);
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

fn write_wav(path: &Path, samples: &[i16], rate: u32, channels: u16) -> Result<(), String> {
    write_wav_hound(path, samples, rate, channels)
}

fn write_wav_hound(
    path: &Path,
    samples: &[i16],
    rate: u32,
    channels: u16,
) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    {
        let mut writer =
            hound::WavWriter::create(path, spec).map_err(|e| format!("wav create: {e}"))?;
        for &s in samples {
            writer.write_sample(s).map_err(|e| format!("wav sample: {e}"))?;
        }
        writer.finalize().map_err(|e| format!("wav finalize: {e}"))?;
    }
    if let Ok(f) = std::fs::OpenOptions::new().read(true).write(true).open(path) {
        let _ = f.sync_all();
    }
    Ok(())
}

fn resample_interleaved_to_stereo(
    pcm: &[i16],
    src_rate: u32,
    src_channels: u16,
    out_rate: u32,
) -> Vec<i16> {
    let src_ch = src_channels.max(1) as usize;
    let frames = pcm.len() / src_ch;
    if frames == 0 {
        return Vec::new();
    }
    let sample_at = |frame: usize, ch: usize| -> f32 {
        let f = frame.min(frames - 1);
        let c = ch.min(src_ch - 1);
        pcm[f * src_ch + c] as f32 / i16::MAX as f32
    };
    let out_frames = ((frames as f64) * out_rate as f64 / src_rate.max(1) as f64).round() as usize;
    let mut out = Vec::with_capacity(out_frames * 2);
    for i in 0..out_frames {
        let src_f = i as f64 * src_rate as f64 / out_rate.max(1) as f64;
        let idx = src_f.floor() as usize;
        let frac = (src_f - idx as f64) as f32;
        for ch in 0..2 {
            let s0 = sample_at(idx, ch.min(src_ch - 1));
            let s1 = sample_at(idx + 1, ch.min(src_ch - 1));
            let s = s0 + (s1 - s0) * frac;
            out.push((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
        }
    }
    out
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

pub fn write_mono_f32_as_wav(mono: &[f32], src_rate: u32, output: &Path) -> Result<(), String> {
    let pcm: Vec<i16> = mono
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect();
    let stereo = resample_interleaved_to_stereo(&pcm, src_rate, 1, OUT_RATE);
    write_wav(output, &stereo, OUT_RATE, OUT_CHANNELS)
}
