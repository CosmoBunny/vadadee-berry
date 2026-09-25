//! Platform file service: open/save/import/export/share.
//!
//! Mobile cannot assume desktop filesystem semantics (content URIs, security
//! scoped URLs, share sheets). Everything crossing the boundary is
//! normalized here; the editor core only sees stable local paths or bytes.

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

/// Narrow file boundary. Desktop uses real paths; mobile normalizes platform
/// handles (content URIs, scoped URLs) before the editor ever sees them.
pub trait FileService: Send {
    /// Human label for diagnostics ("Desktop files", "Android storage", …).
    fn label(&self) -> &'static str;
    /// Whether `save_project` can write directly (desktop) or must go
    /// through a picker/share flow first (mobile).
    fn direct_save(&self) -> bool;
    /// Normalize an inbound platform handle into bytes + suggested name.
    /// Default: read the path straight off disk (desktop).
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
