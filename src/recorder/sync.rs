//! Synchronous video frame recorder — encodes RGBA frames on the caller thread.

use std::path::PathBuf;

use crate::video_decode::LibavEncoder;

/// Encoder settings shared by sync and async recorders.
#[derive(Debug, Clone)]
pub struct RecorderConfig {
    pub output_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub vcodec: String,
    /// `1` = power saving, `0` = libav auto (all cores).
    pub encoder_threads: u32,
}

impl RecorderConfig {
    pub fn output_path_str(&self) -> Result<&str, String> {
        self.output_path
            .to_str()
            .ok_or_else(|| format!("Invalid output path: {}", self.output_path.display()))
    }
}

/// One RGBA frame destined for the encoder.
#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Frame {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        Self {
            width,
            height,
            rgba,
        }
    }

    pub fn from_parts(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, String> {
        let frame = Self::new(width, height, rgba);
        frame.validate()?;
        Ok(frame)
    }

    pub fn validate(&self) -> Result<(), String> {
        let expected = (self.width as usize)
            .checked_mul(self.height as usize)
            .and_then(|p| p.checked_mul(4))
            .ok_or_else(|| "Frame dimensions overflow".to_string())?;
        if self.rgba.len() != expected {
            return Err(format!(
                "RGBA buffer size mismatch: got {}, expected {} ({}x{})",
                self.rgba.len(),
                expected,
                self.width,
                self.height
            ));
        }
        Ok(())
    }
}

/// Blocking encoder — each `write_frame` call encodes immediately on the caller thread.
pub struct SyncRecorder {
    encoder: LibavEncoder,
    width: u32,
    height: u32,
}

impl SyncRecorder {
    pub fn start(config: RecorderConfig) -> Result<Self, String> {
        if config.width == 0 || config.height == 0 {
            return Err("Recorder width/height must be non-zero".into());
        }
        let path = config.output_path_str()?;
        let encoder = LibavEncoder::new(
            path,
            config.width,
            config.height,
            config.fps,
            config.bitrate_kbps,
            &config.vcodec,
            config.encoder_threads,
        )?;
        Ok(Self {
            encoder,
            width: config.width,
            height: config.height,
        })
    }

    pub fn write_frame(&mut self, frame: &Frame) -> Result<(), String> {
        frame.validate()?;
        if frame.width != self.width || frame.height != self.height {
            return Err(format!(
                "Frame size {}x{} does not match encoder {}x{}",
                frame.width, frame.height, self.width, self.height
            ));
        }
        self.encoder.write_frame(&frame.rgba)
    }

    pub fn finish(self) -> Result<(), String> {
        self.encoder.finish()
    }
}
