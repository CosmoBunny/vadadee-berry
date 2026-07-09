//! Timeline audio mix + mux into exported video (libav, no subprocess).

use std::path::{Path, PathBuf};

use crate::app::VideoFormat;
use crate::document::{LayerKind, ProjectFile};

const EXPORT_SAMPLE_RATE: u32 = 44_100;

#[derive(Clone)]
struct ExportAudioLayer {
    path: String,
    timeline_start: f32,
    start_offset: f32,
    play_secs: f32,
    volume: f32,
}

pub fn export_mux_with_audio(
    project: &ProjectFile,
    temp_video: &Path,
    output_path: &Path,
    work_dir: &Path,
    duration_secs: f32,
    format: VideoFormat,
) -> Result<bool, String> {
    if !temp_video.exists() {
        return Ok(false);
    }

    let layers = collect_export_audio_layers(project);
    let supports_audio_mux = matches!(format, VideoFormat::Mp4 | VideoFormat::Mkv | VideoFormat::Mov | VideoFormat::Webm);

    if layers.is_empty() || !supports_audio_mux {
        if layers.is_empty() {
            log::info!("export: no timeline audio/video layers for mux — video only");
        }
        std::fs::copy(temp_video, output_path)
            .map_err(|e| format!("Could not copy video to output: {e}"))?;
        return Ok(true);
    }

    log::info!(
        "export: mixing {} timeline audio source(s) ({duration_secs:.2}s)",
        layers.len()
    );
    for layer in &layers {
        log::info!(
            "export audio: layer {:?} timeline {:.3}s..{:.3}s (offset {:.3}s, vol {:.2})",
            layer.path,
            layer.timeline_start,
            layer.timeline_start + layer.play_secs,
            layer.start_offset,
            layer.volume
        );
        if !layer_overlaps_export(layer, duration_secs) {
            log::warn!(
                "export audio: layer {:?} does not overlap export window 0..{duration_secs:.3}s",
                layer.path
            );
        }
    }
    let pcm = mix_timeline_audio_stereo_i16(&layers, duration_secs, EXPORT_SAMPLE_RATE, output_path)?;
    let mix_peak = pcm_peak(&pcm);
    log::info!(
        "export audio: mixed PCM peak {mix_peak} ({} frames @ {} Hz)",
        pcm.len() / 2,
        EXPORT_SAMPLE_RATE
    );

    // If the mix came out silent, warn but still produce a valid video-only file
    // instead of aborting the export entirely.
    if pcm.is_empty() || !pcm_has_audible_samples(&pcm) {
        log::warn!(
            "export audio: mixed PCM is silent (peak {mix_peak}) — exporting video without audio. \
             Check that audio clips overlap 0..{duration_secs:.2}s on the timeline, \
             volume > 0, and the layer is an AV renderer layer."
        );
        std::fs::copy(temp_video, output_path)
            .map_err(|e| format!("Could not copy video to output: {e}"))?;
        return Ok(true);
    }

    let temp_audio = work_dir.join("temp_export_audio.m4a");
    crate::video_decode::write_stereo_i16_as_aac_mp4_libav(
        &temp_audio,
        &pcm,
        EXPORT_SAMPLE_RATE,
        192,
        |_| {},
    )?;

    // If remux fails, warn and fall back to video-only rather than failing the export.
    match crate::video_decode::remux_video_and_audio_libav(temp_video, &temp_audio, output_path) {
        Ok(()) => {}
        Err(e) => {
            log::warn!("export audio: remux failed ({e}) — falling back to video without audio");
            std::fs::copy(temp_video, output_path)
                .map_err(|ce| format!("Could not copy video to output: {ce}"))?;
        }
    }
    let _ = std::fs::remove_file(&temp_audio);
    Ok(true)
}

