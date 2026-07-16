//! OS screen capture → sibling `.mp4` + mouse track → canonical `.sepscrr`.
//!
//! **Wayland / COSMIC**
//! - xdg-desktop-portal **ScreenCast** (ashpd) → PipeWire node FD
//! - Continuous PipeWire buffers (`MAP_BUFFERS`) → RGBA → [`SyncRecorder`] (libav)
//! - Not a screenshot reel — real stream frames from the portal
//!
//! **X11**
//! - `ffmpeg` x11grab (video only; prefer Wayland path when available)
//!
//! **Mouse** (global desktop — not the app window)
//! - **Primary (Wayland):** PipeWire ScreenCast `SPA_META_Cursor` (compositor global position)
//! - Fallback: `/dev/input` relative (needs `input` group) + X11 `device_query` when live
//!
//! **Audio** (optional): in-process **PipeWire** sink-monitor capture → PCM → libav AAC remux
//! (no `ffmpeg` / cpal-ALSA).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::document::septic::{MouseSample, SepticMeta, SepticSession, SEPSCRR_VERSION};
use crate::recorder::{Frame, RecorderConfig, SyncRecorder};

/// Shared capture timeline: mouse + video both use the **encoder** media clock.
///
/// Important: wall-clock mouse stamps + CFR frame index desync when x264 lags —
/// video content at frame N is “live” at write time, but PTS = N/fps is earlier,
/// so the mouse track looked 10–15 frames behind the picture. We stamp mouse from
/// encoder progress (`frames/fps` + small inter-frame headroom).
struct CaptureClock {
    /// Instant of first encoded / grabbed video frame (`None` until video is live).
    t0: Mutex<Option<Instant>>,
    /// Frames fully written (CFR index of next frame = this value).
    frames_written: AtomicU64,
    fps: AtomicU32,
    /// Wall time of last `note_frame_written` (for sub-frame mouse between encodes).
    last_write: Mutex<Option<Instant>>,
    /// Final video duration in seconds (set by encoder on stop).
    video_duration_sec: Mutex<Option<f64>>,
}

impl CaptureClock {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            t0: Mutex::new(None),
            frames_written: AtomicU64::new(0),
            fps: AtomicU32::new(60),
            last_write: Mutex::new(None),
            video_duration_sec: Mutex::new(None),
        })
    }

    fn set_fps(&self, fps: u32) {
        self.fps.store(fps.max(1), Ordering::Relaxed);
    }

    /// Mark timeline origin once (first video frame). Returns true if this call set it.
    fn mark_video_start(&self) -> bool {
        let Ok(mut g) = self.t0.lock() else {
            return false;
        };
        if g.is_none() {
            let now = Instant::now();
            *g = Some(now);
            if let Ok(mut lw) = self.last_write.lock() {
                *lw = Some(now);
            }
            true
        } else {
            false
        }
    }

    /// Call after each encoded frame is written (`frames` = total written so far).
    fn note_frame_written(&self, frames: u64) {
        self.frames_written.store(frames, Ordering::Release);
        if let Ok(mut lw) = self.last_write.lock() {
            *lw = Some(Instant::now());
        }
    }

    fn is_running(&self) -> bool {
        self.t0.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    /// Encoder-aligned media seconds, or `None` if video has not started.
    ///
    /// = `(frames_written - 1)/fps` for the last frame, plus elapsed since that write
    /// capped to one frame duration (so mouse still moves between CFR ticks without
    /// racing ahead of the picture when the encoder is behind wall clock).
    fn media_sec(&self) -> Option<f64> {
        let _ = self.t0.lock().ok()?.as_ref()?;
        let fps = self.fps.load(Ordering::Relaxed).max(1) as f64;
        let frames = self.frames_written.load(Ordering::Acquire);
        if frames == 0 {
            return Some(0.0);
        }
        let base = (frames - 1) as f64 / fps;
        let frame_dt = 1.0 / fps;
        let partial = self
            .last_write
            .lock()
            .ok()
            .and_then(|g| g.map(|t| t.elapsed().as_secs_f64()))
            .unwrap_or(0.0)
            .clamp(0.0, frame_dt * 0.99);
        Some(base + partial)
    }

    /// Exact PTS of the last written frame (no inter-frame partial).
    fn last_frame_media_sec(&self) -> Option<f64> {
        let _ = self.t0.lock().ok()?.as_ref()?;
        let fps = self.fps.load(Ordering::Relaxed).max(1) as f64;
        let frames = self.frames_written.load(Ordering::Acquire);
        if frames == 0 {
            return Some(0.0);
        }
        Some((frames - 1) as f64 / fps)
    }

    fn set_video_duration(&self, secs: f64) {
        if let Ok(mut g) = self.video_duration_sec.lock() {
            *g = Some(secs.max(0.0));
        }
    }

    fn video_duration(&self) -> Option<f64> {
        self.video_duration_sec.lock().ok().and_then(|g| *g)
    }
}

/// Latest pointer for the encoder to stamp on each frame (keeps mouse 1:1 with picture).
#[derive(Clone, Copy, Debug)]
struct LivePointer {
    x: f64,
    y: f64,
    button_down: bool,
    /// Monotonic updates from mouse thread.
    seq: u64,
}

impl Default for LivePointer {
    fn default() -> Self {
        Self {
            x: 0.5,
            y: 0.5,
            button_down: false,
            seq: 0,
        }
    }
}

/// Live capture for one Screen Record layer.
pub struct ScreenCaptureSession {
    pub layer_id: uuid::Uuid,
    pub sepscrr_path: PathBuf,
    pub video_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub capture_cursor: bool,
    pub capture_audio: bool,
    pub backend: CaptureBackend,
    started: Instant,
    stop: Arc<AtomicBool>,
    clock: Arc<CaptureClock>,
    samples: Arc<Mutex<Vec<MouseSample>>>,
    mouse_join: Option<JoinHandle<()>>,
    /// Wayland: frame-grab + encode thread. X11: unused.
    video_join: Option<JoinHandle<Result<(), String>>>,
    /// X11 video: ffmpeg child. Wayland: unused for video.
    encoder: Option<Child>,
    /// Optional system-audio capture (cpal → PCM; muxed with libav on stop).
    audio_cap: Option<SystemAudioCapture>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureBackend {
    X11Grab,
    WaylandPortalRust,
}

#[derive(Debug, Clone)]
pub struct ScreenCaptureStart {
    pub layer_id: uuid::Uuid,
    pub sepscrr_path: PathBuf,
    pub capture_cursor: bool,
    pub capture_audio: bool,
    /// Target container FPS (default 60). Portal unique shots may be lower; frames are padded.
    pub fps: u32,
    /// Video bitrate in kbps. `0` = auto from resolution × fps.
    pub bitrate_kbps: u32,
}

/// Resolve encode bitrate: explicit kbps or auto from pixels × fps.
fn resolve_bitrate_kbps(requested: u32, width: u32, height: u32, fps: u32) -> u32 {
    if requested > 0 {
        return requested.clamp(500, 80_000);
    }
    (((width as u64 * height as u64 * fps as u64) / 700).clamp(6_000, 20_000)) as u32
}

/// Legacy no-op: screen record uses **global** cursor only (not the app window).
/// Kept so call sites compile; does not feed the capture track.
pub fn push_app_pointer(_x: f64, _y: f64, _button_down: bool) {}

pub fn clear_app_pointer() {}

pub fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|s| s.eq_ignore_ascii_case("wayland"))
            .unwrap_or(false)
}

impl ScreenCaptureSession {
    pub fn start(cfg: ScreenCaptureStart) -> Result<Self, String> {
        #[cfg(target_os = "android")]
        {
            let _ = cfg;
            return Err("Screen capture is not available on Android".into());
        }
        #[cfg(not(target_os = "android"))]
        {
            if is_wayland_session() {
                start_wayland_portal_rust(cfg)
            } else {
                start_x11grab(cfg)
            }
        }
    }

    pub fn elapsed_sec(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    pub fn sample_count(&self) -> usize {
        self.samples.lock().map(|s| s.len()).unwrap_or(0)
    }

    pub fn stop(mut self) -> Result<PathBuf, String> {
        self.stop.store(true, Ordering::SeqCst);
        clear_app_pointer();

        if let Some(join) = self.mouse_join.take() {
            let _ = join.join();
        }

        let mut video_err: Option<String> = None;
        if let Some(join) = self.video_join.take() {
            match join.join() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => video_err = Some(e),
                Err(_) => video_err = Some("video thread panicked".into()),
            }
        }

        if let Some(mut child) = self.encoder.take() {
            stop_ffmpeg_child(&mut child, &mut video_err);
            // X11: media length ≈ wall since video start (clock marked at spawn).
            if self.clock.video_duration().is_none() {
                if let Some(secs) = self.clock.media_sec() {
                    self.clock.set_video_duration(secs);
                }
            }
        }

        // Stop in-process audio capture, then mux with libav (no ffmpeg CLI).
        if let Some(ac) = self.audio_cap.take() {
            match ac.finish() {
                Ok(Some((pcm, rate))) if !pcm.is_empty() && self.video_path.is_file() => {
                    if let Err(e) = mux_pcm_into_video_libav(&self.video_path, &pcm, rate) {
                        log::warn!("[screen] audio mux (libav): {e}");
                        if video_err.is_none() {
                            video_err = Some(format!("audio mux: {e}"));
                        }
                    }
                }
                Ok(None) | Ok(Some(_)) => {
                    log::warn!("[screen] no system audio samples captured");
                }
                Err(e) => {
                    log::warn!("[screen] audio capture stop: {e}");
                    if video_err.is_none() {
                        video_err = Some(format!("audio: {e}"));
                    }
                }
            }
        }

        finalize_session(&self, video_err)
    }
}

