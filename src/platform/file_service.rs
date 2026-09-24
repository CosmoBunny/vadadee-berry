//! Platform file service: open/save/import/export/share.
//!
//! Mobile cannot assume desktop filesystem semantics (content URIs, security
//! scoped URLs, share sheets). Everything crossing the boundary is
//! normalized here; the editor core only sees stable local paths or bytes.

use std::path::PathBuf;

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
    fn import_bytes(&self, path: &PathBuf) -> std::io::Result<(Vec<u8>, String)> {
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
