//! Background video export — keeps heavy libav + SVG raster work off the UI thread.
//!
//! Rasterizes frames on a worker thread and encodes with libav on the same thread.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rustc_hash::FxHashMap;

use crate::app::{ExportPowerLevel, VideoFormat};
use crate::document::{Fill, NodeId, ProjectFile};
use crate::io::{self, VideoFrameMap, VideoLayerBuffer};
use crate::recorder::{Frame, RecorderConfig, SyncRecorder};
use crate::video_decode::VideoStream;

use egui::Context;

/// How often progress events are emitted (frames).
const PROGRESS_EVERY_N_FRAMES: usize = 1;


#[derive(Debug, Clone)]
pub struct ExportJobConfig {
    pub output_path: PathBuf,
    pub work_dir: PathBuf,
    pub fps: u32,
    pub resolution_pct: u32,
    pub bitrate_kbps: u32,
    pub format: VideoFormat,
    pub power: ExportPowerLevel,
    pub total_frames: usize,
    pub anim_fps: u32,
    pub max_anim_frame: usize,
    /// Frames in one animation cycle (for looping export).
    pub cycle_frame_count: usize,
    /// How many times the cycle is repeated in `total_frames`.
    pub export_cycles: u32,
}

/// Readiness phases — worker only advances when the previous step is complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportPhase {
    Preparing,
    Encoding,
    Finalizing,
}

#[derive(Debug, Clone)]
pub enum ExportWorkerEvent {
    Phase(ExportPhase),
    Progress {
        phase: ExportPhase,
        frame_done: usize,
        total: usize,
        message: String,
    },
    Finished {
        success: bool,
        message: String,
    },
}

pub fn spawn_export_worker(
    mut project: ProjectFile,
    config: ExportJobConfig,
    cancel: Arc<AtomicBool>,
    tx: Sender<ExportWorkerEvent>,
    wgpu_render: Option<egui_wgpu::RenderState>,
    renderer_reclaim: Arc<Mutex<Vec<egui_wgpu::Renderer>>>,
) {
    let tx_fail = tx.clone();
    match std::thread::Builder::new()
        .name("vadadee-video-export".into())
        .spawn(move || {
            if let Err(e) = run_export(&mut project, &config, &cancel, &tx, wgpu_render, renderer_reclaim) {
                let _ = tx.send(ExportWorkerEvent::Finished {
                    success: false,
                    message: e,
                });
            }
        }) {
        Ok(_) => {}
        Err(e) => {
            let _ = tx_fail.send(ExportWorkerEvent::Finished {
                success: false,
                message: format!("Could not spawn export thread: {e}"),
            });
        }
    }
}

fn export_cancelled(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed)
}

#[derive(Clone)]
struct ColorAdjust {
    hue: f32,
    saturation: f32,
    brightness: f32,
    contrast: f32,
}

impl ColorAdjust {
    fn active(&self) -> bool {
        self.hue != 0.0
            || self.saturation != 1.0
            || self.brightness != 1.0
            || self.contrast != 1.0
    }
}

struct ExportVideoLayer {
    /// The clip's own UUID (used as key in video_frames).
    id: uuid::Uuid,
    /// The layer this clip belongs to (used as composite key in VideoFrameMap).
    layer_id: uuid::Uuid,
    path: String,
    timeline_start: f32,
    start_offset: f32,
    play_secs: f32,
    color: ColorAdjust,
}

struct ExportSession<'a> {
    project: &'a mut ProjectFile,
    config: &'a ExportJobConfig,
    cancel: &'a AtomicBool,
    tx: &'a Sender<ExportWorkerEvent>,
    phase: ExportPhase,
    started_at: Instant,
    export_fps: f32,
    anim_fps: f32,
    scale: f32,
    vcodec: &'static str,
    temp_video: PathBuf,
    video_layers: Vec<ExportVideoLayer>,
    video_streams: FxHashMap<String, VideoStream>,
    video_frames: VideoFrameMap,
    recorder: Option<SyncRecorder>,
    export_ctx: egui::Context,
    wgpu_render: Option<egui_wgpu::RenderState>,
    offscreen_renderer: Option<egui_wgpu::Renderer>,
    /// Renderers are sent back to the main thread for safe GPU teardown.
    renderer_reclaim: Arc<Mutex<Vec<egui_wgpu::Renderer>>>,
    /// Cross-frame caches (export was re-decoding every frame → ~0.4 fps).
    base_image_cache: std::collections::HashMap<String, image::RgbaImage>,
    fx_image_cache: std::collections::HashMap<String, image::RgbaImage>,
    /// Stable egui textures for Image nodes + NE Output (keyed by NodeId / layer id).
    image_textures: std::collections::HashMap<NodeId, egui::TextureHandle>,
    /// Last FX key uploaded for NE layer id (skip re-upload when unchanged).
    ne_tex_fx_key: std::collections::HashMap<uuid::Uuid, String>,
    fonts: crate::fonts::FontRegistry,
}

impl<'a> ExportSession<'a> {
    fn new(
        project: &'a mut ProjectFile,
        config: &'a ExportJobConfig,
        cancel: &'a AtomicBool,
        tx: &'a Sender<ExportWorkerEvent>,
        wgpu_render: Option<egui_wgpu::RenderState>,
        renderer_reclaim: Arc<Mutex<Vec<egui_wgpu::Renderer>>>,
    ) -> Self {
        let scale = (config.resolution_pct as f32 / 100.0).max(0.1);
        let vcodec = match config.format {
            VideoFormat::Webm => "libvpx-vp9",
            VideoFormat::Mov => "prores_ks",
            _ => "libx264",
        };
        let video_layers = collect_export_video_layers(project);
        let export_ctx = egui::Context::default();
        let offscreen_renderer = wgpu_render.as_ref().map(|rs| {
            let mut r = egui_wgpu::Renderer::new(
                &rs.device,
                wgpu::TextureFormat::Rgba8Unorm,
                egui_wgpu::RendererOptions::default(),
            );
            let shading_res = crate::shading::wgpu_pass::ShadingGpuResources::new(
                rs.device.clone(),
                wgpu::TextureFormat::Rgba8Unorm,
                1,
            );
            r.callback_resources.insert(shading_res);
            r
        });
        Self {
            project,
            config,
            cancel,
            tx,
            phase: ExportPhase::Preparing,
            started_at: Instant::now(),
            export_fps: config.fps as f32,
            anim_fps: config.anim_fps.max(1) as f32,
            scale,
            vcodec,
            temp_video: config.work_dir.join("temp_encoded_video.mp4"),
            video_layers,
            video_streams: FxHashMap::default(),
            video_frames: VideoFrameMap::default(),
            recorder: None,
            export_ctx,
            wgpu_render,
            offscreen_renderer,
            renderer_reclaim,
            base_image_cache: std::collections::HashMap::new(),
            fx_image_cache: std::collections::HashMap::new(),
            image_textures: std::collections::HashMap::new(),
            ne_tex_fx_key: std::collections::HashMap::new(),
            fonts: crate::fonts::FontRegistry::new(),
        }
    }

    /// Max side for NE FilePath bake — keep small; scaled onto page by transform.
    fn ne_bake_max_side(&self, pixel_w: u32, pixel_h: u32) -> u32 {
        let m = pixel_w.max(pixel_h).max(256);
        // 512: export quality is fine when upscaled onto A4; big win vs 1024/2048 blurs.
        m.min(512)
    }