fn stop_ffmpeg_child(child: &mut Child, video_err: &mut Option<String>) {
    // Never write interactive keys to ffmpeg — with a TTY that opens the
    // "Enter command:" console (arrow keys → "Parse error...").
    // Close stdin first, then SIGINT for a clean muxer flush, then SIGKILL.
    drop(child.stdin.take());
    #[cfg(unix)]
    unix_kill(child.id() as i32, 2); // SIGINT — finalize file
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match child.try_wait() {
            Ok(Some(st)) if !st.success() => {
                // SIGINT exit is often non-zero even when the file is fine.
                if video_err.is_none() && st.code() != Some(255) && st.code() != Some(130) {
                    *video_err = Some(format!("ffmpeg exited {st}"));
                }
                break;
            }
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
}

// ── In-process system audio (PipeWire sink monitor) + libav mux ────────────

/// Live PipeWire capture of the **default sink monitor** (what you hear) → stereo i16.
/// Avoids cpal/ALSA `POLLERR` on monitor devices under PipeWire.
struct SystemAudioCapture {
    stop: Arc<AtomicBool>,
    pcm: Arc<Mutex<Vec<i16>>>,
    sample_rate: Arc<std::sync::atomic::AtomicU32>,
    join: Option<JoinHandle<Result<(), String>>>,
}

impl SystemAudioCapture {
    fn start() -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let pcm = Arc::new(Mutex::new(Vec::with_capacity(48_000 * 2 * 60)));
        let sample_rate = Arc::new(std::sync::atomic::AtomicU32::new(48_000));
        let stop_t = stop.clone();
        let pcm_t = pcm.clone();
        let rate_t = sample_rate.clone();

        let join = std::thread::Builder::new()
            .name("vadadee-pw-audio".into())
            .spawn(move || run_pipewire_audio_capture(stop_t, pcm_t, rate_t))
            .map_err(|e| format!("audio thread: {e}"))?;

        // Brief wait to see if the thread dies immediately.
        std::thread::sleep(Duration::from_millis(80));
        if join.is_finished() {
            return match join.join() {
                Ok(Ok(())) => Err("audio thread exited immediately".into()),
                Ok(Err(e)) => Err(e),
                Err(_) => Err("audio thread panicked".into()),
            };
        }

        log::info!("[screen] PipeWire sink-monitor audio capture started");
        Ok(Self {
            stop,
            pcm,
            sample_rate,
            join: Some(join),
        })
    }

    fn finish(mut self) -> Result<Option<(Vec<i16>, u32)>, String> {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            // Don't fail the whole record if audio teardown is noisy.
            match j.join() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::warn!("[screen] pw audio stop: {e}"),
                Err(_) => log::warn!("[screen] pw audio thread panicked"),
            }
        }
        let rate = self.sample_rate.load(Ordering::Relaxed).max(8_000);
        let pcm = self
            .pcm
            .lock()
            .map_err(|_| "audio pcm lock poisoned".to_string())?
            .clone();
        if pcm.len() < 256 {
            return Ok(None);
        }
        Ok(Some((pcm, rate)))
    }
}

/// PipeWire audio capture thread: default sink monitor → interleaved stereo i16 @ graph rate.
#[cfg(target_os = "linux")]
fn run_pipewire_audio_capture(
    stop: Arc<AtomicBool>,
    pcm: Arc<Mutex<Vec<i16>>>,
    sample_rate_out: Arc<std::sync::atomic::AtomicU32>,
) -> Result<(), String> {
    use pipewire as pw;
    use pw::{properties::properties, spa};
    use spa::pod::Pod;
    use std::mem;

    struct AudioData {
        format: spa::param::audio::AudioInfoRaw,
        pcm: Arc<Mutex<Vec<i16>>>,
        sample_rate_out: Arc<std::sync::atomic::AtomicU32>,
    }

    // Safe to call multiple times; video thread may have called already.
    pw::init();

    let mainloop = pw::main_loop::MainLoop::new(None).map_err(|e| format!("pw audio loop: {e}"))?;
    let context =
        pw::context::Context::new(&mainloop).map_err(|e| format!("pw audio context: {e}"))?;
    let core = context
        .connect(None)
        .map_err(|e| format!("pw audio connect: {e}"))?;

    // Capture the default *sink monitor* (what speakers play), not a microphone.
    let props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Screen",
        *pw::keys::STREAM_CAPTURE_SINK => "true",
    };

    let stream = pw::stream::Stream::new(&core, "vadadee-screencast-audio", props)
        .map_err(|e| format!("pw audio stream: {e}"))?;

    let data = AudioData {
        format: Default::default(),
        pcm: pcm.clone(),
        sample_rate_out: sample_rate_out.clone(),
    };

    let _listener = stream
        .add_local_listener_with_user_data(data)
        .param_changed(|_, user_data, id, param| {
            let Some(param) = param else {
                return;
            };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Ok((media_type, media_subtype)) = spa::param::format_utils::parse_format(param)
            else {
                return;
            };
            if media_type != spa::param::format::MediaType::Audio
                || media_subtype != spa::param::format::MediaSubtype::Raw
            {
                return;
            }
            if user_data.format.parse(param).is_err() {
                return;
            }
            let rate = user_data.format.rate();
            if rate > 0 {
                user_data
                    .sample_rate_out
                    .store(rate, Ordering::Relaxed);
            }
            log::info!(
                "[screen] pw audio format: {} Hz, {} ch, {:?}",
                user_data.format.rate(),
                user_data.format.channels(),
                user_data.format.format()
            );
        })
        .process(|stream, user_data| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            if datas.is_empty() {
                return;
            }
            let data = &mut datas[0];
            let chunk = data.chunk();
            let size = chunk.size() as usize;
            let offset = chunk.offset() as usize;
            if size == 0 {
                return;
            }
            let Some(map) = data.data() else {
                return;
            };
            let end = offset.saturating_add(size).min(map.len());
            if offset >= end {
                return;
            }
            let bytes = &map[offset..end];
            let ch = user_data.format.channels().max(1) as usize;
            let fmt = user_data.format.format();
            // Prefer F32LE (PipeWire native); also accept S16LE.
            use spa::param::audio::AudioFormat;
            if let Ok(mut g) = user_data.pcm.lock() {
                match fmt {
                    AudioFormat::F32LE => {
                        let n = bytes.len() / 4;
                        for i in 0..n {
                            let start = i * 4;
                            if start + 4 > bytes.len() {
                                break;
                            }
                            let f = f32::from_le_bytes([
                                bytes[start],
                                bytes[start + 1],
                                bytes[start + 2],
                                bytes[start + 3],
                            ]);
                            // De-interleave to stereo pairs: for multi-ch take L/R or mono→stereo.
                            let _ = (ch, f); // handled below in frames
                        }
                        // Frame-wise
                        let samples = n / ch.max(1);
                        for frame in 0..samples {
                            let base = frame * ch;
                            let l = {
                                let i = base * 4;
                                if i + 4 <= bytes.len() {
                                    f32::from_le_bytes([
                                        bytes[i],
                                        bytes[i + 1],
                                        bytes[i + 2],
                                        bytes[i + 3],
                                    ])
                                } else {
                                    0.0
                                }
                            };
                            let r = if ch >= 2 {
                                let i = (base + 1) * 4;
                                if i + 4 <= bytes.len() {
                                    f32::from_le_bytes([
                                        bytes[i],
                                        bytes[i + 1],
                                        bytes[i + 2],
                                        bytes[i + 3],
                                    ])
                                } else {
                                    l
                                }
                            } else {
                                l
                            };
                            g.push((l.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                            g.push((r.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                        }
                    }
                    AudioFormat::S16LE => {
                        let n = bytes.len() / 2;
                        let samples = n / ch.max(1);
                        for frame in 0..samples {
                            let base = frame * ch;
                            let l = {
                                let i = base * 2;
                                if i + 2 <= bytes.len() {
                                    i16::from_le_bytes([bytes[i], bytes[i + 1]])
                                } else {
                                    0
                                }
                            };
                            let r = if ch >= 2 {
                                let i = (base + 1) * 2;
                                if i + 2 <= bytes.len() {
                                    i16::from_le_bytes([bytes[i], bytes[i + 1]])
                                } else {
                                    l
                                }
                            } else {
                                l
                            };
                            g.push(l);
                            g.push(r);
                        }
                    }
                    _ => {
                        // Unsupported format — skip buffer (don't spam).
                    }
                }
            }
            let _ = mem::size_of::<f32>();
        })
        .register()
        .map_err(|e| format!("pw audio register: {e}"))?;

    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
    let obj = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let values: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .map_err(|e| format!("pw audio pod: {e:?}"))?
    .0
    .into_inner();
    let mut params = [Pod::from_bytes(&values).ok_or_else(|| "pw audio bad pod".to_string())?];

    stream
        .connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|e| format!("pw audio connect: {e}"))?;

    // Poll stop on the mainloop thread (same pattern as video).
    let ml_quit = mainloop.clone();
    let stop_t = stop.clone();
    let timer = mainloop.loop_().add_timer(move |_| {
        if stop_t.load(Ordering::Relaxed) {
            ml_quit.quit();
        }
    });
    let _ = timer
        .update_timer(
            Some(Duration::from_millis(40)),
            Some(Duration::from_millis(40)),
        )
        .into_result();

    mainloop.run();
    drop(timer);
    drop(_listener);
    drop(stream);
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn run_pipewire_audio_capture(
    _stop: Arc<AtomicBool>,
    _pcm: Arc<Mutex<Vec<i16>>>,
    _sample_rate_out: Arc<std::sync::atomic::AtomicU32>,
) -> Result<(), String> {
    Err("PipeWire audio capture is Linux-only".into())
}

/// Encode stereo PCM as AAC (libav) and remux onto the video file (libav). No CLI.
fn mux_pcm_into_video_libav(video: &Path, pcm_stereo_i16: &[i16], sample_rate: u32) -> Result<(), String> {
    if !crate::video_decode::is_libav_available() {
        return Err("libav not available for audio mux".into());
    }
    let work = video.with_extension("rec_a.m4a");
    let out_tmp = video.with_extension("with_a.mp4");
    let _ = std::fs::remove_file(&work);
    let _ = std::fs::remove_file(&out_tmp);

    crate::video_decode::write_stereo_i16_as_aac_mp4_libav(
        &work,
        pcm_stereo_i16,
        sample_rate.max(8000),
        192,
        |_| {},
    )?;
    crate::video_decode::remux_video_and_audio_libav(video, &work, &out_tmp)?;
    std::fs::rename(&out_tmp, video).map_err(|e| {
        let _ = std::fs::remove_file(&out_tmp);
        format!("rename muxed: {e}")
    })?;
    let _ = std::fs::remove_file(&work);
    log::info!(
        "[screen] libav muxed audio ({} samples @ {} Hz) into {}",
        pcm_stereo_i16.len() / 2,
        sample_rate,
        video.display()
    );
    Ok(())
}

fn finalize_session(
    sess: &ScreenCaptureSession,
    video_err: Option<String>,
) -> Result<PathBuf, String> {
    let mut samples = sess
        .samples
        .lock()
        .map_err(|_| "mouse samples lock poisoned".to_string())?
        .clone();
    // Drop any pre-sync junk and sort by media time.
    samples.retain(|s| s.t.is_finite() && s.t >= -1e-6);
    for s in &mut samples {
        s.t = s.t.max(0.0);
        s.x = s.x.clamp(0.0, 1.0);
        s.y = s.y.clamp(0.0, 1.0);
    }
    samples.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));

    // Prefer encoder-reported duration (aligned to mouse media clock).
    let duration = sess
        .clock
        .video_duration()
        .or_else(|| sess.clock.media_sec())
        .unwrap_or_else(|| sess.started.elapsed().as_secs_f64())
        .max(samples.last().map(|s| s.t).unwrap_or(0.0));

    // Stretch / clamp last mouse sample to full video duration for scrubbing end.
    if let Some(last) = samples.last().copied() {
        if duration > last.t + 1.0 / sess.fps.max(1) as f64 {
            samples.push(MouseSample {
                t: duration,
                x: last.x,
                y: last.y,
                button_down: last.button_down,
            });
        }
    }

    let video_name = sess
        .video_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("capture.mp4")
        .to_string();

    let video_ok = sess.video_path.is_file()
        && sess
            .video_path
            .metadata()
            .map(|m| m.len() > 1024)
            .unwrap_or(false);

    let session = SepticSession {
        meta: SepticMeta {
            version: SEPSCRR_VERSION,
            width: sess.width,
            height: sess.height,
            fps: sess.fps as f64,
            duration_sec: duration,
            normalized: true,
            cursor_in_pixels: sess.capture_cursor,
            video_path: if video_ok {
                video_name
            } else {
                String::new()
            },
            source_label: match sess.backend {
                CaptureBackend::X11Grab => "x11grab".into(),
                CaptureBackend::WaylandPortalRust => "portal-pipewire".into(),
            },
        },
        mouse: samples,
    };
    session.save_path(&sess.sepscrr_path)?;

    if !video_ok {
        let hint = video_err.unwrap_or_else(|| "video file missing".into());
        return Err(format!(
            "Mouse saved · video failed: {hint}"
        ));
    }
    log::info!(
        "[screen] wrote {} ({} mouse, {:.1}s, {:?})",
        sess.sepscrr_path.display(),
        session.mouse.len(),
        duration,
        sess.backend
    );
    Ok(sess.sepscrr_path.clone())
}

