//! Platform file service: open/save/import/export/share.
//!
//! Mobile cannot assume desktop filesystem semantics (content URIs, security
//! scoped URLs, share sheets). Everything crossing the boundary is
//! normalized here; the editor core only sees [`ImportedFile`] (bytes +
//! name + MIME) and never touches `content://` URIs, `UIDocumentPicker`,
//! security-scoped URLs, JNI, or UIKit.
//!
//! Native wiring status (honest): neither the Android Storage Access
//! Framework flow nor the iOS `UIDocumentPicker` flow is implemented yet —
//! both mobile services deny picker operations explicitly and the
//! capability flags stay false until on-device testing passes (§10–11).
//! Desktop drives `import_bytes` after its own file dialog.

use std::path::PathBuf;

/// File-boundary errors. Mobile stubs must return [`FileServiceError::Unsupported`]
/// for flows with no platform wiring yet — never silently fall back to
/// desktop `std::fs` semantics (content URIs and security-scoped URLs are
/// not raw paths). The eventual iOS implementation routes through
/// `UIDocumentPicker` / security-scoped URLs / share sheet with
/// temporary-cache copies; Android through the document picker + JNI.
#[derive(Debug)]
pub enum FileServiceError {
    /// No platform implementation exists yet for this operation.
    Unsupported(&'static str),
    /// The platform call was attempted and the OS returned an I/O error.
    Io(std::io::Error),
}

impl From<std::io::Error> for FileServiceError {
    fn from(err: std::io::Error) -> Self {
        FileServiceError::Io(err)
    }
}

/// What the editor receives from any platform picker: raw bytes plus the
/// suggested filename and MIME type. No paths, no URIs, no handles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedFile {
    pub bytes: Vec<u8>,
    pub name: String,
    pub mime: Option<String>,
}

/// What a picker needs to know before opening.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileImportRequest {
    /// Accepted extensions without dots (e.g. `["png", "svg"]`).
    /// Empty = anything the platform picker offers.
    pub extensions: Vec<String>,
    /// Suggested title for the picker UI.
    pub title: String,
}

/// Extension → MIME mapping shared by all platforms (pure, no I/O).
pub fn mime_for_name(name: &str) -> Option<&'static str> {
    match name.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "svg" => Some("image/svg+xml"),
        "json" => Some("application/json"),
        "mp4" => Some("video/mp4"),
        "webm" => Some("video/webm"),
        "mkv" => Some("video/x-matroska"),
        "mp3" => Some("audio/mpeg"),
        "wav" => Some("audio/wav"),
        "ogg" => Some("audio/ogg"),
        _ => None,
    }
}

/// Narrow file boundary. Desktop uses real paths; mobile normalizes platform
/// handles (content URIs, scoped URLs) before the editor ever sees them.
pub trait FileService: Send {
    /// Human label for diagnostics ("Desktop files", "Android storage", …).
    fn label(&self) -> &'static str;
    /// Whether `save_project` can write directly (desktop) or must go
    /// through a picker/share flow first (mobile).
    fn direct_save(&self) -> bool;
    /// Picker-driven import: resolve the user selection into bytes.
    /// Default denies — desktop drives `import_bytes` after its own file
    /// dialog, and mobile stubs deny explicitly until native pickers land.
    fn import(&mut self, request: &FileImportRequest) -> Result<ImportedFile, FileServiceError> {
        let _ = request;
        Err(FileServiceError::Unsupported(
            "no picker wired into FileService yet",
        ))
    }
    /// Picker/share-driven export. Default denies; desktop overrides with
    /// a direct write (its `direct_save` is true).
    fn export(&mut self, data: &[u8], filename: &str, mime: &str) -> Result<(), FileServiceError> {
        let _ = (data, filename, mime);
        Err(FileServiceError::Unsupported("export flow not wired yet"))
    }
    /// Normalize an inbound path into bytes + suggested name.
    /// Desktop path API (called after the desktop file dialog). Mobile
    /// services override to deny: a `content://` URI or scoped URL is not
    /// a filesystem path.
    fn import_bytes(&self, path: &PathBuf) -> Result<(Vec<u8>, String), FileServiceError> {
        let bytes = std::fs::read(path)?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("import")
            .to_string();
        Ok((bytes, name))
    }
}

/// Desktop: unrestricted filesystem.
#[derive(Debug, Default)]
pub struct DesktopFileService;

impl FileService for DesktopFileService {
    fn label(&self) -> &'static str {
        "Desktop files"
    }
    fn direct_save(&self) -> bool {
        true
    }
    fn export(&mut self, data: &[u8], filename: &str, mime: &str) -> Result<(), FileServiceError> {
        let _ = mime;
        std::fs::write(filename, data)?;
        Ok(())
    }
}

/// Android stub: document picker + share sheet live behind JNI here later.
/// Normalization rules (content URI → cache copy) belong in this impl, never
/// in editor code.
#[derive(Debug, Default)]
pub struct AndroidFileService;