    /// GPU only when WGSL shading is active. Vector Image layers use resvg CPU (below).
    /// Previous can_fast required empty Image layers — any shape forced the 8–15s GPU path.
    fn needs_gpu_export(&self) -> bool {
        self.project.document.layers.iter().any(|l| {
            l.visible
                && l.is_renderer
                && l.kind == crate::document::LayerKind::Shading
                && l.shading_passes.iter().any(|p| p.enabled)
        })
    }

    /// Prefer CPU whenever possible (NE FilePath + optional empty vectors + AV).
    fn can_fast_cpu_export(&self) -> bool {
        if self.needs_gpu_export() {
            return false;
        }
        // AppObjects still need vector compositing → fall back to GPU/egui for now.
        for l in &self.project.document.layers {
            if !l.visible || !l.is_renderer {
                continue;
            }
            if l.kind == crate::document::LayerKind::NodeEditor {
                if let Some(g) = &l.node_graph {
                    if matches!(
                        g.resolve_output_image().image,
                        crate::document::GraphImageSource::AppObjects(_)
                    ) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Fast CPU composite: page + AV frames + NE FilePath (cached bake). No egui/GPU.
    fn rasterize_frame_fast_cpu(
        &mut self,
        current_frame: usize,
        time_secs: f32,
    ) -> Option<(u32, u32, Vec<u8>)> {
        use resvg::tiny_skia::{Color, Pixmap, PixmapPaint, Transform};

        let doc_w = self.project.document.width;
        let doc_h = self.project.document.height;
        let mut pixel_w = (doc_w as f32 * self.scale).round() as u32;
        let mut pixel_h = (doc_h as f32 * self.scale).round() as u32;
        if pixel_w % 2 != 0 {
            pixel_w = pixel_w.saturating_sub(1);
        }
        if pixel_h % 2 != 0 {
            pixel_h = pixel_h.saturating_sub(1);
        }
        if pixel_w == 0 || pixel_h == 0 {
            return None;
        }

        let mut pixmap = Pixmap::new(pixel_w, pixel_h)?;
        let pc = self.project.document.page_color;
        let bg = Color::from_rgba(
            pc[0].clamp(0.0, 1.0),
            pc[1].clamp(0.0, 1.0),
            pc[2].clamp(0.0, 1.0),
            pc[3].clamp(0.0, 1.0),
        )
        .unwrap_or(Color::WHITE);
        pixmap.fill(bg);

        let max_side = self.ne_bake_max_side(pixel_w, pixel_h);
        let scale = self.scale;
        let scale_x = pixel_w as f32 / doc_w as f32;
        let scale_y = pixel_h as f32 / doc_h as f32;
        let svg_scale = Transform::from_scale(scale_x, scale_y);
        let usvg_opt = crate::fonts::usvg_options();

        for layer in &self.project.document.layers {
            if !layer.visible || !layer.is_renderer {
                continue;
            }
            match layer.kind {
                crate::document::LayerKind::Image | crate::document::LayerKind::Flowchart => {
                    if layer.nodes.is_empty() {
                        continue;
                    }
                    // CPU vector raster (avoids egui GPU path). Empty layers are free.
                    let svg = io::document_svg_single_image_layer(
                        self.project,
                        layer,
                        &std::collections::HashSet::new(),
                    );
                    if let Ok(tree) = usvg::Tree::from_str(&svg, &usvg_opt) {
                        resvg::render(&tree, svg_scale, &mut pixmap.as_mut());
                    }
                }
                crate::document::LayerKind::AV => {
                    let Some(buf) = self.video_frames.get(&layer.id) else {
                        continue;
                    };
                    let mut src = Pixmap::new(buf.width, buf.height)?;
                    src.data_mut().copy_from_slice(&buf.rgba);
                    let (dx, dy, rot, opacity) =
                        io::layer_anim_transform(layer, self.project, current_frame);
                    let (dw, dh) =
                        io::video_layer_dest_size(layer, buf.width, buf.height);
                    let x = (dx as f32) * scale;
                    let y = (dy as f32) * scale;
                    let w = dw * scale;
                    let h = dh * scale;
                    let sx = w / buf.width as f32;
                    let sy = h / buf.height as f32;
                    let transform = if rot != 0.0 {
                        Transform::from_translate(x, y).pre_concat(
                            Transform::from_translate(w / 2.0, h / 2.0)
                                .pre_rotate(rot as f32)
                                .pre_translate(-w / 2.0, -h / 2.0)
                                .pre_scale(sx, sy),
                        )
                    } else {
                        Transform::from_translate(x, y).pre_scale(sx, sy)
                    };
                    let mut paint = PixmapPaint::default();
                    paint.opacity = opacity;
                    pixmap.draw_pixmap(0, 0, src.as_ref(), &paint, transform, None);
                }
                crate::document::LayerKind::NodeEditor => {
                    let Some(g) = &layer.node_graph else {
                        continue;
                    };
                    let eval = g.resolve_output_image();
                    let crate::document::GraphImageSource::FilePath(path) = &eval.image else {
                        continue;
                    };
                    let rgba = crate::document::bake_graph_output_rgba(
                        path,
                        &eval,
                        max_side,
                        Some(&mut self.base_image_cache),
                        Some(&mut self.fx_image_cache),
                    )?;
                    let (tw, th) = rgba.dimensions();
                    let mut src = Pixmap::new(tw, th)?;
                    src.data_mut().copy_from_slice(&rgba);
                    let (dx, dy, mut w, mut h, rot_rad) =
                        layer.ne_output_paint_geom(&self.project.nodes, &eval);
                    let def_w = layer.width as f64;
                    let def_h = layer.height as f64;
                    let near_default =
                        (w - def_w).abs() < 2.0 && (h - def_h).abs() < 2.0;
                    let near_a4 = (w - crate::document::A4_WIDTH_PX).abs() < 2.0
                        && (h - crate::document::A4_HEIGHT_PX).abs() < 2.0;
                    if near_default || near_a4 {
                        let page_w = doc_w.max(1.0);
                        let page_h = doc_h.max(1.0);
                        let mut nw = tw as f64;
                        let mut nh = th as f64;
                        if nw > page_w || nh > page_h {
                            let s = (page_w / nw).min(page_h / nh);
                            nw *= s;
                            nh *= s;
                        }
                        w = nw.max(1.0);
                        h = nh.max(1.0);
                    }
                    let x = (dx as f32) * scale;
                    let y = (dy as f32) * scale;
                    let dw = (w as f32) * scale;
                    let dh = (h as f32) * scale;
                    let sx = dw / tw as f32;
                    let sy = dh / th as f32;
                    let rot_deg = rot_rad.to_degrees() as f32;
                    let transform = if rot_deg.abs() > 1e-4 {
                        Transform::from_translate(x, y).pre_concat(
                            Transform::from_translate(dw / 2.0, dh / 2.0)
                                .pre_rotate(rot_deg)
                                .pre_translate(-dw / 2.0, -dh / 2.0)
                                .pre_scale(sx, sy),
                        )
                    } else {
                        Transform::from_translate(x, y).pre_scale(sx, sy)
                    };
                    let paint = PixmapPaint::default();
                    pixmap.draw_pixmap(0, 0, src.as_ref(), &paint, transform, None);
                    let _ = time_secs;
                }
                _ => {}
            }
        }

        Some((pixel_w, pixel_h, pixmap.take()))
    }

    /// Ensure embedded Image node textures exist (decode once per export).
    fn ensure_node_image_textures(&mut self) {
        let pending: Vec<(NodeId, Vec<u8>)> = self
            .project
            .nodes
            .map
            .iter()
            .filter_map(|(&id, node)| {
                if self.image_textures.contains_key(&id) {
                    return None;
                }
                if let crate::document::NodeKind::Image { bytes, .. } = &node.kind {
                    if bytes.is_empty() {
                        return None;
                    }
                    Some((id, bytes.clone()))
                } else {
                    None
                }
            })
            .collect();
        for (id, bytes) in pending {
            if let Ok(dyn_img) = image::load_from_memory(&bytes) {
                let rgba = dyn_img.to_rgba8();
                let (w, h) = rgba.dimensions();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                    [w as usize, h as usize],
                    &rgba.into_raw(),
                );
                let handle = self.export_ctx.load_texture(
                    format!("export-node-img-{id}"),
                    color_image,
                    egui::TextureOptions::LINEAR,
                );
                self.image_textures.insert(id, handle);
            }
        }
    }

    /// Bake/upload NE FilePath FX texture for a layer; reuses when FX key unchanged.
    fn ensure_ne_fx_texture(
        &mut self,
        layer_id: uuid::Uuid,
        path: &str,
        eval: &crate::document::GraphOutputEval,
        max_side: u32,
    ) -> Option<egui::TextureHandle> {
        let q = eval.quantized_for_cache(true);
        let fx_key = format!("{}|ms{max_side}", q.fx_cache_key(path));
        if self.ne_tex_fx_key.get(&layer_id) == Some(&fx_key) {
            // Texture is stored under synthetic node id = layer_id for NE Output.
            if let Some(t) = self.image_textures.get(&layer_id) {
                return Some(t.clone());
            }
        }
        let rgba = crate::document::bake_graph_output_rgba(
            path,
            eval,
            max_side,
            Some(&mut self.base_image_cache),
            Some(&mut self.fx_image_cache),
        )?;
        let (tw, th) = rgba.dimensions();
        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            [tw as usize, th as usize],
            &rgba.into_raw(),
        );
        let handle = self.export_ctx.load_texture(
            format!("export-ne-fx-{layer_id}"),
            color_image,
            egui::TextureOptions::LINEAR,
        );
        self.ne_tex_fx_key.insert(layer_id, fx_key);
        self.image_textures.insert(layer_id, handle.clone());
        Some(handle)
    }

    fn set_phase(&mut self, phase: ExportPhase) -> Result<(), String> {
        if self.phase == phase {
            return Ok(());
        }
        self.phase = phase;
        let _ = self.tx.send(ExportWorkerEvent::Phase(phase));
        Ok(())
    }

    fn check_cancel(&self) -> Result<(), String> {
        if export_cancelled(self.cancel) {
            Err("Cancelled.".into())
        } else {
            Ok(())
        }
    }

    fn prepare(&mut self) -> Result<(), String> {
        self.set_phase(ExportPhase::Preparing)?;
        if !crate::video_decode::is_libav_available() {
            return Err(
                "FFmpeg libraries not found (libavformat/libavcodec). Install FFmpeg shared libs."
                    .into(),
            );
        }
        self.check_cancel()
    }

    fn ensure_encoder(&mut self, width: u32, height: u32) -> Result<(), String> {
        if self.recorder.is_some() {
            return Ok(());
        }
        self.check_cancel()?;
        let threads = match self.config.power {
            ExportPowerLevel::PowerSaving => 1,
            ExportPowerLevel::FullPower => 0,
        };
        let output = self
            .temp_video
            .to_str()
            .ok_or_else(|| format!("Invalid temp video path: {}", self.temp_video.display()))?;
        let recorder = SyncRecorder::start(RecorderConfig {
            output_path: PathBuf::from(output),
            width,
            height,
            fps: self.config.fps,
            bitrate_kbps: self.config.bitrate_kbps,
            vcodec: self.vcodec.to_string(),
            encoder_threads: threads,
        })?;
        self.recorder = Some(recorder);
        self.set_phase(ExportPhase::Encoding)
    }

    fn decode_video_layers(&mut self, timeline_sec: f32) -> Result<(), String> {
        self.video_frames.clear();
        for layer in &self.video_layers {
            self.check_cancel()?;
            // Skip if this clip isn't active at the current timeline position.
            let timeline_end = layer.timeline_start + layer.play_secs;
            if timeline_sec < layer.timeline_start || timeline_sec >= timeline_end {
                continue;
            }
            let elapsed_time = (timeline_sec - layer.timeline_start)
                .max(0.0)
                .min(layer.play_secs);
            let source_time = layer.start_offset + elapsed_time;
            let source_frame_idx = (source_time * self.export_fps) as usize;
            if let Some((w, h, mut rgba)) = decode_layer_frame_rgba(
                &mut self.video_streams,
                &layer.path,
                source_frame_idx,
                self.export_fps,
            ) {
                if layer.color.active() {
                    if let Some(mut img) = image::RgbaImage::from_raw(w, h, std::mem::take(&mut rgba))
                    {
                        apply_color_controls(
                            &mut img,
                            layer.color.hue,
                            layer.color.saturation,
                            layer.color.brightness,
                            layer.color.contrast,
                        );
                        rgba = img.into_raw();
                    }
                }
                // Key by layer_id so composite_export_frame can find the frame.
                self.video_frames.insert(
                    layer.layer_id,
                    VideoLayerBuffer {
                        width: w,
                        height: h,
                        rgba,
                    },
                );
            }
        }
        Ok(())
    }

    fn stream_frame(&mut self, width: u32, height: u32, rgba: Vec<u8>) -> Result<(), String> {
        let recorder = self
            .recorder
            .as_mut()
            .ok_or_else(|| "Export encoder not ready".to_string())?;
        recorder.write_frame(&Frame::new(width, height, rgba))
    }

    fn emit_progress(&self, frame_done: usize) {
        if frame_done % PROGRESS_EVERY_N_FRAMES != 0 && frame_done != self.config.total_frames {
            return;
        }
        let elapsed = self.started_at.elapsed().as_secs_f32();
        let eta_str = if frame_done > 1 {
            let avg = elapsed / (frame_done - 1) as f32;
            let rem = (self.config.total_frames - frame_done) as f32 * avg;
            if rem < 60.0 {
                format!("{:.0}s", rem)
            } else {
                format!("{}:{:02}", (rem / 60.0) as i32, (rem % 60.0) as i32)
            }
        } else {
            "estimating…".to_string()
        };
        let _ = self.tx.send(ExportWorkerEvent::Progress {
            phase: self.phase,
            frame_done,
            total: self.config.total_frames,
            message: format!(
                "Encoding frame {}/{} ({} fps) | ETA: {}",
                frame_done, self.config.total_frames, self.config.fps, eta_str
            ),
        });
    }

    fn encode_all_frames(&mut self) -> Result<(), String> {
        let use_cpu = self.can_fast_cpu_export();
        let path_label = if use_cpu { "cpu" } else { "gpu" };
        let _ = self.tx.send(ExportWorkerEvent::Progress {
            phase: ExportPhase::Encoding,
            frame_done: 0,
            total: self.config.total_frames,
            message: format!(
                "Export path={path_label} scale={:.0}% frames={}",
                self.scale * 100.0,
                self.config.total_frames
            ),
        });

        for f in 0..self.config.total_frames {
            self.check_cancel()?;
            let t_frame = Instant::now();

            // Map export frame into one animation cycle (supports multi-cycle loop exports).
            let cycle_len = self.config.cycle_frame_count.max(1);
            let f_in_cycle = f % cycle_len;
            let timeline_sec = f_in_cycle as f32 / self.export_fps;
            let anim_frame = ((timeline_sec * self.anim_fps).round() as usize)
                .min(self.config.max_anim_frame);
            apply_animation_for_frame_project(self.project, anim_frame);
            let t_anim = t_frame.elapsed();

            self.decode_video_layers(timeline_sec)?;
            let t_dec = t_frame.elapsed();

            // Prefer CPU path — GPU/egui was 8–15s/frame for NE FilePath.
            let (w, h, rgba) = if use_cpu {
                self.rasterize_frame_fast_cpu(anim_frame, timeline_sec)
                    .ok_or_else(|| "Frame rasterize failed".to_string())?
            } else if let Some(render_state) = self.wgpu_render.clone() {
                let doc_w = self.project.document.width;
                let doc_h = self.project.document.height;
                let mut pixel_w = (doc_w as f32 * self.scale).round() as u32;
                let mut pixel_h = (doc_h as f32 * self.scale).round() as u32;
                if pixel_w % 2 != 0 {
                    pixel_w = pixel_w.saturating_sub(1);
                }
                if pixel_h % 2 != 0 {
                    pixel_h = pixel_h.saturating_sub(1);
                }
                if pixel_w == 0 || pixel_h == 0 {
                    return Err("Zero width or height for export".to_string());
                }

                if let Some(buf) = self.rasterize_frame_offscreen_gpu(
                    &render_state,
                    anim_frame,
                    timeline_sec,
                    pixel_w,
                    pixel_h,
                ) {
                    (pixel_w, pixel_h, buf)
                } else {
                    io::composite_export_frame(
                        self.project,
                        anim_frame,
                        &self.video_frames,
                        self.scale,
                        timeline_sec,
                    )
                    .ok_or_else(|| "Frame rasterize failed".to_string())?
                }
            } else {
                io::composite_export_frame(
                    self.project,
                    anim_frame,
                    &self.video_frames,
                    self.scale,
                    timeline_sec,
                )
                .ok_or_else(|| "Frame rasterize failed".to_string())?
            };
            let t_rast = t_frame.elapsed();

            self.ensure_encoder(w, h)?;
            self.stream_frame(w, h, rgba)?;
            let t_all = t_frame.elapsed();

            // First frames: surface where time goes (ms).
            if f < 3 || f % 30 == 0 {
                let _ = self.tx.send(ExportWorkerEvent::Progress {
                    phase: ExportPhase::Encoding,
                    frame_done: f + 1,
                    total: self.config.total_frames,
                    message: format!(
                        "f{} {path_label} {}x{} anim={:.0}ms dec={:.0}ms rast={:.0}ms enc={:.0}ms tot={:.0}ms",
                        f + 1,
                        w,
                        h,
                        t_anim.as_secs_f32() * 1000.0,
                        (t_dec - t_anim).as_secs_f32() * 1000.0,
                        (t_rast - t_dec).as_secs_f32() * 1000.0,
                        (t_all - t_rast).as_secs_f32() * 1000.0,
                        t_all.as_secs_f32() * 1000.0,
                    ),
                });
            } else {
                self.emit_progress(f + 1);
            }
        }
        Ok(())
    }

    fn finalize(&mut self) -> Result<bool, String> {
        self.set_phase(ExportPhase::Finalizing)?;
        self.check_cancel()?;
        let _ = self.tx.send(ExportWorkerEvent::Progress {
            phase: ExportPhase::Finalizing,
            frame_done: self.config.total_frames,
            total: self.config.total_frames,
            message: "Muxing audio…".into(),
        });
        if let Some(recorder) = self.recorder.take() {
            recorder.finish()?;
        }

        let duration_secs =
            self.config.total_frames as f32 / self.config.fps.max(1) as f32;
        // Evaluate mux result before cleanup so work_dir is always removed.
        let mux_result = if self.temp_video.exists() {
            crate::export_audio::export_mux_with_audio(
                self.project,
                &self.temp_video,
                &self.config.output_path,
                &self.config.work_dir,
                duration_secs,
                self.config.format,
            )
        } else {
            Ok(false)
        };
        let _ = std::fs::remove_dir_all(&self.config.work_dir); // always clean up
        Ok(mux_result?)
    }

    fn abort(&mut self) {
        self.recorder.take();
        let _ = std::fs::remove_dir_all(&self.config.work_dir);
        let _ = self.tx.send(ExportWorkerEvent::Finished {
            success: false,
            message: "Cancelled.".into(),
        });
    }
}

fn run_export(
    project: &mut ProjectFile,
    config: &ExportJobConfig,
    cancel: &AtomicBool,
    tx: &Sender<ExportWorkerEvent>,
    wgpu_render: Option<egui_wgpu::RenderState>,
    renderer_reclaim: Arc<Mutex<Vec<egui_wgpu::Renderer>>>,
) -> Result<(), String> {
    let mut session = ExportSession::new(project, config, cancel, tx, wgpu_render, renderer_reclaim);
    if let Err(e) = session.prepare().and_then(|_| session.encode_all_frames()) {
        // Reclaim the offscreen renderer BEFORE the session is dropped, so GPU
        // pipeline teardown happens on the main thread rather than here.
        if let Some(r) = session.offscreen_renderer.take() {
            if let Ok(mut q) = session.renderer_reclaim.lock() {
                q.push(r);
            }
        }
        if e == "Cancelled." {
            session.abort();
            return Ok(());
        }
        return Err(e);
    }

    // Reclaim before finalize so the Renderer is already gone when we exit.
    if let Some(r) = session.offscreen_renderer.take() {
        if let Ok(mut q) = session.renderer_reclaim.lock() {
            q.push(r);
        }
    }

    let success = session.finalize()?;
    let message = if success {
        format!("Saved {}", config.output_path.display())
    } else {
        "Export failed while writing output file.".into()
    };
    let _ = tx.send(ExportWorkerEvent::Finished { success, message });
    Ok(())
}

fn collect_export_video_layers(project: &ProjectFile) -> Vec<ExportVideoLayer> {
    let mut out = Vec::new();
    for layer in &project.document.layers {
        if !layer.visible
            || !layer.is_renderer
            || layer.kind != crate::document::LayerKind::AV
        {
            continue;
        }
        let mut layer_clone = layer.clone();
        // Keep primary clip in-point / play length aligned with Active Track Details.
        layer_clone.prepare_av_for_export();
        let color = ColorAdjust {
            hue: layer_clone.hue,
            saturation: layer_clone.saturation,
            brightness: layer_clone.brightness,
            contrast: layer_clone.contrast,
        };
        if !layer_clone.av_clips.is_empty() {
            let mut clips: Vec<_> = layer_clone.av_clips.iter().collect();
            clips.sort_by(|a, b| {
                a.track_row.cmp(&b.track_row).then(
                    a.video_timeline_start
                        .partial_cmp(&b.video_timeline_start)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
            });
            for clip in clips {
                if clip.media_path.is_empty() || clip.is_audio_only() {
                    continue;
                }
                let play_secs = clip.timeline_play_secs();
                if play_secs <= 0.0 {
                    continue;
                }
                out.push(ExportVideoLayer {
                    id: clip.id,
                    layer_id: layer_clone.id,
                    path: clip.media_path.clone(),
                    timeline_start: clip.video_timeline_start.max(0.0),
                    start_offset: clip.video_start_offset.max(0.0),
                    play_secs,
                    color: color.clone(),
                });
            }
        } else if !layer_clone.video_path.is_empty() {
            let play_secs = layer_clone.timeline_play_secs();
            if play_secs > 0.0 {
                out.push(ExportVideoLayer {
                    id: layer_clone.id,
                    layer_id: layer_clone.id,
                    path: layer_clone.video_path.clone(),
                    timeline_start: layer_clone.video_timeline_start.max(0.0),
                    start_offset: layer_clone.video_start_offset.max(0.0),
                    play_secs,
                    color,
                });
            }
        }
    }
    out
}

fn decode_layer_frame_rgba(
    streams: &mut FxHashMap<String, VideoStream>,
    video_path: &str,
    frame_idx: usize,
    fps: f32,
) -> Option<(u32, u32, Vec<u8>)> {
    if !streams.contains_key(video_path) {
        if let Some(s) = VideoStream::open(video_path) {
            streams.insert(video_path.to_string(), s);
        }
    }
    if let Some(stream) = streams.get_mut(video_path) {
        stream.get_frame(frame_idx, fps)
    } else {
        crate::video_decode::decode_frame(video_path, frame_idx, fps)
    }
}

fn apply_color_controls(img: &mut image::RgbaImage, hue: f32, sat: f32, bright: f32, contrast: f32) {
    for pixel in img.pixels_mut() {
        let [r, g, b, _a] = pixel.0;
        let mut rf = r as f32 / 255.0;
        let mut gf = g as f32 / 255.0;
        let mut bf = b as f32 / 255.0;
        if contrast != 1.0 {
            rf = (rf - 0.5) * contrast + 0.5;
            gf = (gf - 0.5) * contrast + 0.5;
            bf = (bf - 0.5) * contrast + 0.5;
        }
        if bright != 1.0 {
            rf *= bright;
            gf *= bright;
            bf *= bright;
        }
        if sat != 1.0 {
            let lum = 0.2126 * rf + 0.7152 * gf + 0.0722 * bf;
            rf = lum + (rf - lum) * sat;
            gf = lum + (gf - lum) * sat;
            bf = lum + (bf - lum) * sat;
        }
        if hue != 0.0 {
            let (h0, s0, l0) = rgb_to_hsl(rf, gf, bf);
            let (nr, ng, nb) = hsl_to_rgb((h0 + hue).rem_euclid(360.0), s0, l0);
            rf = nr;
            gf = ng;
            bf = nb;
        }
        pixel.0 = [
            (rf.clamp(0.0, 1.0) * 255.0).round() as u8,
            (gf.clamp(0.0, 1.0) * 255.0).round() as u8,
            (bf.clamp(0.0, 1.0) * 255.0).round() as u8,
            _a,
        ];
    }
}

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if (max - r).abs() < 1e-6 {
        (g - b) / d + (if g < b { 6.0 } else { 0.0 })
    } else if (max - g).abs() < 1e-6 {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s.abs() < 1e-6 {
        return (l, l, l);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hk = h / 360.0;
    let t = |n: f32| {
        let k = (n + hk) % 1.0;
        if k < 1.0 / 3.0 {
            p + (q - p) * 3.0 * k
        } else if k < 2.0 / 3.0 {
            p
        } else {
            p + (q - p) * (3.0 - 3.0 * k)
        }
    };
    (t(0.0), t(-1.0 / 3.0), t(-2.0 / 3.0))
}

/// Apply timeline at `frame` to a detached project clone (export thread).
///
/// Must match live preview: **stack animation functions** win inside their span
/// (via [`crate::document::NodeAnimation::sample_mut`]), then keyframe interpolation.
/// Using keyframe-only sampling would export linear start→end ramps instead of f(t).
pub fn apply_animation_for_frame_project(project: &mut ProjectFile, frame: usize) {
    let node_ids: Vec<NodeId> = project.anim_timeline.nodes.keys().copied().collect();
    let mut updates: Vec<(
        NodeId,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f32>,
        Option<[f32; 4]>,
        Option<f32>,
        Option<[f32; 4]>,
        Option<Vec<f64>>,
    )> = Vec::with_capacity(node_ids.len());

    for node_id in node_ids {
        // Snapshot geom before mutably borrowing the timeline entry.
        let current_geom = get_node_geom_floats_project(project, node_id);
        let Some(track) = project.anim_timeline.nodes.get_mut(&node_id) else {
            continue;
        };
        // sample_mut: stack formulas (f(t)) override keyframes inside the span — same as UI.
        let x = track.sample_mut("pos_x", frame);
        let y = track.sample_mut("pos_y", frame);
        let rot = track.sample_mut("rotation", frame);
        let opacity = track.sample_mut("opacity", frame).map(|o| o as f32);
        let r = track.sample_mut("color_r", frame);
        let g = track.sample_mut("color_g", frame);
        let b = track.sample_mut("color_b", frame);
        let a = track.sample_mut("color_a", frame);
        let color = if let (Some(r), Some(g), Some(b), Some(a)) = (r, g, b, a) {
            Some([r as f32, g as f32, b as f32, a as f32])
        } else {
            None
        };
        let stroke_w = track.sample_mut("stroke_width", frame).map(|w| w as f32);
        let sr = track.sample_mut("stroke_r", frame);
        let sg = track.sample_mut("stroke_g", frame);
        let sb = track.sample_mut("stroke_b", frame);
        let sa = track.sample_mut("stroke_a", frame);
        let stroke_color = if let (Some(r), Some(g), Some(b), Some(a)) = (sr, sg, sb, sa) {
            Some([r as f32, g as f32, b as f32, a as f32])
        } else {
            None
        };

        let need_geom = track.geom_tracks.iter().any(|t| !t.keyframes.is_empty())
            || track.stack_functions.iter().any(|sf| {
                sf.channels.iter().any(|c| c.track.starts_with("geom_"))
            });
        let geom = if need_geom {
            // Grow geom track slots if stack targets higher indices.
            for ch in &track.stack_functions {
                for c in &ch.channels {
                    if let Some(idx) = c
                        .track
                        .strip_prefix("geom_")
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        if track.geom_tracks.len() <= idx {
                            track
                                .geom_tracks
                                .resize_with(idx + 1, Default::default);
                        }
                    }
                }
            }
            let n = track.geom_tracks.len().max(current_geom.len());
            let mut g_vals = Vec::with_capacity(n);
            for idx in 0..n {
                let def_val = current_geom.get(idx).copied().unwrap_or(0.0);
                let lbl = format!("geom_{idx}");
                g_vals.push(track.sample_mut(&lbl, frame).unwrap_or(def_val));
            }
            Some(g_vals)
        } else {
            None
        };

        updates.push((
            node_id,
            x,
            y,
            rot,
            opacity,
            color,
            stroke_w,
            stroke_color,
            geom,
        ));
    }

    for (
        node_id,
        target_x,
        target_y,
        target_rot,
        target_op,
        target_color,
        target_stroke_w,
        target_stroke_col,
        target_geom,
    ) in updates
    {
        if let Some(node) = project.nodes.get_mut(node_id) {
            let (curr_x, curr_y) = node.get_pos();
            let dx = target_x.map(|tx| tx - curr_x).unwrap_or(0.0);
            let dy = target_y.map(|ty| ty - curr_y).unwrap_or(0.0);
            if dx.abs() > 1e-9 || dy.abs() > 1e-9 {
                node.translate(dx, dy);
            }
            if let Some(rot) = target_rot {
                node.set_rotation(rot);
            }
            if let Some(op) = target_op {
                node.set_opacity(op);
            }
            if let Some(color) = target_color {
                let mut base_fill = project
                    .anim_timeline
                    .nodes
                    .get(&node_id)
                    .and_then(|track| track.base_fill.clone());
                if base_fill.is_none() {
                    base_fill = Some(node.style.fill.clone());
                    if let Some(track) = project.anim_timeline.nodes.get_mut(&node_id) {
                        track.base_fill = base_fill.clone();
                    }
                }
                if let Some(mut bf) = base_fill {
                    match &mut bf {
                        Fill::Solid(paint) => {
                            paint.rgba = color;
                            node.style.fill = Fill::Solid(*paint);
                        }
                        Fill::LinearGradient { stops, .. } | Fill::RadialGradient { stops, .. } => {
                            for stop in stops {
                                stop.color.rgba = [
                                    stop.color.rgba[0] * color[0],
                                    stop.color.rgba[1] * color[1],
                                    stop.color.rgba[2] * color[2],
                                    stop.color.rgba[3] * color[3],
                                ];
                            }
                            node.style.fill = bf;
                        }
                        Fill::None => {}
                    }
                } else {
                    node.set_color(color);
                }
            }
            if let Some(sw) = target_stroke_w {
                node.set_stroke_width(sw);
            }
            if let Some(color) = target_stroke_col {
                let mut base_stroke = project
                    .anim_timeline
                    .nodes
                    .get(&node_id)
                    .and_then(|track| track.base_stroke.clone());
                if base_stroke.is_none() {
                    base_stroke = Some(node.style.stroke.style.clone());
                    if let Some(track) = project.anim_timeline.nodes.get_mut(&node_id) {
                        track.base_stroke = base_stroke.clone();
                    }
                }
                if let Some(mut bs) = base_stroke {
                    match &mut bs {
                        Fill::Solid(paint) => {
                            paint.rgba = color;
                            node.style.stroke.style = Fill::Solid(*paint);
                        }
                        Fill::LinearGradient { stops, .. } | Fill::RadialGradient { stops, .. } => {
                            for stop in stops {
                                stop.color.rgba = [
                                    stop.color.rgba[0] * color[0],
                                    stop.color.rgba[1] * color[1],
                                    stop.color.rgba[2] * color[2],
                                    stop.color.rgba[3] * color[3],
                                ];
                            }
                            node.style.stroke.style = bs;
                        }
                        Fill::None => {
                            node.set_stroke_color(color);
                        }
                    }
                } else {
                    node.set_stroke_color(color);
                }
            }
            if let Some(geom) = target_geom {
                set_node_geom_floats_project(project, node_id, &geom);
            }
        } else if let Some(layer) = project.document.layers.iter_mut().find(|l| {
            l.id == node_id && l.kind == crate::document::LayerKind::AV
        }) {
            if let Some(x) = target_x {
                layer.x = x as f32;
            }
            if let Some(y) = target_y {
                layer.y = y as f32;
            }
            if let Some(rot) = target_rot {
                layer.rotation = rot as f32;
            }
        }
    }

    // P5: sample Node Editor param tracks + eval Reals so blur/FX match the playhead.
    apply_node_editor_params_project(project, frame);
}

/// Apply `param:{uuid}` animation into GraphParam values and `eval_reals` for export.
fn apply_node_editor_params_project(project: &mut ProjectFile, frame: usize) {
    let fps = 30.0_f32; // frame/time nodes; wall anim_fps not critical for export consistency
    let layer_ids: Vec<uuid::Uuid> = project
        .document
        .layers
        .iter()
        .filter(|l| l.kind == crate::document::LayerKind::NodeEditor)
        .map(|l| l.id)
        .collect();
    for layer_id in layer_ids {
        let mut samples: Vec<(uuid::Uuid, Option<usize>, f64)> = Vec::new();
        if let Some(anim) = project.anim_timeline.nodes.get(&layer_id) {
            if let Some(layer) = project.document.layers.iter().find(|l| l.id == layer_id) {
                if let Some(g) = layer.node_graph.as_ref() {
                    for p in &g.parameters {
                        let labels: Vec<(String, Option<usize>)> = match p.kind {
                            crate::document::GraphParamKind::Real => {
                                vec![(format!("param:{}", p.id), None)]
                            }
                            crate::document::GraphParamKind::Color => (0..4)
                                .map(|i| (format!("param:{}:{i}", p.id), Some(i)))
                                .collect(),
                            crate::document::GraphParamKind::Position => (0..2)
                                .map(|i| (format!("param:{}:{i}", p.id), Some(i)))
                                .collect(),
                        };
                        for (lbl, comp) in labels {
                            if let Some(v) = anim.sample(&lbl, frame) {
                                samples.push((p.id, comp, v));
                            }
                        }
                    }
                }
            }
        }
        let Some(layer) = project
            .document
            .layers
            .iter_mut()
            .find(|l| l.id == layer_id)
        else {
            continue;
        };
        let Some(g) = layer.node_graph.as_mut() else {
            continue;
        };
        for (pid, comp, v) in samples {
            if let Some(p) = g.parameters.iter_mut().find(|p| p.id == pid) {
                match comp {
                    None | Some(0) => p.v0 = v,
                    Some(1) => p.v1 = v,
                    Some(2) => p.v2 = v,
                    Some(3) => p.v3 = v,
                    _ => {}
                }
            }
        }
        g.eval_reals(frame, fps);
    }
}

pub fn get_node_geom_floats_project(project: &ProjectFile, id: NodeId) -> Vec<f64> {
    let mut v = if let Some(node) = project.nodes.get(id) {
        node.get_geom_floats()
    } else {
        return Vec::new();
    };
    if let Some(tiling) = project
        .document
        .tiling_effects
        .values()
        .find(|e| e.source_id == id)
    {
        v.push(tiling.gap_x);
        v.push(tiling.gap_y);
        v.push(tiling.count_x as f64);
        v.push(tiling.count_y as f64);
        v.push(tiling.offset_x);
        v.push(tiling.offset_y);
        v.push(tiling.row_rotation);
        v.push(tiling.col_rotation);
        v.push(tiling.row_scale);
        v.push(tiling.col_scale);
    }
    if let Some(circ) = project
        .document
        .circular_effects
        .values()
        .find(|e| e.source_id == id)
    {
        v.push(circ.origin_x);
        v.push(circ.origin_y);
        v.push(circ.radius);
        v.push(circ.copies as f64);
        v.push(circ.angle_offset);
        v.push(circ.base_x);
        v.push(circ.base_y);
    }
    if let Some(oop) = project
        .document
        .path_effects
        .values()
        .find(|e| e.source_id == id)
    {
        v.push(oop.gap);
        v.push(oop.count as f64);
        v.push(oop.start_offset);
        v.push(oop.loft_end_scale as f64);
        v.push(oop.loft_end_opacity as f64);
    }
    v
}

pub fn set_node_geom_floats_project(project: &mut ProjectFile, id: NodeId, floats: &[f64]) {
    let base_len = project
        .nodes
        .get(id)
        .map(|n| n.get_geom_floats().len())
        .unwrap_or(0);
    if base_len > 0 && floats.len() >= base_len {
        if let Some(node) = project.nodes.get_mut(id) {
            node.set_geom_floats(&floats[..base_len]);
        }
    }
    let mut idx = base_len;
    if let Some(tiling_id) = project
        .document
        .tiling_effects
        .values()
        .find(|e| e.source_id == id)
        .map(|e| e.id)
    {
        if floats.len() >= idx + 10 {
            if let Some(tiling) = project.document.tiling_effects.get_mut(&tiling_id) {
                tiling.gap_x = floats[idx];
                tiling.gap_y = floats[idx + 1];
                tiling.count_x = floats[idx + 2].round().max(1.0) as usize;
                tiling.count_y = floats[idx + 3].round().max(1.0) as usize;
                tiling.offset_x = floats[idx + 4];
                tiling.offset_y = floats[idx + 5];
                tiling.row_rotation = floats[idx + 6];
                tiling.col_rotation = floats[idx + 7];
                tiling.row_scale = floats[idx + 8];
                tiling.col_scale = floats[idx + 9];
            }
            idx += 10;
        }
    }
    if let Some(circ_id) = project
        .document
        .circular_effects
        .values()
        .find(|e| e.source_id == id)
        .map(|e| e.id)
    {
        if floats.len() >= idx + 7 {
            if let Some(circ) = project.document.circular_effects.get_mut(&circ_id) {
                circ.origin_x = floats[idx];
                circ.origin_y = floats[idx + 1];
                circ.radius = floats[idx + 2];
                circ.copies = floats[idx + 3].round().max(1.0) as usize;
                circ.angle_offset = floats[idx + 4];
                circ.base_x = floats[idx + 5];
                circ.base_y = floats[idx + 6];
            }
            idx += 7;
        }
    }
    if let Some(oop_id) = project
        .document
        .path_effects
        .values()
        .find(|e| e.source_id == id)
        .map(|e| e.id)
    {
        if floats.len() >= idx + 5 {
            if let Some(oop) = project.document.path_effects.get_mut(&oop_id) {
                oop.gap = floats[idx];
                oop.count = floats[idx + 1].round().max(1.0) as usize;
                oop.start_offset = floats[idx + 2];
                oop.loft_end_scale = floats[idx + 3] as f32;
                oop.loft_end_opacity = floats[idx + 4] as f32;
            }
        }
    }
}

impl<'a> ExportSession<'a> {
    fn rasterize_frame_offscreen_gpu(
        &mut self,
        render_state: &egui_wgpu::RenderState,
        current_frame: usize,
        time_secs: f32,
        width: u32,
        height: u32,
    ) -> Option<Vec<u8>> {
        use egui_wgpu::wgpu;

        let mut input = egui::RawInput::default();
        input.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width as f32, height as f32)));

        // Decode Image nodes once per export; update AV layer textures each frame.
        self.ensure_node_image_textures();
        for (layer_id, buf) in &self.video_frames {
            let color_image = egui::ColorImage::from_rgba_unmultiplied(
                [buf.width as usize, buf.height as usize],
                &buf.rgba,
            );
            let handle = self.export_ctx.load_texture(
                format!("export-av-layer-{}", layer_id),
                color_image,
                egui::TextureOptions::LINEAR,
            );
            self.image_textures.insert(*layer_id, handle);
        }

        // Pre-bake NE FilePath textures (cached by FX key).
        let max_side = self.ne_bake_max_side(width, height);
        let ne_jobs: Vec<(uuid::Uuid, String, crate::document::GraphOutputEval)> = self
            .project
            .document
            .layers
            .iter()
            .filter(|l| l.visible && l.is_renderer && l.kind == crate::document::LayerKind::NodeEditor)
            .filter_map(|l| {
                let g = l.node_graph.as_ref()?;
                let eval = g.resolve_output_image();
                match &eval.image {
                    crate::document::GraphImageSource::FilePath(p) => {
                        Some((l.id, p.clone(), eval))
                    }
                    _ => None,
                }
            })
            .collect();
        for (lid, path, eval) in &ne_jobs {
            let _ = self.ensure_ne_fx_texture(*lid, path, eval, max_side);
        }

        // P6c: hide AppObject sources that feed NE Output (match canvas).
        let mut hidden_sources = std::collections::HashSet::new();
        for layer in &self.project.document.layers {
            if !layer.visible || layer.kind != crate::document::LayerKind::NodeEditor {
                continue;
            }
            if let Some(g) = &layer.node_graph {
                if let crate::document::GraphImageSource::AppObjects(ids) =
                    g.resolve_output_image().image
                {
                    for id in ids {
                        hidden_sources.insert(id);
                    }
                }
            }
        }

        let origin = egui::Pos2::ZERO;
        let viewport = crate::canvas::Viewport {
            pan: egui::Vec2::ZERO,
            zoom: self.scale,
            show_grid: false,
            snap_grid: false,
            grid_step: 20.0,
        };

        let draw_order = self.project.document.ordered_node_ids();
        let loft_paths = std::collections::HashSet::new();
        let image_textures = self.image_textures.clone();
        let fonts = &self.fonts;

        let output = self.export_ctx.run(input, |ctx| {
            let clip_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width as f32, height as f32));
            let painter = egui::Painter::new(
                ctx.clone(),
                egui::LayerId::background(),
                clip_rect,
            );

            let bg_color = egui::Color32::from_rgba_unmultiplied(
                (self.project.document.page_color[0] * 255.0) as u8,
                (self.project.document.page_color[1] * 255.0) as u8,
                (self.project.document.page_color[2] * 255.0) as u8,
                (self.project.document.page_color[3] * 255.0) as u8,
            );
            painter.rect_filled(clip_rect, 0.0, bg_color);

            for layer in &self.project.document.layers {
                if !layer.visible || !layer.is_renderer {
                    continue;
                }
                match layer.kind {
                    crate::document::LayerKind::Image | crate::document::LayerKind::Flowchart => {
                        let layer_set: std::collections::HashSet<uuid::Uuid> = layer.nodes.iter().copied().collect();
                        let layer_draw_order: Vec<uuid::Uuid> = draw_order
                            .iter()
                            .copied()
                            .filter(|id| layer_set.contains(id))
                            .collect();
                        crate::render::draw_nodes(
                            &painter,
                            &self.project.nodes,
                            &layer_draw_order,
                            &viewport,
                            origin,
                            self.project.document.width as f32,
                            self.project.document.height as f32,
                            &[],
                            &hidden_sources,
                            &loft_paths,
                            &fonts,
                            &image_textures,
                        );
                    }
                    crate::document::LayerKind::AV => {
                        // Match canvas: blank when playhead is outside all video clips.
                        if !layer.shows_video_at(time_secs) {
                            // skip — no freeze-frame outside clip
                        } else if let Some(tex) = image_textures.get(&layer.id) {
                            let mut dx = layer.x as f64;
                            let mut dy = layer.y as f64;
                            let mut rot = layer.rotation as f64;
                            let mut opacity = 1.0f32;
                            if let Some(track) = self.project.anim_timeline.nodes.get(&layer.id) {
                                if let Some(o) = track.opacity.interpolate(current_frame) {
                                    opacity = o as f32;
                                }
                                if let Some(x) = track.pos_x.interpolate(current_frame) {
                                    dx = x;
                                }
                                if let Some(y) = track.pos_y.interpolate(current_frame) {
                                    dy = y;
                                }
                                if let Some(r) = track.rotation.interpolate(current_frame) {
                                    rot = r;
                                }
                            }
                            let tex_w = tex.size()[0] as f32;
                            let tex_h = tex.size()[1] as f32;
                            let aspect = if tex_h > 0.0 { tex_w / tex_h } else { 1.0 };
                            let mut w = layer.width;
                            let mut h = layer.height;
                            if layer.aspect_ratio_locked {
                                if w / h > aspect {
                                    w = h * aspect;
                                } else {
                                    h = w / aspect;
                                }
                            }
                            let tl = viewport.doc_to_screen((dx, dy), origin);
                            let br = viewport.doc_to_screen((dx + w as f64, dy + h as f64), origin);
                            let rect = egui::Rect::from_min_max(tl, br);
                            let rot_rad = (rot as f32).to_radians();
                            paint_rotated_image(&painter, tex.id(), rect, rot_rad, opacity);
                        }
                    }
                    crate::document::LayerKind::Shading => {
                        let page_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(self.project.document.width as f32 * self.scale, self.project.document.height as f32 * self.scale));
                        crate::shading::draw_shading_passes(
                            &painter,
                            page_rect,
                            &layer.shading_passes,
                            time_secs,
                            Some(render_state),
                        );
                    }
                    crate::document::LayerKind::NodeEditor => {
                        // P5/P6b: composite Output Object image chain (proxy transform).
                        if let Some(g) = &layer.node_graph {
                            let eval = g.resolve_output_image();
                            match &eval.image {
                                crate::document::GraphImageSource::AppObjects(ids) => {
                                    if !ids.is_empty() {
                                        let order: Vec<uuid::Uuid> = draw_order
                                            .iter()
                                            .copied()
                                            .filter(|id| ids.contains(id))
                                            .chain(
                                                ids.iter()
                                                    .copied()
                                                    .filter(|id| !draw_order.contains(id)),
                                            )
                                            .collect();
                                        if !order.is_empty() {
                                            let mut hide = hidden_sources.clone();
                                            for id in &order {
                                                hide.remove(id);
                                            }
                                            crate::render::draw_nodes(
                                                &painter,
                                                &self.project.nodes,
                                                &order,
                                                &viewport,
                                                origin,
                                                self.project.document.width as f32,
                                                self.project.document.height as f32,
                                                &[],
                                                &hide,
                                                &loft_paths,
                                                &fonts,
                                                &image_textures,
                                            );
                                        }
                                    }
                                }
                                crate::document::GraphImageSource::FilePath(_path) => {
                                    // Texture pre-baked into image_textures[layer.id].
                                    if let Some(tex) = image_textures.get(&layer.id) {
                                        let (tw, th) = (
                                            tex.size()[0] as f64,
                                            tex.size()[1] as f64,
                                        );
                                        let (dx, dy, mut w, mut h, rot_rad) = layer
                                            .ne_output_paint_geom(
                                                &self.project.nodes,
                                                &eval,
                                            );
                                        let def_w = layer.width as f64;
                                        let def_h = layer.height as f64;
                                        let near_default = (w - def_w).abs() < 2.0
                                            && (h - def_h).abs() < 2.0;
                                        let near_a4 = (w
                                            - crate::document::A4_WIDTH_PX)
                                            .abs()
                                            < 2.0
                                            && (h - crate::document::A4_HEIGHT_PX)
                                                .abs()
                                                < 2.0;
                                        if near_default || near_a4 {
                                            let page_w =
                                                self.project.document.width.max(1.0);
                                            let page_h =
                                                self.project.document.height.max(1.0);
                                            let mut nw = tw;
                                            let mut nh = th;
                                            if nw > page_w || nh > page_h {
                                                let s = (page_w / nw)
                                                    .min(page_h / nh);
                                                nw *= s;
                                                nh *= s;
                                            }
                                            w = nw.max(1.0);
                                            h = nh.max(1.0);
                                        }
                                        let tl = viewport
                                            .doc_to_screen((dx, dy), origin);
                                        let br = viewport.doc_to_screen(
                                            (dx + w, dy + h),
                                            origin,
                                        );
                                        let rect =
                                            egui::Rect::from_min_max(tl, br);
                                        paint_rotated_image(
                                            &painter,
                                            tex.id(),
                                            rect,
                                            rot_rad as f32,
                                            1.0,
                                        );
                                    }
                                }
                                crate::document::GraphImageSource::Empty => {}
                            }
                        }
                    }
                }
            }

