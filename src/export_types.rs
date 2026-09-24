//! Export/media configuration types (P0 refactor).
//!
//! These lived on `app.rs`, which made infrastructure modules depend on the
//! application layer (`history -> app`, `export_worker -> app`,
//! `export_audio -> app`). They are pure config enums with no dependency on
//! `VadadeeBerryApp`, so they move here. `app.rs` re-exports them for
//! backward compatibility while call sites migrate.

use serde::{Deserialize, Serialize};

/// Which backend to use for video encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VideoBackend {
    #[default]
    Ffmpeg,
    Gstreamer,
}

impl VideoBackend {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ffmpeg => "FFmpeg",
            Self::Gstreamer => "GStreamer",
        }
    }
}

/// CPU usage profile while encoding video (libav encoder thread count / preset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ExportPowerLevel {
    #[default]
    PowerSaving,
    FullPower,
}

impl ExportPowerLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::PowerSaving => "Power saving",
            Self::FullPower => "Full power",
        }
    }
}

/// P7f: Node Editor / FX bake quality for export (max side + blur quantization).
///
/// Long-side caps must stay in the HD range — older 128/256/512 values made
/// 1080p exports look like soft mush after upscale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ExportFxQuality {
    /// Fast draft: 720p-class bake.
    Draft,
    /// Default: full HD long side.
    #[default]
    Normal,
    /// Best: 1440p-class bake (still capped by page size in the worker).
    High,
}

impl ExportFxQuality {
    pub fn label(self) -> &'static str {
        match self {
            Self::Draft => "Draft (fast)",
            Self::Normal => "Normal",
            Self::High => "High",
        }
    }

    /// Longest side of NE FilePath bake (pixels).
    pub fn max_side(self) -> u32 {
        match self {
            Self::Draft => 720,
            Self::Normal => 1080,
            Self::High => 1440,
        }
    }

    /// Blur radius quantization step for export FX cache keys.
    pub fn blur_step(self) -> f32 {
        match self {
            Self::Draft => 2.0,
            Self::Normal => 1.0,
            Self::High => 0.5,
        }
    }
}

/// Container format for video export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VideoFormat {
    #[default]
    Mp4,
    Mkv,
    Webm,
    Mov,
}

impl VideoFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mp4  => "MP4 (H.264)",
            Self::Mkv  => "MKV (H.264)",
            Self::Webm => "WebM (VP9)",
            Self::Mov  => "MOV (ProRes)",
        }
    }
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp4  => "mp4",
            Self::Mkv  => "mkv",
            Self::Webm => "webm",
            Self::Mov  => "mov",
        }
    }
}

/// All render-to-video settings plus live progress state.
pub struct VideoExportState {
    pub backend: VideoBackend,
    pub fps: u32,
    pub resolution_pct: u32,  // 25, 50, 75, 100, 150, 200
    pub bitrate_kbps: u32,
    pub format: VideoFormat,
    /// 0.0 – 1.0 while rendering, None when idle.
    pub progress: Option<f32>,
    /// True while the progress dialog is shown, false when hidden.
    pub progress_visible: bool,
    /// True when a render is actually running.
    pub rendering: bool,
    /// Latest status message from the encoder.
    pub status_msg: String,
    pub frame_done: usize,
    pub total_frames: usize,
    /// 0 = auto from timeline/content; otherwise fixed export length in seconds.
    pub export_duration_secs: f32,
    /// How many times to repeat the animation loop in the export (1 = once).
    pub export_cycles: u32,
    pub restore_anim_frame: usize,
    pub frames_dir: Option<std::path::PathBuf>,
    pub output_path: Option<std::path::PathBuf>,
    pub power_level: ExportPowerLevel,
    /// P7f: NE Output bake quality (Draft / Normal / High).
    pub fx_quality: ExportFxQuality,
    pub export_start_time: Option<std::time::Instant>,
    pub(crate) export_rx: Option<std::sync::mpsc::Receiver<crate::export_worker::ExportWorkerEvent>>,
    pub(crate) export_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub sec_per_frame: f32,
    pub last_frame_time: Option<std::time::Instant>,
    /// P7a: bar target from worker (`frame_done / total`).
    pub progress_target: f32,
    /// P7a: displayed bar value (lerps toward `progress_target` each UI frame).
    pub progress_smooth: f32,
    /// P7a: latest worker frame count (may jump ahead of displayed progress).
    pub worker_frame_done: usize,
    pub renderer_reclaim: std::sync::Arc<std::sync::Mutex<Vec<egui_wgpu::Renderer>>>,
}

impl std::fmt::Debug for VideoExportState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoExportState")
            .field("backend", &self.backend)
            .field("fps", &self.fps)
            .field("resolution_pct", &self.resolution_pct)
            .field("bitrate_kbps", &self.bitrate_kbps)
            .field("format", &self.format)
            .field("progress", &self.progress)
            .field("progress_visible", &self.progress_visible)
            .field("rendering", &self.rendering)
            .field("status_msg", &self.status_msg)
            .field("frame_done", &self.frame_done)
            .field("total_frames", &self.total_frames)
            .field("export_duration_secs", &self.export_duration_secs)
            .field("restore_anim_frame", &self.restore_anim_frame)
            .field("frames_dir", &self.frames_dir)
            .field("output_path", &self.output_path)
            .field("power_level", &self.power_level)
            .field("fx_quality", &self.fx_quality)
            .field("export_start_time", &self.export_start_time)
            .finish()
    }
}

impl Default for VideoExportState {
    fn default() -> Self {
        Self {
            backend: VideoBackend::Ffmpeg,
            fps: 30,
            resolution_pct: 100,
            bitrate_kbps: 8000,
            format: VideoFormat::Mp4,
            progress: None,
            progress_visible: false,
            rendering: false,
            status_msg: String::new(),
            frame_done: 0,
            total_frames: 0,
            export_duration_secs: 0.0,
            export_cycles: 1,
            restore_anim_frame: 0,
            frames_dir: None,
            output_path: None,
            power_level: ExportPowerLevel::default(),
            fx_quality: ExportFxQuality::default(),
            export_start_time: None,
            export_rx: None,
            export_cancel: None,
            sec_per_frame: 0.0,
            last_frame_time: None,
            progress_target: 0.0,
            progress_smooth: 0.0,
            worker_frame_done: 0,
            renderer_reclaim: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}
