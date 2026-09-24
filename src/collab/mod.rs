#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod desktop;
mod protocol;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod relay;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod sync_project;

pub use protocol::{ChatLine, CollabMessage, RemotePeer};
#[cfg(any(target_os = "android", target_os = "ios"))]
mod stub;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub use desktop::*;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub use sync_project::*;
#[cfg(any(target_os = "android", target_os = "ios"))]
pub use stub::*;