            crate::render::draw_path_effects(
                &painter,
                &self.project.nodes,
                &self.project.document.path_effects,
                &viewport,
                origin,
                &fonts,
                &image_textures,
                &[],
            );
            crate::render::draw_tiling_effects(
                &painter,
                &self.project.nodes,
                &self.project.document.tiling_effects,
                &viewport,
                origin,
                &fonts,
                &image_textures,
                &[],
            );
            crate::render::draw_circular_effects(
                &painter,
                &self.project.nodes,
                &self.project.document.circular_effects,
                &viewport,
                origin,
                &fonts,
                &image_textures,
                &[],
            );
            crate::render::draw_clip_mask_effects(
                &painter,
                &self.project.nodes,
                &self.project.document.clip_masks,
                &viewport,
                origin,
                &fonts,
                &image_textures,
                &[],
            );
        });

        let device = &render_state.device;
        let queue = &render_state.queue;

        let offscreen_renderer = self.offscreen_renderer.as_mut()?;

        for (id, image_delta) in output.textures_delta.set {
            offscreen_renderer.update_texture(device, queue, id, &image_delta);
        }

        let clipped_primitives = self.export_ctx.tessellate(output.shapes, 1.0);

        let texture_desc = wgpu::TextureDescriptor {
            label: Some("egui_export_texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        };
        let texture = device.create_texture(&texture_desc);
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point: 1.0,
        };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("egui_export_encoder"),
        });

        let command_buffers = offscreen_renderer.update_buffers(
            device,
            queue,
            &mut encoder,
            &clipped_primitives,
            &screen_descriptor,
        );
        queue.submit(command_buffers);

        {
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui_export_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            let mut static_render_pass = render_pass.forget_lifetime();
            offscreen_renderer.render(&mut static_render_pass, &clipped_primitives, &screen_descriptor);
            drop(static_render_pass);
        }

        let row_bytes = width * 4;
        let aligned_row_bytes = (row_bytes + 255) & !255;
        let buffer_size = (aligned_row_bytes * height) as u64;

        let readback_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("egui_export_readback_buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(aligned_row_bytes),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        queue.submit(Some(encoder.finish()));

        let (res_tx, res_rx) = std::sync::mpsc::channel();
        let buffer_slice = readback_buffer.slice(..);
        buffer_slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = res_tx.send(res);
        });

        let _ = device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });

        if let Ok(Ok(())) = res_rx.recv() {
            let data = buffer_slice.get_mapped_range();
            let mut rgba = Vec::with_capacity((width * height * 4) as usize);
            for row in 0..height {
                let start = (row * aligned_row_bytes) as usize;
                let end = start + (width * 4) as usize;
                rgba.extend_from_slice(&data[start..end]);
            }
            drop(data);
            readback_buffer.unmap();

            for id in output.textures_delta.free {
                offscreen_renderer.free_texture(&id);
            }

            Some(rgba)
        } else {
            None
        }
    }
}

fn paint_rotated_image(
    painter: &egui::Painter,
    texture_id: egui::TextureId,
    rect: egui::Rect,
    rotation_rad: f32,
    opacity: f32,
) {
    let mut mesh = egui::Mesh::with_texture(texture_id);
    let color = egui::Color32::WHITE.gamma_multiply(opacity);
    let mut points = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    if rotation_rad != 0.0 {
        let center = rect.center();
        let cos = rotation_rad.cos();
        let sin = rotation_rad.sin();
        for pt in &mut points {
            let d = *pt - center;
            let rx = d.x * cos - d.y * sin;
            let ry = d.x * sin + d.y * cos;
            *pt = center + egui::vec2(rx, ry);
        }
    }
    mesh.vertices.push(egui::epaint::Vertex { pos: points[0], uv: egui::pos2(0.0, 0.0), color });
    mesh.vertices.push(egui::epaint::Vertex { pos: points[1], uv: egui::pos2(1.0, 0.0), color });
    mesh.vertices.push(egui::epaint::Vertex { pos: points[2], uv: egui::pos2(1.0, 1.0), color });
    mesh.vertices.push(egui::epaint::Vertex { pos: points[3], uv: egui::pos2(0.0, 1.0), color });
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
}
