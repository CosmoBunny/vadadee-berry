//! Bridge between sync and async recording — swaps to async mode on `start_recording`.

use std::sync::mpsc::{self, SyncSender};

use super::async_recorder::AsyncRecorder;
use super::sync::{Frame, RecorderConfig, SyncRecorder};

/// Bounded queue depth — limits in-flight RGBA frames between rasterizer and encoder.
pub const DEFAULT_FRAME_QUEUE_DEPTH: usize = 3;

enum BridgeMode {
    /// Configured but not yet encoding.
    Idle(RecorderConfig),
    /// Frames are encoded synchronously on the caller thread.
    Sync(SyncRecorder),
    /// Frames are sent to a background encoder thread.
    Async {
        tx: SyncSender<Frame>,
        recorder: AsyncRecorder,
    },
}

/// Unified recorder facade: synchronous before `start_recording`, asynchronous after.
pub struct AsyncBridge {
    mode: BridgeMode,
}

impl AsyncBridge {
    pub fn new(config: RecorderConfig) -> Self {
        Self {
            mode: BridgeMode::Idle(config),
        }
    }

    pub fn is_recording(&self) -> bool {
        !matches!(self.mode, BridgeMode::Idle(_))
    }

    pub fn is_async(&self) -> bool {
        matches!(self.mode, BridgeMode::Async { .. })
    }

    /// Begin synchronous recording on the caller thread.
    pub fn start_sync(&mut self) -> Result<(), String> {
        let config = match std::mem::replace(
            &mut self.mode,
            BridgeMode::Idle(dummy_config()),
        ) {
            BridgeMode::Idle(config) => config,
            other => {
                self.mode = other;
                return Err("Recorder already active".into());
            }
        };
        self.mode = BridgeMode::Sync(SyncRecorder::start(config)?);
        Ok(())
    }

    /// Swap from idle into asynchronous recording.
    ///
    /// Returns a clone of the bounded frame sender so the caller can push frames
    /// without holding the bridge mutably. The queue applies backpressure when full.
    /// Dropping all senders signals end-of-stream.
    pub fn start_recording(&mut self) -> Result<SyncSender<Frame>, String> {
        self.start_recording_with_depth(DEFAULT_FRAME_QUEUE_DEPTH)
    }

    /// Like [`start_recording`] but with a custom bounded queue depth.
    pub fn start_recording_with_depth(
        &mut self,
        queue_depth: usize,
    ) -> Result<SyncSender<Frame>, String> {
        let config = match &self.mode {
            BridgeMode::Idle(cfg) => cfg.clone(),
            BridgeMode::Sync(_) => {
                return Err(
                    "Cannot start async recording while sync recorder is active; call stop_recording first"
                        .into(),
                );
            }
            BridgeMode::Async { .. } => return Err("Already recording asynchronously".into()),
        };

        let (tx, rx) = mpsc::sync_channel(queue_depth.max(1));
        let recorder = AsyncRecorder::spawn(rx, config);
        let tx_clone = tx.clone();
        self.mode = BridgeMode::Async { tx, recorder };
        Ok(tx_clone)
    }

    /// Write a frame using the current mode (sync or async).
    pub fn write_frame(&mut self, frame: Frame) -> Result<(), String> {
        match &mut self.mode {
            BridgeMode::Idle(_) => Err("Recording not started".into()),
            BridgeMode::Sync(recorder) => recorder.write_frame(&frame),
            BridgeMode::Async { tx, .. } => tx
                .send(frame)
                .map_err(|e| format!("Failed to send frame to async recorder: {e}")),
        }
    }

    /// Stop recording and finalize the output file.
    pub fn stop_recording(&mut self) -> Result<(), String> {
        match std::mem::replace(&mut self.mode, BridgeMode::Idle(dummy_config())) {
            BridgeMode::Idle(config) => {
                self.mode = BridgeMode::Idle(config);
                Ok(())
            }
            BridgeMode::Sync(recorder) => recorder.finish(),
            BridgeMode::Async { tx, recorder } => {
                drop(tx);
                recorder.join()
            }
        }
    }

    /// Borrow the async frame sender when in async mode.
    pub fn frame_sender(&self) -> Option<&SyncSender<Frame>> {
        match &self.mode {
            BridgeMode::Async { tx, .. } => Some(tx),
            _ => None,
        }
    }

    /// Take the stored config when idle (e.g. to rebuild the bridge after export).
    pub fn into_config(self) -> Option<RecorderConfig> {
        match self.mode {
            BridgeMode::Idle(config) => Some(config),
            _ => None,
        }
    }
}

fn dummy_config() -> RecorderConfig {
    RecorderConfig {
        output_path: std::path::PathBuf::new(),
        width: 0,
        height: 0,
        fps: 0,
        bitrate_kbps: 0,
        vcodec: String::new(),
        encoder_threads: 0,
    }
}
