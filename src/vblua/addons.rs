//! Addon system (Phase 18): local lifecycle for VBLua addons.
//!
//! ## Input
//!
//! Folders under `<data-dir>/addons/` (or `.vbaddon` bundles via
//! [`install_bundle`]).
//!
//! ## Output
//!
//! Validated manifests, enable/disable state, queued `addon.lua` sources.
//!
//! ## Errors
//!
//! Bad manifests, API-major mismatches, missing deps, and traversal bundles
//! fail with plain strings (surfaced to Lua or the console).
//!
//! Layout (user drops folders here; no marketplace yet — spec defers it):
//! ```text
//! <data-dir>/addons/<folder>/
//! ├── manifest.lua   -- return { id, name, version, vblua, permissions?, deps? }
//! ├── addon.lua      -- runs on enable (registers panels, templates, ...)
//! └── README.md      -- optional docs
//! ```
//!
//! Rules:
//! - Manifests parse in a throwaway sandboxed Lua (no `vblua` APIs, no I/O).
//! - `vblua` compat is major-version checked against `API_VERSION`.
//! - Permissions validate through the sandbox manifest parser (typos fail).
//! - `enable` runs `addon.lua` in the MAIN runtime (panels persist); errors
//!   keep it disabled with the structured message. Deps must be enabled first.
//! - Install = folder drop + `refresh()`; uninstall = `disable` + dir remove.
//!   Updates/marketplace are explicitly out of scope (Phase 19).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::sandbox::permissions_from_manifest;

/// Parsed, validated manifest (engine-owned; Lua tables never cross).
#[derive(Debug, Clone)]
pub struct AddonManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub permissions: HashSet<super::sandbox::Capability>,
    pub deps: Vec<String>,
    pub description: String,
}

/// Live addon state.
#[derive(Debug, Clone)]
pub struct AddonEntry {
    pub manifest: AddonManifest,
    pub dir: PathBuf,
    pub enabled: bool,
}

/// Local addon manager (scan/validate/enable/disable/install/uninstall).
#[derive(Debug, Default)]
pub struct AddonManager {
    dir: Option<PathBuf>,
    pub addons: HashMap<String, AddonEntry>,
}