impl Drop for ScreenCaptureSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(mut child) = self.encoder.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = self.audio_cap.take().map(|ac| ac.finish());
    }
}

#[cfg(unix)]
fn unix_kill(pid: i32, sig: i32) {
    unsafe {
        unsafe extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        let _ = kill(pid, sig);
    }
}

#[cfg(unix)]
fn unix_fcntl(fd: i32, cmd: i32, arg: i32) -> i32 {
    unsafe {
        unsafe extern "C" {
            fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        }
        fcntl(fd, cmd, arg)
    }
}

/// `screen_w` / `screen_h` = **capture** size for pointer normalize (portal size_hint /
/// stream — matches video). Not the encode-scaled size, and not full multi-monitor
/// root unless that is what was captured.
/// `portal_cursor` — when set (Wayland ScreenCast), **preferred** global source.
/// `live` — always updated for the encoder to stamp per-frame (A/V lock).
fn spawn_mouse_thread(
    stop: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<MouseSample>>>,
    clock: Arc<CaptureClock>,
    screen_w: u32,
    screen_h: u32,
    portal_cursor: Option<Arc<Mutex<PortalCursor>>>,
    live: Arc<Mutex<LivePointer>>,
) -> Result<JoinHandle<()>, String> {
    std::thread::Builder::new()
        .name("vadadee-screen-mouse".into())
        .spawn(move || {
            let mut tracker = MouseTracker::new(screen_w, screen_h);
            let mut armed = false;
            let mut live_seq = 0u64;
            // Portal SPA_META_Cursor when present (true global, stream space).
            // Else hybrid /dev/input REL + sparse ABS warps (never noisy frozen ABS).
            let poll_dt = Duration::from_millis(2);
            while !stop.load(Ordering::Relaxed) {
                let mut portal = None;
                if let Some(ref pc) = portal_cursor {
                    if let Ok(g) = pc.lock() {
                        if g.valid {
                            portal = Some((g.x, g.y));
                        }
                    }
                }
                let fallback = tracker.poll();
                let (x, y, down) = match (portal, fallback) {
                    // Meta matches video stream coordinates — prefer when live.
                    (Some((px, py)), Some((_, _, bd))) => {
                        tracker.latch(px, py, bd);
                        (px, py, bd)
                    }
                    (Some((px, py)), None) => {
                        tracker.latch(px, py, false);
                        (px, py, false)
                    }
                    (None, Some((fx, fy, bd))) => (fx, fy, bd),
                    (None, None) => {
                        std::thread::sleep(poll_dt);
                        continue;
                    }
                };

                live_seq = live_seq.wrapping_add(1);
                if let Ok(mut g) = live.lock() {
                    g.x = x;
                    g.y = y;
                    g.button_down = down;
                    g.seq = live_seq;
                }

                if !clock.is_running() {
                    std::thread::sleep(poll_dt);
                    continue;
                }
                // Encoder-aligned time (not wall clock) — stays locked to picture PTS.
                let Some(t) = clock.media_sec() else {
                    std::thread::sleep(poll_dt);
                    continue;
                };
                if let Ok(mut s) = samples.lock() {
                    if !armed {
                        s.push(MouseSample {
                            t: 0.0,
                            x,
                            y,
                            button_down: down,
                        });
                        armed = true;
                    } else {
                        let push = s
                            .last()
                            .map(|p| {
                                // Dense track for smooth scrubbing; still dedupe micro-noise.
                                (p.x - x).abs() > 0.00015
                                    || (p.y - y).abs() > 0.00015
                                    || p.button_down != down
                                    || (t - p.t) >= 0.008
                            })
                            .unwrap_or(true);
                        // Never write a sample *before* the last one (encoder can stall).
                        let t_ok = s.last().map(|p| t + 1e-6 >= p.t).unwrap_or(true);
                        if push && t_ok {
                            s.push(MouseSample {
                                t,
                                x,
                                y,
                                button_down: down,
                            });
                        }
                    }
                }
                std::thread::sleep(poll_dt);
            }
            // Final sample at stop (encoder timeline).
            if let Some(t) = clock
                .last_frame_media_sec()
                .or_else(|| clock.media_sec())
            {
                let (x, y, down) = live
                    .lock()
                    .map(|g| (g.x, g.y, g.button_down))
                    .unwrap_or((0.5, 0.5, false));
                if let Ok(mut s) = samples.lock() {
                    let t = t.max(s.last().map(|p| p.t).unwrap_or(0.0));
                    s.push(MouseSample {
                        t,
                        x,
                        y,
                        button_down: down,
                    });
                }
            }
        })
        .map_err(|e| format!("mouse thread: {e}"))
}

/// Push a mouse sample locked to an encoded frame PTS (encoder thread).
fn push_mouse_at_frame(
    samples: &Mutex<Vec<MouseSample>>,
    live: &Mutex<LivePointer>,
    frame_index: u64,
    fps: u32,
) {
    let t = frame_index as f64 / fps.max(1) as f64;
    let (x, y, down) = live
        .lock()
        .map(|g| (g.x, g.y, g.button_down))
        .unwrap_or((0.5, 0.5, false));
    if let Ok(mut s) = samples.lock() {
        // Replace last sample if it is at the same PTS (mouse thread race).
        if let Some(last) = s.last_mut() {
            if (last.t - t).abs() < 1e-6 {
                last.x = x;
                last.y = y;
                last.button_down = down;
                return;
            }
        }
        if s.last().map(|p| t + 1e-9 >= p.t).unwrap_or(true) {
            s.push(MouseSample {
                t,
                x,
                y,
                button_down: down,
            });
        }
    }
}

