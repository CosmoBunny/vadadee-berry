//! Video frame recorders — sync and async encoding pipelines.

pub mod async_bridge;
pub mod async_recorder;
pub mod sync;

pub use async_bridge::AsyncBridge;
pub use async_recorder::AsyncRecorder;
pub use sync::{Frame, RecorderConfig, SyncRecorder};