fn collect_export_audio_layers(project: &ProjectFile) -> Vec<ExportAudioLayer> {
    let mut out = Vec::new();
    for layer in &project.document.layers {
        if !layer.visible || !layer.is_renderer || !matches!(layer.kind, LayerKind::AV) {
            continue;
        }
        let mut layer_clone = layer.clone();
        // Push Active Track Trim Start / Play Duration onto primary clip before mix.
        layer_clone.prepare_av_for_export();

        if !layer_clone.av_clips.is_empty() {
            // Stable track order: sub-row then timeline position.
            let mut clips: Vec<_> = layer_clone.av_clips.iter().collect();
            clips.sort_by(|a, b| {
                a.track_row
                    .cmp(&b.track_row)
                    .then(
                        a.video_timeline_start
                            .partial_cmp(&b.video_timeline_start)
                            .unwrap_or(std::cmp::Ordering::Equal),
                    )
            });
            for clip in clips {
                if clip.media_path.is_empty() {
                    continue;
                }
                let start_offset = clip.video_start_offset.max(0.0);
                let play_secs = clip.timeline_play_secs();
                if play_secs <= 0.0 {
                    continue;
                }
                out.push(ExportAudioLayer {
                    path: clip.media_path.clone(),
                    timeline_start: clip.video_timeline_start.max(0.0),
                    start_offset,
                    play_secs,
                    volume: layer_clone.volume.max(0.0),
                });
            }
        } else if !layer_clone.video_path.is_empty() {
            let play_secs = layer_clone.timeline_play_secs();
            if play_secs > 0.0 {
                out.push(ExportAudioLayer {
                    path: layer_clone.video_path.clone(),
                    timeline_start: layer_clone.video_timeline_start.max(0.0),
                    start_offset: layer_clone.video_start_offset.max(0.0),
                    play_secs,
                    volume: layer_clone.volume.max(0.0),
                });
            }
        }
    }
    // Global order: timeline start then path (deterministic mix).
    out.sort_by(|a, b| {
        a.timeline_start
            .partial_cmp(&b.timeline_start)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
    });
    out
}

fn pcm_has_audible_samples(pcm: &[i16]) -> bool {
    pcm_peak(pcm) >= 500
}

fn pcm_peak(pcm: &[i16]) -> i16 {
    pcm.iter().map(|&s| (s as i32).abs() as i16).max().unwrap_or(0)
}

fn layer_overlaps_export(layer: &ExportAudioLayer, export_secs: f32) -> bool {
    layer.play_secs > 0.0
        && layer.timeline_start < export_secs
        && layer.timeline_start + layer.play_secs > 0.0
}

fn resolve_media_path(path: &str, output_path: &Path) -> PathBuf {
    let raw = PathBuf::from(path);
    if raw.is_absolute() && raw.exists() {
        return raw;
    }
    if raw.exists() {
        return raw;
    }
    if let Some(parent) = output_path.parent() {
        let joined = parent.join(&raw);
        if joined.exists() {
            return joined;
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        let joined = cwd.join(&raw);
        if joined.exists() {
            return joined;
        }
    }
    raw
}

fn mix_timeline_audio_stereo_i16(
    layers: &[ExportAudioLayer],
    duration_secs: f32,
    sample_rate: u32,
    output_path: &Path,
) -> Result<Vec<i16>, String> {
    let duration_secs = duration_secs.max(0.0);
    let out_frames = (duration_secs * sample_rate as f32).ceil() as usize;
    if out_frames == 0 {
        return Ok(Vec::new());
    }

    let mut mix = vec![0f32; out_frames * 2];

    for layer in layers {
        if layer.volume <= 0.0 || layer.play_secs <= 0.0 {
            continue;
        }
        let resolved = resolve_media_path(&layer.path, output_path);
        let resolved_str = resolved.to_string_lossy();
        let (src, src_rate) = match load_stereo_i16_layer(&resolved) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("export audio: skip layer {}: {e}", resolved_str);
                continue;
            }
        };
        if src.is_empty() {
            log::warn!("export audio: empty decode for {}", resolved_str);
            continue;
        }
        log::info!(
            "export audio: layer {} — {} samples @ {} Hz",
            resolved_str,
            src.len() / 2,
            src_rate
        );
        let src_frames = src.len() / 2;
        let timeline_end = layer.timeline_start + layer.play_secs;

        for out_frame in 0..out_frames {
            let t = out_frame as f32 / sample_rate as f32;
            if t < layer.timeline_start || t >= timeline_end {
                continue;
            }
            let elapsed = t - layer.timeline_start;
            let src_t = layer.start_offset + elapsed;
            if src_t < 0.0 {
                continue;
            }
            let src_pos = src_t * src_rate as f32;
            let idx0 = src_pos.floor() as usize;
            let frac = src_pos.fract();
            let idx1 = (idx0 + 1).min(src_frames.saturating_sub(1));
            if idx0 >= src_frames {
                continue;
            }
            let l = lerp_i16_to_f32(&src, idx0, 0, idx1, frac) * layer.volume;
            let r = lerp_i16_to_f32(&src, idx0, 1, idx1, frac) * layer.volume;
            let o = out_frame * 2;
            mix[o] += l;
            mix[o + 1] += r;
        }
    }

    let mut out = Vec::with_capacity(mix.len());
    for s in mix {
        let clamped = s.clamp(-1.0, 1.0);
        out.push((clamped * i16::MAX as f32) as i16);
    }
    Ok(out)
}