struct MouseTracker {
    /// Capture region size (normalize output 0..1 to match video).
    width: f64,
    height: f64,
    /// Full X11 root / desktop size (device_query absolute space).
    full_w: f64,
    full_h: f64,
    /// Internal unclamped position (allows edge recovery when REL reverses).
    x: f64,
    y: f64,
    button_down: bool,
    evdev: Vec<std::fs::File>,
    /// Recent absolute samples for freeze detection: (norm_x, norm_y, Instant).
    abs_hist: Vec<(f64, f64, Instant)>,
    last_abs: Option<(f64, f64)>,
    abs_live: bool,
    rel_live: bool,
    /// Cached device_query handle (not Sync — lives only on the mouse thread).
    #[cfg(not(target_os = "android"))]
    dq: Option<device_query::DeviceState>,
}

impl MouseTracker {
    fn new(w: u32, h: u32) -> Self {
        // Capture dims from portal/stream — must match the video frame, not full root.
        let cap_w = w.max(1) as f64;
        let cap_h = h.max(1) as f64;
        let (full_w, full_h) = probe_x11_screen_size()
            .map(|(a, b)| (a.max(1) as f64, b.max(1) as f64))
            .unwrap_or((cap_w, cap_h));
        let mut evdev = Vec::new();
        if let Ok(rd) = std::fs::read_dir("/dev/input") {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if !n.starts_with("event") {
                    continue;
                }
                // Prefer pointer-like nodes; still open all event* (filtering by name is fragile).
                if let Ok(f) = std::fs::OpenOptions::new().read(true).open(e.path()) {
                    #[cfg(unix)]
                    {
                        use std::os::fd::AsRawFd;
                        let fd = f.as_raw_fd();
                        let flags = unix_fcntl(fd, 3, 0);
                        if flags >= 0 {
                            let _ = unix_fcntl(fd, 4, flags | 0x800);
                        }
                    }
                    evdev.push(f);
                }
            }
        }
        if evdev.is_empty() {
            log::error!(
                "[screen] no /dev/input access (not in 'input' group). \
                 Mouse track cannot follow the global cursor on Wayland. Fix: \
                 sudo usermod -aG input $USER   # then log out/in"
            );
        } else {
            log::info!(
                "[screen] mouse: {} input node(s); capture {cap_w:.0}x{cap_h:.0} full {full_w:.0}x{full_h:.0}",
                evdev.len()
            );
        }
        #[cfg(not(target_os = "android"))]
        let dq = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            device_query::DeviceState::new()
        }))
        .ok();
        #[cfg(not(target_os = "android"))]
        let (sx, sy, sdown, abs_ok) =
            poll_global_pointer_capture(dq.as_ref(), full_w, full_h, cap_w, cap_h);
        #[cfg(target_os = "android")]
        let (sx, sy, sdown, abs_ok) = (0.5, 0.5, false, false);
        // Seed from abs only if it looks real (not a frozen mid-screen XWayland ghost).
        let seed = abs_ok && !is_likely_frozen_mid(sx, sy);
        Self {
            width: cap_w,
            height: cap_h,
            full_w,
            full_h,
            x: if seed { sx } else { 0.5 },
            y: if seed { sy } else { 0.5 },
            button_down: sdown,
            evdev,
            abs_hist: Vec::new(),
            last_abs: if abs_ok { Some((sx, sy)) } else { None },
            abs_live: false,
            rel_live: false,
            #[cfg(not(target_os = "android"))]
            dq,
        }
    }

    /// Snap tracker to a trusted (portal) position so fallback doesn't jump later.
    fn latch(&mut self, x: f64, y: f64, button_down: bool) {
        self.x = x.clamp(0.0, 1.0);
        self.y = y.clamp(0.0, 1.0);
        self.button_down = button_down;
        self.last_abs = Some((self.x, self.y));
        self.abs_live = true;
        self.rel_live = true;
        self.abs_hist.clear();
    }

    fn report(&self) -> (f64, f64, bool) {
        (
            self.x.clamp(0.0, 1.0),
            self.y.clamp(0.0, 1.0),
            self.button_down,
        )
    }

    fn at_edge(&self) -> bool {
        let (x, y, _) = self.report();
        x <= 0.002 || x >= 0.998 || y <= 0.002 || y >= 0.998
    }

    fn push_abs_hist(&mut self, ax: f64, ay: f64) {
        let now = Instant::now();
        self.abs_hist.push((ax, ay, now));
        // Keep ~120ms of history.
        self.abs_hist
            .retain(|(_, _, t)| now.duration_since(*t) < Duration::from_millis(120));
    }

    /// Absolute path is “alive” if it moved meaningfully in the recent window.
    fn abs_is_moving(&self) -> bool {
        if self.abs_hist.len() < 2 {
            return false;
        }
        let (x0, y0, t0) = self.abs_hist[0];
        let (x1, y1, t1) = *self.abs_hist.last().unwrap();
        let dt = t1.duration_since(t0).as_secs_f64().max(1e-3);
        let dist = (x1 - x0).hypot(y1 - y0);
        // ≥ ~15% screen / sec of travel in the window, or a clear step.
        dist > 0.012 || dist / dt > 0.15
    }

    fn poll(&mut self) -> Option<(f64, f64, bool)> {
        // ── Relative /dev/input ───────────────────────────────────────────
        let (dx_px, dy_px, rel_btn, had_rel) = self.poll_evdev_deltas();
        if let Some(b) = rel_btn {
            self.button_down = b;
        }

        // ── Absolute (X11 / XWayland) ─────────────────────────────────────
        #[cfg(not(target_os = "android"))]
        let (ax, ay, adown, abs_ok) = poll_global_pointer_capture(
            self.dq.as_ref(),
            self.full_w,
            self.full_h,
            self.width,
            self.height,
        );
        #[cfg(target_os = "android")]
        let (ax, ay, adown, abs_ok) = (0.5, 0.5, false, false);

        if abs_ok {
            self.push_abs_hist(ax, ay);
            if adown {
                self.button_down = true;
            }
            self.last_abs = Some((ax, ay));
        }

        let abs_moving = abs_ok && self.abs_is_moving();
        let abs_frozen_mid =
            abs_ok && !abs_moving && is_likely_frozen_mid(ax, ay);

        if abs_moving {
            // Live absolute — matches on-screen pointer under X11/XWayland.
            // Soft-blend for one frame to avoid single-sample spikes.
            const BLEND: f64 = 0.72;
            self.x = self.x + (ax - self.x) * BLEND;
            self.y = self.y + (ay - self.y) * BLEND;
            self.abs_live = true;
            self.rel_live = false;
            if !adown {
                // Keep button from abs when not forced true above.
                #[cfg(not(target_os = "android"))]
                {
                    self.button_down = adown;
                }
            }
        } else if had_rel {
            // Absolute frozen → integrate relative. No hard clamp: keep a little
            // overshoot so reverse motion immediately leaves the edge (no stick).
            // Scale ≈ 1.0 matches pixel motion to capture size (was 1.35 → early edges).
            self.x += dx_px / self.width;
            self.y += dy_px / self.height;
            // Soft walls: allow slight overshoot, then spring back into range.
            const SLACK: f64 = 0.08;
            self.x = self.x.clamp(-SLACK, 1.0 + SLACK);
            self.y = self.y.clamp(-SLACK, 1.0 + SLACK);
            self.rel_live = true;

            // If we're pinned on an edge but abs is clearly *not* on that edge
            // (and not a frozen mid-screen ghost), resync — unsticks y=0 traps.
            if self.at_edge() && abs_ok && !abs_frozen_mid {
                let (rx, ry, _) = self.report();
                let abs_on_same_edge = (rx <= 0.01 && ax <= 0.04)
                    || (rx >= 0.99 && ax >= 0.96)
                    || (ry <= 0.01 && ay <= 0.04)
                    || (ry >= 0.99 && ay >= 0.96);
                if !abs_on_same_edge && (rx - ax).hypot(ry - ay) > 0.06 {
                    self.x = ax;
                    self.y = ay;
                    self.abs_live = true;
                }
            }
        } else if abs_ok && !abs_frozen_mid {
            // No REL this tick; if abs moved a real jump since last trusted pos, snap.
            let jump = (self.x.clamp(0.0, 1.0) - ax).hypot(self.y.clamp(0.0, 1.0) - ay);
            if jump > 0.10 {
                self.x = ax;
                self.y = ay;
                self.abs_live = true;
            } else if self.at_edge() && jump > 0.04 {
                // Idle but edge-stuck vs abs — pull free.
                self.x = ax;
                self.y = ay;
            }
            if abs_ok {
                self.button_down = adown;
            }
        }

        // Pull soft-overshoot back into 0..1 gently when idle (keeps report clean).
        if !had_rel && !abs_moving {
            if self.x < 0.0 {
                self.x = 0.0;
            } else if self.x > 1.0 {
                self.x = 1.0;
            }
            if self.y < 0.0 {
                self.y = 0.0;
            } else if self.y > 1.0 {
                self.y = 1.0;
            }
        }

        Some(self.report())
    }

    /// Sum REL/ABS deltas and optional left-button state from all open event nodes.
    fn poll_evdev_deltas(&mut self) -> (f64, f64, Option<bool>, bool) {
        let mut dx = 0.0_f64;
        let mut dy = 0.0_f64;
        let mut btn: Option<bool> = None;
        let mut any = false;
        let mut buf = [0u8; 24 * 64];
        for f in &mut self.evdev {
            loop {
                match f.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let mut off = 0;
                        while off + 24 <= n {
                            let typ = u16::from_ne_bytes([buf[off + 16], buf[off + 17]]);
                            let code = u16::from_ne_bytes([buf[off + 18], buf[off + 19]]);
                            let val = i32::from_ne_bytes([
                                buf[off + 20],
                                buf[off + 21],
                                buf[off + 22],
                                buf[off + 23],
                            ]);
                            off += 24;
                            match typ {
                                // EV_REL
                                2 => {
                                    any = true;
                                    if code == 0 {
                                        dx += val as f64;
                                    } else if code == 1 {
                                        dy += val as f64;
                                    }
                                }
                                // EV_ABS — absolute devices (touchpads report in their range)
                                3 => {
                                    // Don't treat ABS as pixel delta; apply as normalized snap below.
                                    if code == 0 || code == 1 {
                                        any = true;
                                        // Approximate: map large ranges to screen motion fraction.
                                        let span = if val.abs() > 4096 { 65535.0 } else { 1000.0 };
                                        if code == 0 {
                                            // Absolute touchpads jump — use as direct position later
                                            // via storing on self in caller; here use small step 0.
                                            let _ = span;
                                        }
                                    }
                                }
                                // EV_KEY BTN_LEFT
                                1 if code == 0x110 => {
                                    any = true;
                                    btn = Some(val != 0);
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => break,
                }
            }
        }
        (dx, dy, btn, any && (dx != 0.0 || dy != 0.0 || btn.is_some()))
    }
}

/// Absolute pointer mapped into **capture** 0..1 (matches video), not full desktop 0..1.
///
/// `full_*` = X11 root / device_query pixel space. `cap_*` = portal stream / selected
/// monitor. Without this, multi-monitor or fractional-scale roots pull the track left.
#[cfg(not(target_os = "android"))]
fn poll_global_pointer_capture(
    dq: Option<&device_query::DeviceState>,
    full_w: f64,
    full_h: f64,
    cap_w: f64,
    cap_h: f64,
) -> (f64, f64, bool, bool) {
    use device_query::DeviceQuery;
    let fw = full_w.max(1.0);
    let fh = full_h.max(1.0);
    let cw = cap_w.max(1.0);
    let ch = cap_h.max(1.0);
    let Some(dev) = dq else {
        return (0.5, 0.5, false, false);
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let m = dev.get_mouse();
        let px = m.coords.0 as f64;
        let py = m.coords.1 as f64;
        let (x, y) = map_abs_px_to_capture(px, py, fw, fh, cw, ch);
        let down = m.button_pressed.get(1).copied().unwrap_or(false)
            || m.button_pressed.first().copied().unwrap_or(false);
        (x, y, down)
    }));
    match result {
        Ok((x, y, down)) => (x, y, down, true),
        Err(_) => (0.5, 0.5, false, false),
    }
}