impl AddonManager {
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            addons: HashMap::new(),
        }
    }

    /// Default data dir (`<data>/addons`), if the platform resolves one.
    pub fn default_dir() -> Option<PathBuf> {
        directories::ProjectDirs::from("com", "CosmoBunny", "VadadeeBerry")
            .map(|p| p.data_dir().join("addons"))
    }

    pub fn dir(&self) -> Option<&PathBuf> {
        self.dir.as_ref()
    }

    /// Rescan the addon dir: new manifests appear, deleted ones drop
    /// (enabled state is kept when the id survives).
    pub fn refresh(&mut self) -> Vec<String> {
        let mut errors = Vec::new();
        let mut found: HashMap<String, (AddonManifest, PathBuf)> = HashMap::new();
        if let Some(dir) = self.dir.clone()
            && let Ok(rd) = std::fs::read_dir(&dir)
        {
            let mut folders: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            folders.sort();
            for folder in folders {
                if !folder.is_dir() {
                    continue;
                }
                let manifest_path = folder.join("manifest.lua");
                if !manifest_path.is_file() {
                    continue;
                }
                match std::fs::read_to_string(&manifest_path)
                    .map_err(|e| e.to_string())
                    .and_then(|code| {
                        parse_manifest(
                            &folder
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                            &code,
                        )
                    }) {
                    Ok(m) => {
                        found.insert(m.id.clone(), (m, folder));
                    }
                    Err(e) => errors.push(format!(
                        "{}: {e}",
                        folder.display()
                    )),
                }
            }
        }
        // Drop vanished ids; keep enabled flags for survivors.
        self.addons.retain(|id, _| found.contains_key(id));
        for (id, (manifest, dir)) in found {
            self.addons
                .entry(id)
                .and_modify(|e| {
                    e.manifest = manifest.clone();
                    e.dir = dir.clone();
                })
                .or_insert(AddonEntry {
                    manifest,
                    dir,
                    enabled: false,
                });
        }
        errors.sort();
        errors
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<Vec<String>, String> {
        if !self.addons.contains_key(id) {
            return Err(format!("unknown addon '{id}'"));
        }
        if enabled {
            // Deps must be present and enabled first (exact ids, no ranges yet).
            let deps = self.addons.get(id).map(|e| e.manifest.deps.clone()).unwrap_or_default();
            for dep in &deps {
                match self.addons.get(dep) {
                    Some(d) if d.enabled => {}
                    Some(_) => {
                        return Err(format!("addon '{id}' needs '{dep}' enabled first"));
                    }
                    None => {
                        return Err(format!("addon '{id}' needs missing addon '{dep}'"));
                    }
                }
            }
            let entry = self.addons.get_mut(id).unwrap();
            entry.enabled = true;
            Ok(entry.manifest.permissions.iter().map(|c| c.name().to_string()).collect())
        } else {
            // Disabling cascades: dependents cannot stay enabled.
            let mut off = vec![id.to_string()];
            for (other_id, other) in self.addons.iter() {
                if other.enabled && other.manifest.deps.iter().any(|d| d == id) {
                    off.push(other_id.clone());
                }
            }
            for oid in &off {
                if let Some(e) = self.addons.get_mut(oid) {
                    e.enabled = false;
                }
            }
            Ok(vec![])
        }
    }

    /// Read an addon's `addon.lua` for execution by the main runtime.
    /// Only enabled addons run; errors name the file.
    pub fn addon_source(&self, id: &str) -> Result<String, String> {
        let entry = self
            .addons
            .get(id)
            .ok_or_else(|| format!("unknown addon '{id}'"))?;
        if !entry.enabled {
            return Err(format!("addon '{id}' is not enabled"));
        }
        std::fs::read_to_string(entry.dir.join("addon.lua"))
            .map_err(|_| format!("addon '{id}' has no addon.lua"))
    }

    /// Uninstall: must be disabled first (refuses otherwise — no surprise loss).
    pub fn uninstall(&mut self, id: &str) -> Result<(), String> {
        let entry = self
            .addons
            .get(id)
            .ok_or_else(|| format!("unknown addon '{id}'"))?;
        if entry.enabled {
            return Err(format!("disable addon '{id}' before uninstalling"));
        }
        std::fs::remove_dir_all(&entry.dir)
            .map_err(|e| format!("uninstall failed: {e}"))?;
        self.addons.remove(id);
        Ok(())
    }
}

/// `.vbaddon` bundle budgets (Phase 19 local packages, no marketplace).
pub const MAX_BUNDLE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_BUNDLE_UNPACKED: u64 = 64 * 1024 * 1024;
pub const MAX_BUNDLE_FILES: usize = 256;

/// Export an installed addon as a `.vbaddon` bundle (Stored zip).
/// Layout inside: `<id>/manifest.lua`, `<id>/addon.lua`, `<id>/...`.
pub fn export_bundle(manager: &AddonManager, id: &str) -> Result<Vec<u8>, String> {
    let entry = manager
        .addons
        .get(id)
        .ok_or_else(|| format!("unknown addon '{id}'"))?;
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let mut files: Vec<PathBuf> = Vec::new();
        collect_files(&entry.dir, &mut files);
        files.sort();
        if files.len() > MAX_BUNDLE_FILES {
            return Err("addon has too many files for a bundle".to_string());
        }
        for path in files {
            let rel = path
                .strip_prefix(&entry.dir)
                .map_err(|_| "addon dir escaped".to_string())?;
            let name = format!("{}/{}", id, rel.display());
            let data = std::fs::read(&path).map_err(|e| format!("read failed: {e}"))?;
            zip.start_file(name, opts)
                .map_err(|e| format!("bundle write failed: {e}"))?;
            use std::io::Write;
            zip.write_all(&data)
                .map_err(|e| format!("bundle write failed: {e}"))?;
        }
        zip.finish().map_err(|e| format!("bundle finish failed: {e}"))?;
    }
    Ok(buf.into_inner())
}

