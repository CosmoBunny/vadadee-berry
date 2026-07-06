//! Asynchronous video recorder — consumes frames from a channel on a dedicated thread.

use std::sync::mpsc::Receiver;
use std::thread::{self, JoinHandle};

use super::sync::{Frame, RecorderConfig, SyncRecorder};

/// Background encoder that reads frames from `rx` until the sender is dropped.
pub struct AsyncRecorder {
    join: Option<JoinHandle<Result<(), String>>>,
}

impl AsyncRecorder {
    /// Spawn the encoder thread. The thread exits when all senders to `rx` are dropped.
    pub fn spawn(rx: Receiver<Frame>, config: RecorderConfig) -> Self {
        let join = thread::Builder::new()
            .name("vadadee-async-recorder".into())
            .spawn(move || run_encoder(rx, config))
            .expect("failed to spawn async recorder thread");
        Self { join: Some(join) }
    }

    /// Wait for the encoder thread to finish and return its result.
    pub fn join(mut self) -> Result<(), String> {
        match self.join.take() {
            Some(handle) => handle
                .join()
                .map_err(|_| "async recorder thread panicked".to_string())?,
            None => Ok(()),
        }
    }
}

impl Drop for AsyncRecorder {
    fn drop(&mut self) {
        if let Some(handle) = self.join.take() {
            if let Err(e) = handle
                .join()
                .map_err(|_| "async recorder thread panicked".to_string())
                .and_then(|r| r)
            {
                log::error!("AsyncRecorder drop: {e}");
            }
        }
    }
}

fn run_encoder(rx: Receiver<Frame>, config: RecorderConfig) -> Result<(), String> {
    let mut recorder = SyncRecorder::start(config)?;
    for frame in rx {
        recorder.write_frame(&frame)?;
    }
    recorder.finish()
}