/// True when abs looks like a stuck XWayland ghost near mid-screen (not a real pointer).
fn is_likely_frozen_mid(x: f64, y: f64) -> bool {
    x > 0.38 && x < 0.62 && y > 0.38 && y < 0.62
}

/// Map absolute desktop pixels → capture-normalized 0..1 (half-pixel centered).
fn map_abs_px_to_capture(
    px: f64,
    py: f64,
    full_w: f64,
    full_h: f64,
    cap_w: f64,
    cap_h: f64,
) -> (f64, f64) {
    let fw = full_w.max(1.0);
    let fh = full_h.max(1.0);
    let cw = cap_w.max(1.0);
    let ch = cap_h.max(1.0);

    // Same size (or capture is the full root): simple normalize.
    if (fw - cw).abs() < 2.0 && (fh - ch).abs() < 2.0 {
        return (
            ((px + 0.5) / cw).clamp(0.0, 1.0),
            ((py + 0.5) / ch).clamp(0.0, 1.0),
        );
    }

    // Estimate capture origin inside the virtual desktop so a single selected
    // monitor lines up with the stream (left / right / top / bottom slab).
    let ox = if fw <= cw + 1.0 {
        0.0
    } else if px < cw {
        0.0
    } else if px >= fw - cw {
        fw - cw
    } else {
        (px - cw * 0.5).clamp(0.0, fw - cw)
    };
    let oy = if fh <= ch + 1.0 {
        0.0
    } else if py < ch {
        0.0
    } else if py >= fh - ch {
        fh - ch
    } else {
        (py - ch * 0.5).clamp(0.0, fh - ch)
    };

    (
        ((px - ox + 0.5) / cw).clamp(0.0, 1.0),
        ((py - oy + 0.5) / ch).clamp(0.0, 1.0),
    )
}

/// X11 root window size (same space as device_query coords).
fn probe_x11_screen_size() -> Option<(u32, u32)> {
    // Prefer $DISPLAY X server dimensions over Wayland portal hint.
    if let Ok(out) = Command::new("xdpyinfo").output() {
        let s = String::from_utf8_lossy(&out.stdout);
        for line in s.lines() {
            if let Some(rest) = line.trim().strip_prefix("dimensions:") {
                let tok = rest.split_whitespace().next()?;
                let (a, b) = tok.split_once('x')?;
                let w: u32 = a.parse().ok()?;
                let h: u32 = b
                    .trim_end_matches("pixels")
                    .trim()
                    .parse()
                    .ok()?;
                if w >= 320 && h >= 240 {
                    return Some((w, h));
                }
            }
        }
    }
    probe_screen_size()
}

fn sibling_video_path(sepscrr: &Path) -> PathBuf {
    sepscrr.with_extension("mp4")
}

fn safe_layer_stem(layer_name: &str) -> String {
    let safe: String = layer_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if safe.is_empty() {
        "capture".into()
    } else {
        safe
    }
}

/// Default save folder (XDG cache / `$HOME/.cache` / temp — never hardcodes a username).
pub fn default_capture_dir() -> PathBuf {
    dirs_screen_capture_dir()
}

/// Timestamped `.sepscrr` under the default cache folder.
pub fn default_sepscrr_path(layer_name: &str) -> PathBuf {
    sepscrr_in_dir(&dirs_screen_capture_dir(), layer_name)
}

/// Timestamped `.sepscrr` inside `dir` (creates the directory if needed).
pub fn sepscrr_in_dir(dir: &Path, layer_name: &str) -> PathBuf {
    let _ = std::fs::create_dir_all(dir);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    dir.join(format!("{}_{stamp}.sepscrr", safe_layer_stem(layer_name)))
}

/// Resolve where to write this take: `capture_dir` (folder), else default cache.
pub fn resolve_sepscrr_for_record(capture_dir: &str, layer_name: &str) -> PathBuf {
    let dir = capture_dir.trim();
    if dir.is_empty() {
        return default_sepscrr_path(layer_name);
    }
    let p = PathBuf::from(dir);
    if p.is_file() {
        // Legacy: was a .sepscrr path — use its parent.
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() {
                return sepscrr_in_dir(parent, layer_name);
            }
        }
        return default_sepscrr_path(layer_name);
    }
    sepscrr_in_dir(&p, layer_name)
}

fn dirs_screen_capture_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("vadadee-berry").join("screen");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".cache")
            .join("vadadee-berry")
            .join("screen");
    }
    std::env::temp_dir().join("vadadee-berry-screen")
}

fn probe_screen_size() -> Option<(u32, u32)> {
    if let Ok(out) = Command::new("xdpyinfo").output() {
        let s = String::from_utf8_lossy(&out.stdout);
        for line in s.lines() {
            if let Some(rest) = line.trim().strip_prefix("dimensions:") {
                let tok = rest.split_whitespace().next()?;
                let (w, h) = tok.split_once('x')?;
                let w: u32 = w.parse().ok()?;
                let h: u32 = h.trim_end_matches("pixels").trim().parse().ok()?;
                if w > 0 && h > 0 {
                    return Some((w, h));
                }
            }
        }
    }
    if let Ok(out) = Command::new("cosmic-randr").arg("list").output() {
        let s = String::from_utf8_lossy(&out.stdout);
        for tok in s.split_whitespace() {
            if let Some((w, h)) = tok.split_once('x') {
                if let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) {
                    if w >= 640 && h >= 480 {
                        return Some((w, h));
                    }
                }
            }
        }
    }
    None
}

