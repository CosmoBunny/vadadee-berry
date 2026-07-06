#[cfg(not(target_os = "android"))]
mod desktop;
mod protocol;
#[cfg(not(target_os = "android"))]
mod relay;
#[cfg(not(target_os = "android"))]
mod sync_project;

pub use protocol::{ChatLine, CollabMessage, RemotePeer};
#[cfg(target_os = "android")]
mod stub;

#[cfg(not(target_os = "android"))]
pub use desktop::*;
#[cfg(not(target_os = "android"))]
pub use sync_project::*;
#[cfg(target_os = "android")]
pub use stub::*;