fn lerp_i16_to_f32(
    src: &[i16],
    idx0: usize,
    ch: usize,
    idx1: usize,
    frac: f32,
) -> f32 {
    let a = src[idx0 * 2 + ch] as f32 / i16::MAX as f32;
    let b = src[idx1 * 2 + ch] as f32 / i16::MAX as f32;
    a + (b - a) * frac
}

fn load_stereo_i16_layer(path: &Path) -> Result<(Vec<i16>, u32), String> {
    let path_str = path.to_string_lossy();
    if !path.exists() {
        return Err(format!("file not found: {path_str}"));
    }
    if is_video_container_ext(&path_str) {
        return crate::video_decode::decode_audio_to_stereo_i16_libav(path_str.as_ref(), |_| {});
    }
    if let Some(pcm) = crate::audio_extract::load_pcm_from_file(path) {
        return Ok((
            f32_interleaved_to_stereo_i16(&pcm.samples, pcm.channels),
            pcm.sample_rate,
        ));
    }
    Err(format!("Could not load audio for export: {path_str}"))
}

fn f32_interleaved_to_stereo_i16(samples: &[f32], channels: u16) -> Vec<i16> {
    let ch = channels.max(1) as usize;
    let frames = samples.len() / ch;
    let mut out = Vec::with_capacity(frames * 2);
    for f in 0..frames {
        let base = f * ch;
        let l = samples.get(base).copied().unwrap_or(0.0);
        let r = if ch > 1 {
            samples.get(base + 1).copied().unwrap_or(l)
        } else {
            l
        };
        out.push((l.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
        out.push((r.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
    }
    out
}

fn is_video_container_ext(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "mp4" | "m4v" | "mov" | "mkv" | "webm" | "avi"
            )
        })
}

#[cfg(test)]
mod export_audio_tests {
    use super::*;
    use crate::document::{AvClip, Layer, LayerKind, ProjectFile};
    use std::path::Path;
    use uuid::Uuid;

    #[test]
    fn prepare_pushes_layer_trim_start_onto_primary_clip() {
        let mut layer = Layer::new_av_layer(Uuid::new_v4(), "Audio".into(), "track.mp3".into());
        layer.av_clips.clear();
        layer.av_clips.push(AvClip {
            id: Uuid::new_v4(),
            name: "clip".into(),
            media_path: "track.mp3".into(),
            video_start_offset: 0.0,
            video_play_length: 120.0,
            video_timeline_start: 0.0,
            media_source_duration: Some(180.0),
            track_row: 0,
        });
        layer.video_start_offset = 40.0;
        layer.video_play_length = 60.0;
        layer.media_source_duration = Some(180.0);
        layer.prepare_av_for_export();
        let clip = layer.av_clips.first().expect("primary");
        assert!(
            (clip.video_start_offset - 40.0).abs() < 1e-4,
            "expected trim start 40s on clip, got {}",
            clip.video_start_offset
        );
        assert!((clip.video_play_length - 60.0).abs() < 1e-4);
        assert!((clip.timeline_play_secs() - 60.0).abs() < 1e-4);
    }

    #[test]
    fn collect_export_audio_respects_trim_start_and_sorts() {
        let mut layer = Layer::new_av_layer(Uuid::new_v4(), "A".into(), "a.mp3".into());
        layer.kind = LayerKind::AV;
        layer.visible = true;
        layer.is_renderer = true;
        layer.volume = 1.0;
        layer.video_start_offset = 40.0;
        layer.video_play_length = 10.0;
        layer.media_source_duration = Some(200.0);
        layer.av_clips = vec![
            // Primary clip (index 0) receives Active Track Trim Start via prepare.
            AvClip {
                id: Uuid::new_v4(),
                name: "early".into(),
                media_path: "a.mp3".into(),
                video_start_offset: 0.0, // stale — prepare should push 40 from layer
                video_play_length: 10.0,
                video_timeline_start: 0.0,
                media_source_duration: Some(200.0),
                track_row: 0,
            },
            AvClip {
                id: Uuid::new_v4(),
                name: "late".into(),
                media_path: "b.mp3".into(),
                video_start_offset: 0.0,
                video_play_length: 5.0,
                video_timeline_start: 20.0,
                media_source_duration: Some(50.0),
                track_row: 1,
            },
        ];
        let mut doc = crate::document::Document::new_empty_project().document;
        doc.layers.clear();
        doc.layers.push(layer);
        let project = ProjectFile::new(doc, crate::document::NodeStore::default());

        let layers = collect_export_audio_layers(&project);
        assert_eq!(layers.len(), 2, "expected both clips");
        // Sorted by timeline start: early (0) then late (20)
        assert_eq!(layers[0].path, "a.mp3");
        assert!(
            (layers[0].start_offset - 40.0).abs() < 1e-4,
            "primary trim start should be 40s, got {}",
            layers[0].start_offset
        );
        assert_eq!(layers[1].path, "b.mp3");
        assert!((layers[1].timeline_start - 20.0).abs() < 1e-4);
        assert!((layers[1].start_offset - 0.0).abs() < 1e-4);
    }

    #[test]
    fn mix_applies_start_offset_if_ozen_present() {
        let path = {
            let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("OZEN.mp3");
            if local.exists() {
                local
            } else {
                return;
            }
        };
        let (src, sr) = load_stereo_i16_layer(&path).expect("load");
        if sr == 0 || src.len() < sr as usize * 2 * 50 {
            return; // need ~50s of audio
        }
        // Sample at t=0 vs t=40s should differ if track is not silence/constant.
        let at0 = src[0].abs().max(src[1].abs());
        let idx40 = (40.0 * sr as f32) as usize * 2;
        if idx40 + 1 >= src.len() {
            return;
        }
        let at40 = src[idx40].abs().max(src[idx40 + 1].abs());

        let layers_trim = vec![ExportAudioLayer {
            path: path.to_string_lossy().into_owned(),
            timeline_start: 0.0,
            start_offset: 40.0,
            play_secs: 1.0,
            volume: 1.0,
        }];
        let layers_head = vec![ExportAudioLayer {
            path: path.to_string_lossy().into_owned(),
            timeline_start: 0.0,
            start_offset: 0.0,
            play_secs: 1.0,
            volume: 1.0,
        }];
        let pcm_trim =
            mix_timeline_audio_stereo_i16(&layers_trim, 1.0, EXPORT_SAMPLE_RATE, &path).unwrap();
        let pcm_head =
            mix_timeline_audio_stereo_i16(&layers_head, 1.0, EXPORT_SAMPLE_RATE, &path).unwrap();
        // First frame of each mix should track different source positions.
        let d0 = (pcm_trim[0] as i32 - pcm_head[0] as i32).abs()
            + (pcm_trim[1] as i32 - pcm_head[1] as i32).abs();
        // Only assert when source content at 0 vs 40 differs.
        if (at0 as i32 - at40 as i32).abs() > 200 {
            assert!(
                d0 > 50,
                "trim mix should not match untrimmed head (diff {d0})"
            );
        }
    }

    #[test]
    fn mp3_mix_and_aac_roundtrip_if_ozen_present() {
        // Look in project root first (checked in), fall back to Downloads.
        let path = {
            let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("OZEN.mp3");
            if local.exists() {
                local
            } else {
                Path::new("/home/angsudo/Downloads/OZEN.mp3").to_path_buf()
            }
        };
        if !path.exists() {
            return;
        }
        let (src, sr) = load_stereo_i16_layer(&path).expect("load mp3");
        let src_peak = src.iter().map(|&s| (s as i32).abs() as i16).max().unwrap_or(0);
        assert!(src_peak > 500, "source peak {src_peak}");
        assert!(sr > 0, "sample_rate");

        let layers = vec![ExportAudioLayer {
            path: path.to_string_lossy().into_owned(),
            timeline_start: 0.0,
            start_offset: 0.0,
            play_secs: 10.02,
            volume: 1.0,
        }];
        let pcm = mix_timeline_audio_stereo_i16(&layers, 10.02, EXPORT_SAMPLE_RATE, &path)
            .expect("mix");
        let mix_peak = pcm.iter().map(|&s| (s as i32).abs() as i16).max().unwrap_or(0);
        assert!(mix_peak > 500, "mix peak {mix_peak}");

        if !crate::video_decode::is_libav_available() {
            return;
        }
        let out = std::env::temp_dir().join(format!("vadadee_export_aac_{}.m4a", std::process::id()));
        crate::video_decode::write_stereo_i16_as_aac_mp4_libav(&out, &pcm, EXPORT_SAMPLE_RATE, 192, |_| {})
            .expect("aac encode");
        let wav = std::env::temp_dir().join(format!("vadadee_export_aac_{}.wav", std::process::id()));
        std::process::Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-i", out.to_str().unwrap(), "-f", "wav", wav.to_str().unwrap()])
            .status()
            .expect("ffmpeg");
        let bytes = std::fs::read(&wav).expect("wav");
        let mut dec_peak = 0i16;
        for chunk in bytes[44..].chunks_exact(2) {
            let s = i16::from_le_bytes([chunk[0], chunk[1]]);
            dec_peak = dec_peak.max((s as i32).abs() as i16);
        }
        assert!(dec_peak > 500, "decoded aac peak {dec_peak}");

        let anim_file = Path::new("/home/angsudo/project/vadadee-berry/animation.mp4");
        let manifest_anim = Path::new(env!("CARGO_MANIFEST_DIR")).join("animation.mp4");
        let anim_path = if anim_file.exists() {
            Some(anim_file)
        } else if manifest_anim.exists() {
            Some(manifest_anim.as_path())
        } else {
            None
        };

        if let Some(ap) = anim_path {
            let video_only = std::env::temp_dir().join(format!("vadadee_vonly_{}.mp4", std::process::id()));
            std::process::Command::new("ffmpeg")
                .args([
                    "-y",
                    "-v",
                    "error",
                    "-i",
                    ap.to_str().unwrap(),
                    "-an",
                    "-c:v",
                    "copy",
                    video_only.to_str().unwrap(),
                ])
                .status()
                .expect("video only");
            let remuxed = std::env::temp_dir().join(format!("vadadee_remux_{}.mp4", std::process::id()));
            crate::video_decode::remux_video_and_audio_libav(&video_only, &out, &remuxed)
                .expect("remux");
            let remux_wav = std::env::temp_dir().join(format!("vadadee_remux_{}.wav", std::process::id()));
            std::process::Command::new("ffmpeg")
                .args(["-y", "-v", "error", "-i", remuxed.to_str().unwrap(), "-map", "0:a:0", "-f", "wav", remux_wav.to_str().unwrap()])
                .status()
                .expect("decode remux");
            let bytes = std::fs::read(&remux_wav).expect("remux wav");
            let mut remux_peak = 0i16;
            for chunk in bytes[44..].chunks_exact(2) {
                let s = i16::from_le_bytes([chunk[0], chunk[1]]);
                remux_peak = remux_peak.max((s as i32).abs() as i16);
            }
            assert!(remux_peak > 500, "remuxed audio peak {remux_peak}");

            let _ = std::fs::remove_file(video_only);
            let _ = std::fs::remove_file(remuxed);
            let _ = std::fs::remove_file(remux_wav);
        } else {
            println!("Skipping audio remux smoke test because animation.mp4 is not present.");
        }

        let _ = std::fs::remove_file(out);
        let _ = std::fs::remove_file(wav);
    }
}