#[cfg(not(target_os = "android"))]
fn start_x11grab(cfg: ScreenCaptureStart) -> Result<ScreenCaptureSession, String> {
    let (width, height) = probe_screen_size().unwrap_or((1920, 1080));
    let fps = cfg.fps.clamp(1, 120);
    let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
    if let Some(parent) = cfg.sepscrr_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }
    let video_path = sibling_video_path(&cfg.sepscrr_path);
    let _ = std::fs::remove_file(&video_path);

    let mut cmd = Command::new("ffmpeg");
    // Video from x11grab; optional second input = pulse monitor → AAC.
    // `-nostdin` + null stdin so stop never opens ffmpeg interactive console.
    cmd.args([
        "-hide_banner",
        "-nostdin",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "x11grab",
        "-draw_mouse",
        if cfg.capture_cursor { "1" } else { "0" },
        "-framerate",
        &fps.to_string(),
        "-video_size",
        &format!("{width}x{height}"),
        "-i",
        &format!("{display}+0,0"),
    ]);
    // Video only via x11grab CLI; system audio is in-process cpal + libav mux on stop.
    let bitrate_kbps = resolve_bitrate_kbps(cfg.bitrate_kbps, width, height, fps);
    cmd.args([
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-pix_fmt",
        "yuv420p",
        "-b:v",
        &format!("{bitrate_kbps}k"),
        "-maxrate",
        &format!("{bitrate_kbps}k"),
        "-bufsize",
        &format!("{}k", bitrate_kbps.saturating_mul(2)),
    ]);
    cmd.arg(&video_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let child = cmd.spawn().map_err(|e| format!("spawn ffmpeg: {e}"))?;
    let stop = Arc::new(AtomicBool::new(false));
    let samples = Arc::new(Mutex::new(Vec::with_capacity(4096)));
    let clock = CaptureClock::new();
    clock.set_fps(fps);
    // x11grab starts producing frames almost immediately after spawn.
    clock.mark_video_start();
    clock.note_frame_written(1); // treat as live CFR from t=0
    let live = Arc::new(Mutex::new(LivePointer::default()));
    let started = Instant::now();
    let mouse_join = spawn_mouse_thread(
        stop.clone(),
        samples.clone(),
        clock.clone(),
        width,
        height,
        None, // X11: no portal cursor meta
        live,
    )?;
    let audio_cap = if cfg.capture_audio {
        match SystemAudioCapture::start() {
            Ok(c) => Some(c),
            Err(e) => {
                log::warn!("[screen] audio capture unavailable: {e}");
                None
            }
        }
    } else {
        None
    };

    Ok(ScreenCaptureSession {
        layer_id: cfg.layer_id,
        sepscrr_path: cfg.sepscrr_path,
        video_path,
        width,
        height,
        fps,
        capture_cursor: cfg.capture_cursor,
        capture_audio: cfg.capture_audio,
        backend: CaptureBackend::X11Grab,
        started,
        stop,
        clock,
        samples,
        mouse_join: Some(mouse_join),
        video_join: None,
        encoder: Some(child),
        audio_cap,
    })
}

// ── Wayland: ScreenCast portal + PipeWire stream → SyncRecorder ──────────────

/// Global async-io friendly block_on for ashpd (no Tokio reactor required).
fn portal_block_on<F, T>(f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    pollster::block_on(f)
}

fn scale_rgba(rgba: &[u8], w: u32, h: u32, tw: u32, th: u32) -> Result<Vec<u8>, String> {
    if w == tw && h == th {
        return Ok(rgba.to_vec());
    }
    let src = image::RgbaImage::from_raw(w, h, rgba.to_vec())
        .ok_or_else(|| "bad rgba buffer".to_string())?;
    let resized =
        image::imageops::resize(&src, tw, th, image::imageops::FilterType::Triangle);
    Ok(resized.into_raw())
}

fn cap_encode_size(w: u32, h: u32, max_w: u32) -> (u32, u32) {
    if w <= max_w {
        return (w & !1, h & !1);
    }
    let scale = max_w as f32 / w as f32;
    let nw = max_w & !1;
    let nh = ((h as f32 * scale) as u32).max(2) & !1;
    (nw, nh)
}

/// Shared latest RGBA frame from the PipeWire process callback.
struct PwFrameSlot {
    /// Native stream size (pixels).
    width: u32,
    height: u32,
    /// Full RGBA (width * height * 4).
    rgba: Option<Vec<u8>>,
    /// Monotonic sequence; encoder detects new buffers via this.
    seq: u64,
    /// True after first valid buffer.
    has_frame: bool,
}

/// Global cursor from ScreenCast stream metadata (normalized 0..1, screen Y-down).
/// This is the only reliable global pointer under Wayland (compositor owns /dev/input).
#[derive(Clone, Copy, Debug, Default)]
struct PortalCursor {
    x: f64,
    y: f64,
    /// True once we have seen at least one valid SPA_META_Cursor.
    valid: bool,
    seq: u64,
}

/// Open ScreenCast session → PipeWire node id + remote FD.
/// Keeps the portal session alive for the duration of the recording thread.
#[cfg(all(target_os = "linux", not(target_os = "android")))]
struct PortalPwRemote {
    /// Dropping this ends the screencast on some portals.
    _session: ashpd::desktop::Session<ashpd::desktop::screencast::Screencast>,
    node_id: u32,
    fd: std::os::fd::OwnedFd,
    /// Compositor-reported size hint (may differ from buffer size).
    size_hint: Option<(u32, u32)>,
}

#[cfg(all(target_os = "linux", not(target_os = "android")))]
async fn open_screencast_remote(capture_cursor: bool) -> Result<PortalPwRemote, String> {
    use ashpd::desktop::{
        PersistMode,
        screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType},
    };

    let proxy = Screencast::new()
        .await
        .map_err(|e| format!("screencast portal: {e}"))?;
    let session = proxy
        .create_session(Default::default())
        .await
        .map_err(|e| format!("screencast session: {e}"))?;

    // Prefer Metadata (SPA_META_Cursor → global mouse track). COSMIC and some portals
    // only advertise Hidden|Embedded (AvailableCursorModes=3) — Metadata alone fails
    // SelectSources and Record appears to "do nothing".
    let available = proxy
        .available_cursor_modes()
        .await
        .unwrap_or_else(|_| CursorMode::Hidden | CursorMode::Embedded);
    let cursor = if available.contains(CursorMode::Metadata) {
        CursorMode::Metadata
    } else if capture_cursor && available.contains(CursorMode::Embedded) {
        CursorMode::Embedded
    } else if available.contains(CursorMode::Hidden) {
        CursorMode::Hidden
    } else if available.contains(CursorMode::Embedded) {
        CursorMode::Embedded
    } else {
        CursorMode::Hidden
    };
    log::info!(
        "[screen] AvailableCursorModes={available:?} → requesting {cursor:?} (capture_cursor={capture_cursor})"
    );

    proxy
        .select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_cursor_mode(cursor)
                .set_sources(SourceType::Monitor | SourceType::Window)
                .set_multiple(false)
                .set_persist_mode(PersistMode::DoNot),
        )
        .await
        .map_err(|e| format!("select_sources: {e}"))?;

    let response = proxy
        .start(&session, None, Default::default())
        .await
        .map_err(|e| format!("screencast start: {e}"))?
        .response()
        .map_err(|e| format!("screencast response: {e}"))?;

    let stream = response
        .streams()
        .first()
        .ok_or_else(|| "no screencast stream selected".to_string())?
        .clone();

    let node_id = stream.pipe_wire_node_id();
    let size_hint = stream.size().and_then(|(w, h)| {
        if w > 0 && h > 0 {
            Some((w as u32, h as u32))
        } else {
            None
        }
    });

    let fd = proxy
        .open_pipe_wire_remote(&session, Default::default())
        .await
        .map_err(|e| format!("open_pipe_wire_remote: {e}"))?;

    log::info!(
        "[screen] ScreenCast node={node_id} size_hint={size_hint:?} cursor={capture_cursor}"
    );

    Ok(PortalPwRemote {
        _session: session,
        node_id,
        fd,
        size_hint,
    })
}