impl FileService for AndroidFileService {
    fn label(&self) -> &'static str {
        "Android storage"
    }
    fn direct_save(&self) -> bool {
        false
    }
    fn import_bytes(&self, _path: &PathBuf) -> Result<(Vec<u8>, String), FileServiceError> {
        // Content URIs are not filesystem paths: refuse explicitly instead
        // of inheriting the desktop std::fs::read default.
        Err(FileServiceError::Unsupported(
            "Android document picker not wired yet",
        ))
    }
    fn import(&mut self, _request: &FileImportRequest) -> Result<ImportedFile, FileServiceError> {
        // Storage Access Framework (ACTION_OPEN_DOCUMENT) belongs here:
        // open the picker, resolve the content URI, return bytes + name.
        Err(FileServiceError::Unsupported(
            "Android document picker not wired yet",
        ))
    }
}

/// iOS stub: picker + share sheet + security-scoped URLs, same rules.
#[derive(Debug, Default)]
pub struct IosFileService;

impl FileService for IosFileService {
    fn label(&self) -> &'static str {
        "iOS storage"
    }
    fn direct_save(&self) -> bool {
        false
    }
    fn import_bytes(&self, _path: &PathBuf) -> Result<(Vec<u8>, String), FileServiceError> {
        // Security-scoped URLs need UIDocumentPicker resolution first:
        // refuse explicitly instead of inheriting desktop std::fs::read.
        Err(FileServiceError::Unsupported(
            "iOS document picker not wired yet",
        ))
    }
    fn import(&mut self, _request: &FileImportRequest) -> Result<ImportedFile, FileServiceError> {
        // UIDocumentPickerViewController(forOpeningContentTypes:) belongs
        // here: startAccessingSecurityScopedResource, read/copy bytes,
        // stopAccessingSecurityScopedResource, return bytes + name.
        Err(FileServiceError::Unsupported(
            "iOS document picker not wired yet",
        ))
    }
}

pub fn create_file_service() -> Box<dyn FileService> {
    #[cfg(target_os = "android")]
    {
        Box::new(AndroidFileService)
    }
    #[cfg(target_os = "ios")]
    {
        Box::new(IosFileService)
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        Box::new(DesktopFileService)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_service_shape() {
        let s = DesktopFileService;
        assert_eq!(s.label(), "Desktop files");
        assert!(s.direct_save());
    }

    #[test]
    fn mobile_services_require_picker_flow() {
        assert!(!AndroidFileService.direct_save());
        assert!(!IosFileService.direct_save());
    }

    #[test]
    fn mobile_import_refuses_explicitly() {
        let p = PathBuf::from("/tmp/whatever.bin");
        assert!(matches!(
            AndroidFileService.import_bytes(&p),
            Err(FileServiceError::Unsupported(_))
        ));
        assert!(matches!(
            IosFileService.import_bytes(&p),
            Err(FileServiceError::Unsupported(_))
        ));
        let req = FileImportRequest {
            extensions: vec!["png".into()],
            title: "Import".into(),
        };
        assert!(matches!(
            AndroidFileService.import(&req),
            Err(FileServiceError::Unsupported(_))
        ));
        assert!(matches!(
            IosFileService.import(&req),
            Err(FileServiceError::Unsupported(_))
        ));
        assert!(matches!(
            AndroidFileService.export(b"x", "a.png", "image/png"),
            Err(FileServiceError::Unsupported(_))
        ));
        assert!(matches!(
            IosFileService.export(b"x", "a.png", "image/png"),
            Err(FileServiceError::Unsupported(_))
        ));
    }

    #[test]
    fn desktop_export_roundtrip() {
        let dir = std::env::temp_dir();
        let p = dir.join(format!("vadadee-fsvc-exp-{}.bin", std::process::id()));
        let mut s = DesktopFileService;
        s.export(b"payload", p.to_str().unwrap(), "application/octet-stream")
            .unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"payload");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn mime_mapping_covers_editor_formats() {
        assert_eq!(mime_for_name("a.png"), Some("image/png"));
        assert_eq!(mime_for_name("a.JPG"), Some("image/jpeg"));
        assert_eq!(mime_for_name("a.svg"), Some("image/svg+xml"));
        assert_eq!(
            mime_for_name("a.vadadee-berry.json"),
            Some("application/json")
        );
        assert_eq!(mime_for_name("a.mp4"), Some("video/mp4"));
        assert_eq!(mime_for_name("noext"), None);
    }

    #[test]
    fn desktop_import_reads_disk() {
        let dir = std::env::temp_dir();
        let p = dir.join(format!("vadadee-fsvc-{}.bin", std::process::id()));
        std::fs::write(&p, b"hello").unwrap();
        let s = DesktopFileService;
        let (bytes, name) = s.import_bytes(&p).unwrap();
        assert_eq!(bytes, b"hello");
        assert!(name.contains("vadadee-fsvc-"));
        let _ = std::fs::remove_file(&p);
    }
}