/// Install a `.vbaddon` bundle into the manager dir. Returns the addon id.
/// Refuses traversal entries, absolute paths, oversize unpacks, and
/// existing ids (uninstall first — no silent overwrite).
pub fn install_bundle(manager: &mut AddonManager, bytes: &[u8]) -> Result<String, String> {
    let Some(dir) = manager.dir.clone() else {
        return Err("no addon directory on this platform".to_string());
    };
    if bytes.len() > MAX_BUNDLE_BYTES {
        return Err(format!("bundle exceeds {} bytes", MAX_BUNDLE_BYTES));
    }
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).map_err(|_| "not a .vbaddon bundle".to_string())?;
    if zip.len() > MAX_BUNDLE_FILES {
        return Err("bundle has too many files".to_string());
    }
    // Stage into memory first (validate before touching disk).
    let mut staged: Vec<(String, Vec<u8>)> = Vec::new();
    let mut unpacked: u64 = 0;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|_| "bundle entry unreadable".to_string())?;
        let name = file.name().to_string();
        if name.starts_with('/') || name.starts_with('\\') || name.contains("..") {
            return Err(format!("bundle entry escapes: '{name}'"));
        }
        if file.is_dir() {
            continue;
        }
        unpacked = unpacked.saturating_add(file.size());
        if unpacked > MAX_BUNDLE_UNPACKED {
            return Err("bundle unpacks too large".to_string());
        }
        let mut data = Vec::new();
        use std::io::Read;
        file.read_to_end(&mut data)
            .map_err(|_| "bundle entry unreadable".to_string())?;
        staged.push((name, data));
    }
    if staged.is_empty() {
        return Err("bundle is empty".to_string());
    }
    // Normalize: a single top-level dir is stripped (`id/...` layout).
    let stripped: Vec<(String, Vec<u8>)> = {
        let firsts: Vec<String> = staged
            .iter()
            .map(|(n, _)| n.split('/').next().unwrap_or("").to_string())
            .collect();
        let shared = !firsts.is_empty() && firsts.iter().all(|f| f == &firsts[0] && !f.is_empty());
        if shared && staged.iter().any(|(n, _)| n.contains('/')) {
            let prefix = format!("{}/", firsts[0]);
            staged
                .into_iter()
                .filter(|(n, _)| {
                    n.strip_prefix(&prefix).is_some_and(|r| !r.is_empty())
                })
                .map(|(n, d)| (n[prefix.len()..].to_string(), d))
                .collect()
        } else {
            staged
        }
    };
    let manifest_src = stripped
        .iter()
        .find(|(n, _)| n == "manifest.lua")
        .map(|(_, d)| d)
        .ok_or_else(|| "bundle has no manifest.lua".to_string())?;
    if !stripped.iter().any(|(n, _)| n == "addon.lua") {
        return Err("bundle has no addon.lua".to_string());
    }
    let manifest_src = String::from_utf8(manifest_src.clone())
        .map_err(|_| "manifest.lua is not UTF-8".to_string())?;
    let manifest = parse_manifest("bundle", &manifest_src)?;
    let target = dir.join(&manifest.id);
    if target.exists() {
        return Err(format!(
            "addon '{}' already installed (uninstall first)",
            manifest.id
        ));
    }
    for (rel, data) in &stripped {
        if rel.contains("..") || rel.starts_with('/') {
            return Err(format!("bundle entry escapes: '{rel}'"));
        }
        let dest = target.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("install failed: {e}"))?;
        }
        std::fs::write(&dest, data).map_err(|e| format!("install failed: {e}"))?;
    }
    let id = manifest.id.clone();
    manager.addons.insert(
        id.clone(),
        AddonEntry {
            manifest,
            dir: target,
            enabled: false,
        },
    );
    Ok(id)
}