/// Convert a mapped PipeWire video buffer to tightly-packed RGBA.
#[cfg(all(target_os = "linux", not(target_os = "android")))]
fn pw_buffer_to_rgba(
    src: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    format: pipewire::spa::param::video::VideoFormat,
) -> Option<Vec<u8>> {
    use pipewire::spa::param::video::VideoFormat;

    let w = width as usize;
    let h = height as usize;
    if w == 0 || h == 0 {
        return None;
    }

    let bpp: usize = match format {
        VideoFormat::RGBA | VideoFormat::RGBx | VideoFormat::BGRA | VideoFormat::BGRx => 4,
        VideoFormat::RGB | VideoFormat::BGR => 3,
        _ => {
            log::warn!("[screen] unsupported PipeWire video format {format:?}");
            return None;
        }
    };

    let min_row = w.checked_mul(bpp)?;
    let stride = if stride >= min_row { stride } else { min_row };
    let need = stride.checked_mul(h)?.checked_add(0)?;
    if src.len() < need && src.len() < min_row.saturating_mul(h) {
        return None;
    }

    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        let row_off = y * stride;
        if row_off + min_row > src.len() {
            break;
        }
        let row = &src[row_off..row_off + min_row];
        let dst = &mut out[y * w * 4..(y + 1) * w * 4];
        match format {
            VideoFormat::RGBA => {
                dst.copy_from_slice(row);
            }
            VideoFormat::RGBx => {
                for x in 0..w {
                    let i = x * 4;
                    let o = x * 4;
                    dst[o] = row[i];
                    dst[o + 1] = row[i + 1];
                    dst[o + 2] = row[i + 2];
                    dst[o + 3] = 255;
                }
            }
            VideoFormat::BGRA => {
                for x in 0..w {
                    let i = x * 4;
                    let o = x * 4;
                    dst[o] = row[i + 2];
                    dst[o + 1] = row[i + 1];
                    dst[o + 2] = row[i];
                    dst[o + 3] = row[i + 3];
                }
            }
            VideoFormat::BGRx => {
                for x in 0..w {
                    let i = x * 4;
                    let o = x * 4;
                    dst[o] = row[i + 2];
                    dst[o + 1] = row[i + 1];
                    dst[o + 2] = row[i];
                    dst[o + 3] = 255;
                }
            }
            VideoFormat::RGB => {
                for x in 0..w {
                    let i = x * 3;
                    let o = x * 4;
                    dst[o] = row[i];
                    dst[o + 1] = row[i + 1];
                    dst[o + 2] = row[i + 2];
                    dst[o + 3] = 255;
                }
            }
            VideoFormat::BGR => {
                for x in 0..w {
                    let i = x * 3;
                    let o = x * 4;
                    dst[o] = row[i + 2];
                    dst[o + 1] = row[i + 1];
                    dst[o + 2] = row[i];
                    dst[o + 3] = 255;
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

/// SPA_META_Cursor = 5 (see spa/buffer/meta.h).
const SPA_META_CURSOR: u32 = 5;

/// Read global cursor from PipeWire `spa_buffer` metadata (normalized 0..1, Y down).
///
/// `spa_meta_cursor.position` is the hotspot on the stream surface (matches embedded
/// cursor tip). Half-pixel centering avoids a consistent ~0.5px left/up bias.
#[cfg(all(target_os = "linux", not(target_os = "android")))]
fn extract_spa_meta_cursor(spa_buf: *mut std::ffi::c_void, stream_w: u32, stream_h: u32) -> Option<(f64, f64)> {
    if spa_buf.is_null() || stream_w < 2 || stream_h < 2 {
        return None;
    }
    // Layout must match spa/buffer/buffer.h + meta.h (little-endian).
    unsafe {
        // spa_buffer { u32 n_metas; u32 n_datas; spa_meta *metas; spa_data *datas; }
        let n_metas = std::ptr::read_unaligned(spa_buf as *const u32);
        let metas_ptr = std::ptr::read_unaligned((spa_buf as *const u8).add(8) as *const *mut u8);
        if metas_ptr.is_null() || n_metas == 0 {
            return None;
        }
        for i in 0..n_metas as usize {
            // spa_meta { u32 type; u32 size; void *data; } = 16 bytes
            let m = metas_ptr.add(i * 16);
            let mtype = std::ptr::read_unaligned(m as *const u32);
            let msize = std::ptr::read_unaligned(m.add(4) as *const u32);
            let mdata = std::ptr::read_unaligned(m.add(8) as *const *const u8);
            if mtype != SPA_META_CURSOR || mdata.is_null() || msize < 20 {
                continue;
            }
            // spa_meta_cursor { u32 id; u32 flags; spa_point position; spa_point hotspot; ... }
            let id = std::ptr::read_unaligned(mdata as *const u32);
            if id == 0 {
                return None;
            }
            let px = std::ptr::read_unaligned(mdata.add(8) as *const i32) as f64;
            let py = std::ptr::read_unaligned(mdata.add(12) as *const i32) as f64;
            // Hotspot is already the tip in stream pixels; map with half-pixel for
            // UV/zoom centers that sample between texels. Do not add bitmap hotspot —
            // spa position is already the tip on the surface.
            let x = ((px + 0.5) / stream_w as f64).clamp(0.0, 1.0);
            let y = ((py + 0.5) / stream_h as f64).clamp(0.0, 1.0);
            return Some((x, y));
        }
    }
    None
}

/// Run PipeWire mainloop: connect to portal FD, MAP_BUFFERS, push RGBA + cursor into slots.
#[cfg(all(target_os = "linux", not(target_os = "android")))]
fn run_pipewire_capture(
    node_id: u32,
    fd: std::os::fd::OwnedFd,
    stop: Arc<AtomicBool>,
    slot: Arc<Mutex<PwFrameSlot>>,
    portal_cursor: Arc<Mutex<PortalCursor>>,
    fps_hint: u32,
) -> Result<(), String> {
    use pipewire as pw;
    use pw::{properties::properties, spa};
    use spa::pod::Pod;

    struct UserData {
        format: spa::param::video::VideoInfoRaw,
        slot: Arc<Mutex<PwFrameSlot>>,
        portal_cursor: Arc<Mutex<PortalCursor>>,
        cursor_logged: bool,
    }

    pw::init();

    let mainloop = pw::main_loop::MainLoop::new(None).map_err(|e| format!("pw mainloop: {e}"))?;
    let context =
        pw::context::Context::new(&mainloop).map_err(|e| format!("pw context: {e}"))?;
    let core = context
        .connect_fd(fd, None)
        .map_err(|e| format!("pw connect_fd: {e}"))?;

    let data = UserData {
        format: Default::default(),
        slot: slot.clone(),
        portal_cursor: portal_cursor.clone(),
        cursor_logged: false,
    };

    let stream = pw::stream::Stream::new(
        &core,
        "vadadee-screencast",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(|e| format!("pw stream: {e}"))?;

    let _listener = stream
        .add_local_listener_with_user_data(data)
        .state_changed(|_, _, old, new| {
            log::debug!("[screen] pw state {old:?} → {new:?}");
        })
        .param_changed(|_, user_data, id, param| {
            let Some(param) = param else {
                return;
            };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let (media_type, media_subtype) =
                match spa::param::format_utils::parse_format(param) {
                    Ok(v) => v,
                    Err(_) => return,
                };
            if media_type != spa::param::format::MediaType::Video
                || media_subtype != spa::param::format::MediaSubtype::Raw
            {
                return;
            }
            if user_data.format.parse(param).is_err() {
                return;
            }
            let sz = user_data.format.size();
            log::info!(
                "[screen] pw format {:?} {}x{} @ {}/{}",
                user_data.format.format(),
                sz.width,
                sz.height,
                user_data.format.framerate().num,
                user_data.format.framerate().denom
            );
        })
        .process(|stream, user_data| {
            // Raw buffer: need SPA_META_Cursor + mapped pixels.
            let pw_buf = unsafe { stream.dequeue_raw_buffer() };
            if pw_buf.is_null() {
                return;
            }
            unsafe {
                let spa_buf = (*pw_buf).buffer;
                let sz = user_data.format.size();
                let w = sz.width;
                let h = sz.height;
                if !spa_buf.is_null() && w >= 2 && h >= 2 {
                    if let Some((cx, cy)) =
                        extract_spa_meta_cursor(spa_buf as *mut std::ffi::c_void, w, h)
                    {
                        if let Ok(mut g) = user_data.portal_cursor.lock() {
                            g.x = cx;
                            g.y = cy;
                            g.valid = true;
                            g.seq = g.seq.wrapping_add(1);
                        }
                        if !user_data.cursor_logged {
                            user_data.cursor_logged = true;
                            log::info!(
                                "[screen] portal cursor meta live ({cx:.3},{cy:.3}) — global track"
                            );
                        }
                    }
                    let n_datas = (*spa_buf).n_datas;
                    let datas = (*spa_buf).datas;
                    if n_datas > 0 && !datas.is_null() {
                        let d0 = datas;
                        let data_ptr = (*d0).data as *const u8;
                        let maxsize = (*d0).maxsize as usize;
                        let chunk = (*d0).chunk;
                        if !data_ptr.is_null() && !chunk.is_null() {
                            let size = (*chunk).size as usize;
                            let offset = (*chunk).offset as usize;
                            let stride = (*chunk).stride.max(0) as usize;
                            if size > 0 {
                                let end = offset.saturating_add(size).min(maxsize);
                                if offset < end {
                                    let src = std::slice::from_raw_parts(
                                        data_ptr.add(offset),
                                        end - offset,
                                    );
                                    let fmt = user_data.format.format();
                                    if let Some(rgba) = pw_buffer_to_rgba(src, w, h, stride, fmt) {
                                        if let Ok(mut g) = user_data.slot.lock() {
                                            g.width = w;
                                            g.height = h;
                                            g.rgba = Some(rgba);
                                            g.seq = g.seq.wrapping_add(1);
                                            g.has_frame = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                stream.queue_raw_buffer(pw_buf);
            }
        })
        .register()
        .map_err(|e| format!("pw register: {e}"))?;

    let fr = fps_hint.clamp(1, 120);
    let obj = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::RGBA,
            spa::param::video::VideoFormat::RGB,
            spa::param::video::VideoFormat::BGR,
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: 1920,
                height: 1080
            },
            spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            spa::utils::Rectangle {
                width: 8192,
                height: 8192
            }
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction {
                num: fr,
                denom: 1
            },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction {
                num: 1000,
                denom: 1
            }
        ),
    );

    let values: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .map_err(|e| format!("pod serialize: {e:?}"))?
    .0
    .into_inner();

    let mut params = [Pod::from_bytes(&values).ok_or_else(|| "bad format pod".to_string())?];

    stream
        .connect(
            spa::utils::Direction::Input,
            Some(node_id),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(|e| format!("pw connect: {e}"))?;

    log::info!("[screen] PipeWire stream connected to node {node_id}");

    // Poll stop on the mainloop thread (WeakMainLoop is !Send — no quit thread).
    let ml_quit = mainloop.clone();
    let stop_t = stop.clone();
    let timer = mainloop.loop_().add_timer(move |_| {
        if stop_t.load(Ordering::Relaxed) {
            ml_quit.quit();
        }
    });
    timer
        .update_timer(
            Some(Duration::from_millis(40)),
            Some(Duration::from_millis(40)),
        )
        .into_result()
        .map_err(|e| format!("pw stop timer: {e}"))?;

    mainloop.run();
    // Keep sources alive until after run returns.
    drop(timer);
    drop(_listener);
    drop(stream);
    Ok(())
}

#[cfg(all(target_os = "linux", not(target_os = "android")))]
fn start_wayland_portal_rust(cfg: ScreenCaptureStart) -> Result<ScreenCaptureSession, String> {
    if let Some(parent) = cfg.sepscrr_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }
    let video_path = sibling_video_path(&cfg.sepscrr_path);
    let _ = std::fs::remove_file(&video_path);

    let fps = cfg.fps.clamp(1, 120);
    let bitrate_req = cfg.bitrate_kbps;
    let capture_cursor = cfg.capture_cursor;

    // Open portal on a worker so the UI thread never blocks on the picker dialog.
    let remote = std::thread::Builder::new()
        .name("vadadee-screencast-open".into())
        .spawn(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                portal_block_on(open_screencast_remote(capture_cursor))
            }))
            .unwrap_or_else(|_| Err("screencast open panicked".into()))
        })
        .map_err(|e| format!("screencast open thread: {e}"))?
        .join()
        .map_err(|_| "screencast open join failed".to_string())??;

    // Encode size (may be scaled down). Mouse must use **capture** size (portal
    // size_hint / stream) so 0..1 matches the video, not the full multi-monitor root.
    let (hint_w, hint_h) = remote.size_hint.unwrap_or((1920, 1080));
    let (ew, eh) = cap_encode_size(hint_w.max(2), hint_h.max(2), 1920);
    let (screen_w, screen_h) = remote
        .size_hint
        .or_else(probe_screen_size)
        .unwrap_or((hint_w.max(2), hint_h.max(2)));

    let stop = Arc::new(AtomicBool::new(false));
    let samples = Arc::new(Mutex::new(Vec::with_capacity(4096)));
    let clock = CaptureClock::new();
    clock.set_fps(fps);
    let portal_cursor = Arc::new(Mutex::new(PortalCursor::default()));
    let live = Arc::new(Mutex::new(LivePointer::default()));
    let started = Instant::now();
    // Live /dev/input + X11 primary; portal meta only as gap-fill. Encoder stamps
    // each frame PTS so mouse stays locked even if x264 lags wall clock.
    let mouse_join = spawn_mouse_thread(
        stop.clone(),
        samples.clone(),
        clock.clone(),
        screen_w,
        screen_h,
        Some(portal_cursor.clone()),
        live.clone(),
    )?;

    let audio_cap = if cfg.capture_audio {
        match SystemAudioCapture::start() {
            Ok(c) => Some(c),
            Err(e) => {
                log::warn!("[screen] audio capture unavailable: {e}");
                None
            }
        }
    } else {
        None
    };

    let slot = Arc::new(Mutex::new(PwFrameSlot {
        width: 0,
        height: 0,
        rgba: None,
        seq: 0,
        has_frame: false,
    }));

    let stop_v = stop.clone();
    let slot_pw = slot.clone();
    let slot_enc = slot.clone();
    let video_path_c = video_path.clone();
    let clock_enc = clock.clone();
    let portal_cursor_pw = portal_cursor.clone();
    // Clones for encoder; originals stay for ScreenCaptureSession / mouse thread.
    let samples_v = samples.clone();
    let live_v = live.clone();
    // Move fd + session into the video thread so the portal stays open.
    let video_join = std::thread::Builder::new()
        .name("vadadee-portal-pw".into())
        .spawn(move || -> Result<(), String> {
            let PortalPwRemote {
                _session,
                node_id,
                fd,
                size_hint: _,
            } = remote;

            // Encoder thread: wait for first continuous buffer, then CFR encode.
            let stop_e = stop_v.clone();
            let samples_enc = samples_v;
            let live_enc = live_v;
            let enc_join = std::thread::Builder::new()
                .name("vadadee-portal-enc".into())
                .spawn(move || -> Result<(), String> {
                    // Wait up to 15s for first PipeWire buffer.
                    let deadline = Instant::now() + Duration::from_secs(15);
                    let (native_w, native_h, first) = loop {
                        if stop_e.load(Ordering::Relaxed) {
                            return Err("stopped before first PipeWire frame".into());
                        }
                        if let Ok(g) = slot_enc.lock() {
                            if g.has_frame {
                                if let Some(ref rgba) = g.rgba {
                                    break (g.width, g.height, rgba.clone());
                                }
                            }
                        }
                        if Instant::now() > deadline {
                            return Err(
                                "timeout waiting for PipeWire frame (check portal permission)"
                                    .into(),
                            );
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    };

                    let (ew, eh) = cap_encode_size(native_w.max(2), native_h.max(2), 1920);
                    let bitrate_kbps = resolve_bitrate_kbps(bitrate_req, ew, eh, fps);

                    let first_scaled = scale_rgba(&first, native_w, native_h, ew, eh)?;
                    let mut rec = SyncRecorder::start(RecorderConfig {
                        output_path: video_path_c,
                        width: ew,
                        height: eh,
                        fps,
                        bitrate_kbps,
                        vcodec: "libx264".into(),
                        encoder_threads: 0,
                    })?;

                    // Align mouse clock to first encoded frame (not portal open).
                    clock_enc.set_fps(fps);
                    clock_enc.mark_video_start();

                    // CFR from continuous stream: write every 1/fps sec using latest buffer.
                    // Timeline is frame-index based — mouse stamps use the same PTS so
                    // encoder backlog cannot desync the track (was ~10–15 frames late).
                    let frame_dt = Duration::from_secs_f64(1.0 / fps as f64);
                    let mut next_due = Instant::now();
                    let mut frames_written: u64 = 0;
                    let mut last_seq = 0u64;
                    let mut unique: u64 = 1;
                    let mut current = first_scaled;
                    let t0_wall = Instant::now();

                    rec.write_frame(&Frame::new(ew, eh, current.clone()))?;
                    frames_written = 1;
                    clock_enc.note_frame_written(frames_written);
                    push_mouse_at_frame(&samples_enc, &live_enc, 0, fps);
                    next_due += frame_dt;

                    while !stop_e.load(Ordering::Relaxed) {
                        // Pull newest buffer if available.
                        if let Ok(g) = slot_enc.lock() {
                            if g.has_frame && g.seq != last_seq {
                                if let Some(ref rgba) = g.rgba {
                                    last_seq = g.seq;
                                    unique += 1;
                                    current = scale_rgba(rgba, g.width, g.height, ew, eh)
                                        .unwrap_or_else(|_| current.clone());
                                }
                            }
                        }

                        let now = Instant::now();
                        if now >= next_due {
                            // Catch up to wall schedule — write enough frames so media
                            // time keeps up with real time (was capped at 2 → chronic lag).
                            let target_frames = ((now.duration_since(t0_wall).as_secs_f64()
                                * fps as f64)
                                .floor() as u64)
                                .saturating_add(1)
                                .max(frames_written + 1);
                            // Bound burst so one stall doesn't freeze the UI thread long.
                            let burst_end = target_frames.min(frames_written + 8);
                            while frames_written < burst_end {
                                rec.write_frame(&Frame::new(ew, eh, current.clone()))?;
                                frames_written += 1;
                                clock_enc.note_frame_written(frames_written);
                                push_mouse_at_frame(
                                    &samples_enc,
                                    &live_enc,
                                    frames_written - 1,
                                    fps,
                                );
                                next_due += frame_dt;
                            }
                            // If still behind, don't sleep — loop immediately.
                            if frames_written < target_frames {
                                continue;
                            }
                        } else {
                            let sleep_for = (next_due - now).min(Duration::from_millis(2));
                            std::thread::sleep(sleep_for);
                        }
                    }

                    // Pad only to the last scheduled PTS (already encoder-aligned).
                    let media_dur = frames_written as f64 / fps.max(1) as f64;
                    clock_enc.set_video_duration(media_dur);
                    clock_enc.note_frame_written(frames_written);

                    rec.finish()?;
                    log::info!(
                        "[screen] pipewire {unique} buffers → {frames_written} frames / {media_dur:.1}s media @ {fps}fps ({bitrate_kbps} kbps, {ew}x{eh}; mouse screen {screen_w}x{screen_h})",
                    );
                    Ok(())
                })
                .map_err(|e| format!("encoder thread: {e}"))?;

            // Block on PipeWire until stop; keep `_session` alive until after.
            let pw_res = run_pipewire_capture(
                node_id,
                fd,
                stop_v.clone(),
                slot_pw,
                portal_cursor_pw,
                fps,
            );
            // Ensure stop so encoder exits even if pw failed early.
            stop_v.store(true, Ordering::SeqCst);
            let enc_res = enc_join
                .join()
                .map_err(|_| "encoder join panicked".to_string())?;
            drop(_session);

            match (pw_res, enc_res) {
                (Ok(()), Ok(())) => Ok(()),
                (Err(e), Ok(())) => Err(e),
                (Ok(()), Err(e)) => Err(e),
                (Err(e1), Err(e2)) => Err(format!("{e1}; encode: {e2}")),
            }
        })
        .map_err(|e| format!("video thread: {e}"))?;

    Ok(ScreenCaptureSession {
        layer_id: cfg.layer_id,
        sepscrr_path: cfg.sepscrr_path,
        video_path,
        width: ew,
        height: eh,
        fps,
        capture_cursor: cfg.capture_cursor,
        capture_audio: cfg.capture_audio,
        backend: CaptureBackend::WaylandPortalRust,
        started,
        stop,
        clock,
        samples,
        mouse_join: Some(mouse_join),
        video_join: Some(video_join),
        encoder: None,
        audio_cap,
    })
}

#[cfg(all(not(target_os = "linux"), not(target_os = "android")))]
fn start_wayland_portal_rust(_cfg: ScreenCaptureStart) -> Result<ScreenCaptureSession, String> {
    Err("ScreenCast PipeWire capture is only available on Linux".into())
}
