//! Cross-platform clipboard (text + image).
//!
//! Desktop goes through `arboard`; mobile has no clipboard wiring yet, so
//! the stub degrades gracefully (reads look empty, writes report
//! unsupported) instead of importing a desktop-only crate. Editor code only
//! sees [`ClipboardService`] — never `arboard`, `ClipboardManager`, or
//! `UIPasteboard`.

/// RGBA image for clipboard transfer (matches `arboard::ImageData` layout).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardImage {
    pub width: usize,
    pub height: usize,
    /// Row-major RGBA bytes (`width * height * 4`).
    pub bytes: Vec<u8>,
}

/// Clipboard failures. Reads degrade to "empty"; writes report the missing
/// platform integration explicitly.
#[derive(Debug)]
pub enum ClipboardError {
    /// No platform implementation exists yet for this operation.
    Unsupported(&'static str),
    /// The platform call was attempted and failed (busy, empty, denied).
    Unavailable(String),
}

impl std::fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClipboardError::Unsupported(msg) => write!(f, "unsupported: {msg}"),
            ClipboardError::Unavailable(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ClipboardError {}

/// Narrow clipboard boundary. One implementation per platform; editor code
/// only sees this trait.
pub trait ClipboardService: Send {
    /// Current text, or `None` when the clipboard holds no text (or no
    /// clipboard exists — mobile stub).
    fn get_text(&mut self) -> Result<Option<String>, ClipboardError>;
    /// Replace the clipboard text.
    fn set_text(&mut self, text: &str) -> Result<(), ClipboardError>;
    /// Current image, or `None` when absent.
    fn get_image(&mut self) -> Result<Option<ClipboardImage>, ClipboardError>;
    /// Replace the clipboard image.
    fn set_image(&mut self, image: ClipboardImage) -> Result<(), ClipboardError>;
}

/// Desktop: `arboard` (X11/Wayland/macOS/Windows native clipboards).
/// Created per call, mirroring the previous inline `Clipboard::new()` uses.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Debug, Default)]
pub struct DesktopClipboard;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl ClipboardService for DesktopClipboard {
    fn get_text(&mut self) -> Result<Option<String>, ClipboardError> {
        let mut cb =
            arboard::Clipboard::new().map_err(|e| ClipboardError::Unavailable(e.to_string()))?;
        cb.get_text()
            .map(Some)
            .map_err(|e| ClipboardError::Unavailable(e.to_string()))
    }

    fn set_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        let mut cb =
            arboard::Clipboard::new().map_err(|e| ClipboardError::Unavailable(e.to_string()))?;
        cb.set_text(text.to_owned())
            .map_err(|e| ClipboardError::Unavailable(e.to_string()))
    }

    fn get_image(&mut self) -> Result<Option<ClipboardImage>, ClipboardError> {
        let mut cb =
            arboard::Clipboard::new().map_err(|e| ClipboardError::Unavailable(e.to_string()))?;
        cb.get_image()
            .map(|img| {
                Some(ClipboardImage {
                    width: img.width,
                    height: img.height,
                    bytes: img.bytes.into_owned(),
                })
            })
            .map_err(|e| ClipboardError::Unavailable(e.to_string()))
    }

    fn set_image(&mut self, image: ClipboardImage) -> Result<(), ClipboardError> {
        let mut cb =
            arboard::Clipboard::new().map_err(|e| ClipboardError::Unavailable(e.to_string()))?;
        cb.set_image(arboard::ImageData {
            width: image.width,
            height: image.height,
            bytes: std::borrow::Cow::from(image.bytes),
        })
        .map_err(|e| ClipboardError::Unavailable(e.to_string()))
    }
}

/// Mobile stub: no `ClipboardManager` / `UIPasteboard` bridge exists yet.
/// Reads degrade to empty (callers already treat "nothing" as a no-op);
/// writes fail explicitly. Wiring the native bridges means filling in
/// these methods — no widget or editor changes required.
#[cfg(any(target_os = "android", target_os = "ios"))]
#[derive(Debug, Default)]
pub struct UnimplementedClipboard;

#[cfg(any(target_os = "android", target_os = "ios"))]
impl ClipboardService for UnimplementedClipboard {
    fn get_text(&mut self) -> Result<Option<String>, ClipboardError> {
        Ok(None)
    }

    fn set_text(&mut self, _text: &str) -> Result<(), ClipboardError> {
        Err(ClipboardError::Unsupported(
            "mobile clipboard not wired yet",
        ))
    }

    fn get_image(&mut self) -> Result<Option<ClipboardImage>, ClipboardError> {
        Ok(None)
    }

    fn set_image(&mut self, _image: ClipboardImage) -> Result<(), ClipboardError> {
        Err(ClipboardError::Unsupported(
            "mobile clipboard not wired yet",
        ))
    }
}

/// Build the right implementation for this binary.
pub fn create_clipboard() -> Box<dyn ClipboardService> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        Box::new(DesktopClipboard)
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        Box::new(UnimplementedClipboard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stub contract (mobile only): reads look empty, writes fail
    /// explicitly. Callers already treat "nothing" as a no-op.
    #[cfg(any(target_os = "android", target_os = "ios"))]
    #[test]
    fn mobile_stub_is_graceful() {
        let mut cb = UnimplementedClipboard;
        assert_eq!(cb.get_text().unwrap(), None);
        assert_eq!(cb.get_image().unwrap(), None);
        assert!(matches!(
            cb.set_text("x"),
            Err(ClipboardError::Unsupported(_))
        ));
        assert!(matches!(
            cb.set_image(ClipboardImage {
                width: 0,
                height: 0,
                bytes: vec![],
            }),
            Err(ClipboardError::Unsupported(_))
        ));
    }

    /// Factory builds on every platform without touching a real clipboard
    /// (headless CI has no display server; construction is side-effect free).
    #[test]
    fn factory_builds_everywhere() {
        let _ = create_clipboard();
    }

    #[test]
    fn clipboard_image_shape() {
        let img = ClipboardImage {
            width: 2,
            height: 1,
            bytes: vec![0; 8],
        };
        assert_eq!(img.bytes.len(), img.width * img.height * 4);
    }
}