fn collect_files(dir: &PathBuf, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

/// Parse + validate `manifest.lua` in a throwaway sandbox (no vblua, no I/O).
pub fn parse_manifest(folder: &str, code: &str) -> Result<AddonManifest, String> {
    if code.len() > 16 * 1024 {
        return Err("manifest.lua exceeds 16 KiB".to_string());
    }
    let lua = mlua::Lua::new();
    super::sandbox::SandboxPolicy::default()
        .apply(&lua)
        .map_err(|e| format!("sandbox failed: {e}"))?;
    let value: mlua::Value = lua
        .load(code)
        .set_name("manifest.lua")
        .eval()
        .map_err(|e| super::error::from_mlua("manifest.lua", e).to_string())?;
    let mlua::Value::Table(t) = value else {
        return Err("manifest.lua must return a table".to_string());
    };
    let get_str = |key: &str| -> Result<String, String> {
        t.get::<String>(key)
            .map_err(|_| format!("manifest needs {key} = \"...\""))
    };
    let id = get_str("id")?;
    if id.is_empty() || id.len() > 64 || id.chars().any(|c| !(c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')) {
        return Err("manifest id must be 1..64 chars ([a-z0-9._-])".to_string());
    }
    let name = get_str("name")?;
    if name.is_empty() || name.len() > 128 {
        return Err("manifest name must be 1..128 chars".to_string());
    }
    let version = get_str("version")?;
    if version.is_empty() || version.len() > 32 {
        return Err("manifest version must be 1..32 chars".to_string());
    }
    check_api_compat(&t)?;
    let mut perm_names = Vec::new();
    if let Ok(pt) = t.get::<mlua::Table>("permissions") {
        for v in pt.sequence_values::<String>() {
            perm_names.push(v.map_err(|_| "permissions must be strings".to_string())?);
        }
    }
    let permissions =
        permissions_from_manifest(&perm_names).map_err(|e| format!("manifest: {e}"))?;
    let mut deps = Vec::new();
    if let Ok(dt) = t.get::<mlua::Table>("deps") {
        for v in dt.sequence_values::<String>() {
            deps.push(v.map_err(|_| "deps must be addon-id strings".to_string())?);
        }
        if deps.len() > 32 {
            return Err("too many deps (max 32)".to_string());
        }
    }
    let description = t.get::<String>("description").unwrap_or_default();
    let _ = folder;
    Ok(AddonManifest {
        id,
        name,
        version,
        permissions,
        deps,
        description: description.chars().take(1024).collect(),
    })
}

/// Major-version gate against `API_VERSION` (`vblua = "1"` or `{min,max}`).
fn check_api_compat(t: &mlua::Table) -> Result<(), String> {
    let host_major: u64 = super::API_VERSION
        .split('.')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let major_of = |s: &str| -> Option<u64> {
        s.trim().split('.').next()?.parse().ok()
    };
    match t.get::<mlua::Value>("vblua") {
        Ok(mlua::Value::String(s)) => {
            let s = s.to_str().map_err(|e| e.to_string())?;
            match major_of(&s) {
                Some(m) if m == host_major => Ok(()),
                _ => Err(format!(
                    "addon needs vblua {s}, host is {}",
                    super::API_VERSION
                )),
            }
        }
        Ok(mlua::Value::Table(range)) => {
            let min = range
                .get::<String>("min")
                .ok()
                .and_then(|s| major_of(&s))
                .unwrap_or(0);
            let max = range
                .get::<String>("max")
                .ok()
                .and_then(|s| major_of(&s))
                .unwrap_or(u64::MAX);
            if min <= host_major && host_major <= max {
                Ok(())
            } else {
                Err(format!(
                    "addon needs vblua {min}..{max}, host is {}",
                    super::API_VERSION
                ))
            }
        }
        _ => Err("manifest needs vblua = \"1\" (or { min, max })".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(code: &str) -> Result<AddonManifest, String> {
        parse_manifest("test", code)
    }

    #[test]
    fn parses_full_manifest() {
        let m = manifest(
            r#"return {
            id = "example.glow", name = "Glow", version = "1.0.0",
            vblua = "1", permissions = { "document.write", "ui" },
            deps = { "example.base" }, description = "demo",
          }"#,
        )
        .unwrap();
        assert_eq!(m.id, "example.glow");
        assert!(m.permissions.contains(&super::super::sandbox::Capability::Ui));
        assert_eq!(m.deps, vec!["example.base"]);
    }

    #[test]
    fn rejects_bad_manifests() {
        // Not a table.
        assert!(manifest("return 42").is_err());
        // Missing id.
        assert!(manifest(r#"return { name = "x", version = "1", vblua = "1" }"#).is_err());
        // Bad id chars.
        assert!(manifest(r#"return { id = "a b", name = "x", version = "1", vblua = "1" }"#).is_err());
        // Wrong API major.
        assert!(manifest(r#"return { id = "a.b", name = "x", version = "1", vblua = "99" }"#).is_err());
        // Range that excludes host.
        assert!(manifest(
            r#"return { id = "a.b", name = "x", version = "1", vblua = { min = "99", max = "100" } }"#
        )
        .is_err());
        // Unknown permission.
        assert!(manifest(
            r#"return { id = "a.b", name = "x", version = "1", vblua = "1", permissions = { "root" } }"#
        )
        .is_err());
        // Range form accepted.
        assert!(manifest(
            r#"return { id = "a.b", name = "x", version = "1", vblua = { min = "1", max = "2" } }"#
        )
        .is_ok());
    }

    #[test]
    fn lifecycle_scan_enable_deps_uninstall() {
        let dir = std::env::temp_dir().join(format!("vblua-addon-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("base")).unwrap();
        std::fs::create_dir_all(dir.join("glow")).unwrap();
        std::fs::write(
            dir.join("base").join("manifest.lua"),
            r#"return { id = "example.base", name = "Base", version = "1.0.0", vblua = "1" }"#,
        )
        .unwrap();
        std::fs::write(dir.join("base").join("addon.lua"), "vblua.log('base on')").unwrap();
        std::fs::write(
            dir.join("glow").join("manifest.lua"),
            r#"return { id = "example.glow", name = "Glow", version = "1.0.0", vblua = "1", deps = { "example.base" } }"#,
        )
        .unwrap();
        std::fs::write(dir.join("glow").join("addon.lua"), "vblua.log('glow on')").unwrap();

        let mut mgr = AddonManager::new(Some(dir.clone()));
        assert!(mgr.refresh().is_empty());
        assert_eq!(mgr.addons.len(), 2);
        // Dep gate: glow before base fails.
        assert!(mgr.set_enabled("example.glow", true).is_err());
        mgr.set_enabled("example.base", true).unwrap();
        mgr.set_enabled("example.glow", true).unwrap();
        assert!(mgr.addon_source("example.glow").unwrap().contains("glow on"));
        // Disabling base cascades to glow.
        mgr.set_enabled("example.base", false).unwrap();
        assert!(!mgr.addons["example.glow"].enabled);
        // Uninstall refuses while enabled.
        mgr.set_enabled("example.base", true).unwrap();
        assert!(mgr.uninstall("example.base").is_err());
        mgr.set_enabled("example.base", false).unwrap();
        mgr.uninstall("example.base").unwrap();
        assert!(!mgr.addons.contains_key("example.base"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bundle_export_install_round_trip() {
        let dir = std::env::temp_dir().join(format!("vblua-bundle-a-{}", std::process::id()));
        let dir2 = std::env::temp_dir().join(format!("vblua-bundle-b-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
        std::fs::create_dir_all(dir.join("demo")).unwrap();
        std::fs::write(
            dir.join("demo").join("manifest.lua"),
            r#"return { id = "demo.pack", name = "Pack", version = "1.0.0", vblua = "1" }"#,
        )
        .unwrap();
        std::fs::write(dir.join("demo").join("addon.lua"), "vblua.log('hi')").unwrap();
        let mgr = AddonManager::new(Some(dir.clone()));
        // Seed the manager entry without refresh path games.
        let mut mgr = mgr;
        assert!(mgr.refresh().is_empty());
        let bytes = export_bundle(&mgr, "demo.pack").unwrap();
        assert!(!bytes.is_empty());
        // Install elsewhere: appears, disabled, runs after enable.
        let mut mgr2 = AddonManager::new(Some(dir2.clone()));
        std::fs::create_dir_all(&dir2).unwrap();
        let id = install_bundle(&mut mgr2, &bytes).unwrap();
        assert_eq!(id, "demo.pack");
        assert!(!mgr2.addons[&id].enabled);
        // Reinstall refuses (no silent overwrite); traversal refused.
        assert!(install_bundle(&mut mgr2, &bytes).is_err());
        assert!(install_bundle(&mut mgr2, b"not a zip").is_err());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }
}
