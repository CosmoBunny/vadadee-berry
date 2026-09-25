//! Platform abstraction for desktop / Android / iOS.
//!
//! UI code must query [`PlatformCapabilities`] and [`UiDeviceClass`] instead
//! of scattering `#[cfg(target_os = ...)]` through widgets. Concrete
//! OS integration (window setup, text input, pickers, share sheet) lives in
//! the per-platform submodules behind the narrow traits declared here.

pub mod capabilities;
pub mod clipboard;
pub mod file_service;
pub mod text_input;

pub use capabilities::{
    PlatformCapabilities, SafeAreaInsets, UiDeviceClass, UiLayout, UiMetrics, classify_device,
};
pub use clipboard::{ClipboardError, ClipboardImage, ClipboardService};
pub use file_service::{
    FileImportRequest, FileService, FileServiceError, ImportedFile, mime_for_name,
};
pub use text_input::{NativeTextInput, TextInputState};

/// Capabilities of the platform this binary was built for.
///
/// Determined once at startup from `cfg` — never from viewport size (that is
/// [`UiDeviceClass`], which can change as windows resize).
pub fn current_capabilities() -> PlatformCapabilities {
    #[cfg(target_os = "android")]
    {
        PlatformCapabilities::android()
    }
    #[cfg(all(target_os = "ios", not(target_os = "android")))]
    {
        PlatformCapabilities::ios()
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        PlatformCapabilities::desktop()
    }
}
