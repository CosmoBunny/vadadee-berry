//! `vblua-lsp` core (editor adapter over the VBLua API model).
//!
//! ## Input
//!
//! Open-document text + LSP requests (completion, hover, diagnostics).
//!
//! ## Output
//!
//! `lsp-types` responses. The stdio JSON-RPC loop lives in
//! `src/bin/vblua_lsp.rs`; this module owns everything testable.
//!
//! ## Errors
//!
//! Unknown documents/methods yield empty results, never panics. Syntax
//! failures become `Diagnostic`s (severity Error, best-effort line).
//!
//! ## Capabilities (honest subset)
//!
//! text sync (full), diagnostics, completion, hover. Definition,
//! references, rename, and formatting are NOT implemented — the server
//! advertises exactly what exists.

use std::collections::HashMap;

/// One completion row (shared by completion + hover: one source, no drift).
#[derive(Debug, Clone)]
pub struct ApiEntry {
    pub label: &'static str,
    pub kind: lsp_types::CompletionItemKind,
    pub detail: &'static str,
    pub doc: &'static str,
}

/// The VBLua API surface for editors. Structural entries (namespaces) come
/// first, then functions, kinds, and tracks.
pub const API_ENTRIES: &[ApiEntry] = &[
    ApiEntry { label: "vblua", kind: lsp_types::CompletionItemKind::MODULE, detail: "VBLua root", doc: "Embedded scripting API. See vblua.version(), vblua.document, vblua.graph." },
    ApiEntry { label: "version", kind: lsp_types::CompletionItemKind::FUNCTION, detail: "vblua.version() -> string", doc: "Implementation version (e.g. 0.1.0)." },
    ApiEntry { label: "api_version", kind: lsp_types::CompletionItemKind::FUNCTION, detail: "vblua.api_version() -> string", doc: "Script API major manifests pin (1)." },
    ApiEntry { label: "maturity", kind: lsp_types::CompletionItemKind::FUNCTION, detail: "vblua.maturity() -> string", doc: "Release channel (experimental)." },
    ApiEntry { label: "log", kind: lsp_types::CompletionItemKind::FUNCTION, detail: "vblua.log(x)", doc: "Script console line." },
    ApiEntry { label: "has", kind: lsp_types::CompletionItemKind::FUNCTION, detail: "vblua.has(cap) -> bool", doc: "Capability probe; unknown is false." },
    ApiEntry { label: "permissions", kind: lsp_types::CompletionItemKind::FUNCTION, detail: "vblua.permissions() -> [string]", doc: "Granted capability names." },
    ApiEntry { label: "document", kind: lsp_types::CompletionItemKind::MODULE, detail: "document API", doc: "current/rename/resize/set_layer_visible." },
    ApiEntry { label: "graph", kind: lsp_types::CompletionItemKind::MODULE, detail: "node graph API", doc: "create/get/connect/ports/params over NodeEditor layers." },
    ApiEntry { label: "batch", kind: lsp_types::CompletionItemKind::MODULE, detail: "bulk ops", doc: "create_nodes/keyframes/grid/circle." },
    ApiEntry { label: "node", kind: lsp_types::CompletionItemKind::MODULE, detail: "templates", doc: "define/spawn/drop." },
    ApiEntry { label: "animation", kind: lsp_types::CompletionItemKind::MODULE, detail: "keyframes", doc: "set_keyframe/sample/keyframes/remove_keyframe." },
    ApiEntry { label: "editor", kind: lsp_types::CompletionItemKind::MODULE, detail: "automation", doc: "transaction/rename/duplicate/delete nodes." },
    ApiEntry { label: "kinematics", kind: lsp_types::CompletionItemKind::MODULE, detail: "FK/IK", doc: "chain/fk/ik/ik2/damp/spring/look_at/orbit." },
    ApiEntry { label: "shader", kind: lsp_types::CompletionItemKind::MODULE, detail: "WGSL passes", doc: "validate/create/set_source/set_uniforms." },
    ApiEntry { label: "video", kind: lsp_types::CompletionItemKind::MODULE, detail: "clips", doc: "clips/move/trim/split (existing media)." },
    ApiEntry { label: "assets", kind: lsp_types::CompletionItemKind::MODULE, detail: "inventory", doc: "list/info (read-only handles)." },
    ApiEntry { label: "file", kind: lsp_types::CompletionItemKind::MODULE, detail: "picker", doc: "status/pick (platform hook)." },
    ApiEntry { label: "ui", kind: lsp_types::CompletionItemKind::MODULE, detail: "panels", doc: "panel/button/slider/checkbox." },
    ApiEntry { label: "events", kind: lsp_types::CompletionItemKind::MODULE, detail: "subscriptions", doc: "on/off (one-level dispatch)." },
    ApiEntry { label: "addons", kind: lsp_types::CompletionItemKind::MODULE, detail: "lifecycle", doc: "refresh/enable/disable/reload/uninstall." },
    ApiEntry { label: "debug", kind: lsp_types::CompletionItemKind::MODULE, detail: "tracer", doc: "trace/breakpoint (no locals)." },
    ApiEntry { label: "image", kind: lsp_types::CompletionItemKind::MODULE, detail: "GPU images", doc: "solid/analysis/apply (opaque handles)." },
    ApiEntry { label: "kernel", kind: lsp_types::CompletionItemKind::MODULE, detail: "WGSL kernels", doc: "create/info (validated, cached)." },
    ApiEntry { label: "Blur", kind: lsp_types::CompletionItemKind::CLASS, detail: "graph kind", doc: "Linear blur (in/out image, amount real)." },
    ApiEntry { label: "Brightness", kind: lsp_types::CompletionItemKind::CLASS, detail: "graph kind", doc: "Brightness effect node." },
    ApiEntry { label: "Value", kind: lsp_types::CompletionItemKind::CLASS, detail: "graph kind", doc: "Scalar source (out real; param value)." },
    ApiEntry { label: "Speed", kind: lsp_types::CompletionItemKind::CLASS, detail: "graph kind", doc: "Time remap: source offset = t * factor (factor >= 0)." },
    ApiEntry { label: "Reverse", kind: lsp_types::CompletionItemKind::CLASS, detail: "graph kind", doc: "Mirror around the player window; out-of-window blanks." },
    ApiEntry { label: "TimeOffset", kind: lsp_types::CompletionItemKind::CLASS, detail: "graph kind", doc: "Constant source-time shift; out-of-window blanks." },
    ApiEntry { label: "FreezeFrame", kind: lsp_types::CompletionItemKind::CLASS, detail: "graph kind", doc: "Hold one source timestamp." },
    ApiEntry { label: "TimeRemap", kind: lsp_types::CompletionItemKind::CLASS, detail: "graph kind", doc: "Piecewise-linear time curve (curve param); empty = identity." },
    ApiEntry { label: "Transform", kind: lsp_types::CompletionItemKind::CLASS, detail: "graph kind", doc: "Position + Scale + Rotation in one node." },
    ApiEntry { label: "Crop", kind: lsp_types::CompletionItemKind::CLASS, detail: "graph kind", doc: "Normalized-rect crop (preview UV + export bake)." },
    ApiEntry { label: "FlipHorizontal", kind: lsp_types::CompletionItemKind::CLASS, detail: "graph kind", doc: "Horizontal mirror." },
    ApiEntry { label: "FlipVertical", kind: lsp_types::CompletionItemKind::CLASS, detail: "graph kind", doc: "Vertical mirror." },
    ApiEntry { label: "ZoomVideo", kind: lsp_types::CompletionItemKind::CLASS, detail: "graph kind alias", doc: "Zoom on video chains (normalized center, factor >= 1)." },
    ApiEntry { label: "Frame", kind: lsp_types::CompletionItemKind::CLASS, detail: "graph kind", doc: "Current frame number source." },
    ApiEntry { label: "Time", kind: lsp_types::CompletionItemKind::CLASS, detail: "graph kind", doc: "Timeline time source." },
    ApiEntry { label: "rotation", kind: lsp_types::CompletionItemKind::PROPERTY, detail: "anim track", doc: "Rotation keyframe track." },
    ApiEntry { label: "pos_x", kind: lsp_types::CompletionItemKind::PROPERTY, detail: "anim track", doc: "X position keyframe track." },
    ApiEntry { label: "pos_y", kind: lsp_types::CompletionItemKind::PROPERTY, detail: "anim track", doc: "Y position keyframe track." },
    ApiEntry { label: "opacity", kind: lsp_types::CompletionItemKind::PROPERTY, detail: "anim track", doc: "Opacity keyframe track." },
];

/// Open-document store + providers. No I/O, no threads.
#[derive(Debug, Default)]
pub struct LspServer {
    docs: HashMap<String, String>,
}

impl LspServer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Server capabilities (exactly what exists — see module docs).
    pub fn capabilities() -> lsp_types::ServerCapabilities {
        lsp_types::ServerCapabilities {
            text_document_sync: Some(lsp_types::TextDocumentSyncCapability::Kind(
                lsp_types::TextDocumentSyncKind::FULL,
            )),
            completion_provider: Some(lsp_types::CompletionOptions {
                trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
                ..Default::default()
            }),
            hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
            ..Default::default()
        }
    }

    pub fn open(&mut self, uri: String, text: String) -> Vec<lsp_types::Diagnostic> {
        self.docs.insert(uri.clone(), text.clone());
        diagnose(&uri, &text)
    }

    pub fn change(&mut self, uri: &str, text: String) -> Vec<lsp_types::Diagnostic> {
        self.docs.insert(uri.to_string(), text.clone());
        diagnose(uri, &text)
    }

    pub fn close(&mut self, uri: &str) {
        self.docs.remove(uri);
    }

    /// Prefix-filtered completion over [`API_ENTRIES`].
    pub fn complete(&self, prefix: &str) -> Vec<lsp_types::CompletionItem> {
        API_ENTRIES
            .iter()
            .filter(|e| e.label.starts_with(prefix))
            .map(|e| lsp_types::CompletionItem {
                label: e.label.to_string(),
                kind: Some(e.kind.clone()),
                detail: Some(e.detail.to_string()),
                documentation: Some(lsp_types::Documentation::String(e.doc.to_string())),
                ..Default::default()
            })
            .collect()
    }

    /// Hover: exact word lookup in [`API_ENTRIES`].
    pub fn hover(&self, word: &str) -> Option<String> {
        API_ENTRIES.iter().find(|e| e.label == word).map(|e| {
            format!("**{}** — {}\n\n{}", e.label, e.detail, e.doc)
        })
    }

    /// Best-effort word before a (line, UTF-16-ish character) position.
    /// Splits trailing identifier runs on `.`/`:` so `vblua.gr|aph` → `gr`.
    pub fn word_at(&self, uri: &str, line: u32, character: u32) -> String {
        let Some(text) = self.docs.get(uri) else {
            return String::new();
        };
        let Some(line_text) = text.lines().nth(line as usize) else {
            return String::new();
        };
        let upto: String = line_text.chars().take(character as usize).collect();
        let run: String = upto
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.' || *c == ':')
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        run.split(['.', ':']).next_back().unwrap_or("").to_string()
    }
}

/// Syntax-check Lua WITHOUT running it (compile to a function, discard).
/// Bare interpreter: `vblua` globals are runtime-provided, not syntax.
pub fn diagnose(uri: &str, text: &str) -> Vec<lsp_types::Diagnostic> {
    if text.len() > 1_000_000 {
        return vec![lsp_types::Diagnostic {
            range: zero_range(0),
            severity: Some(lsp_types::DiagnosticSeverity::ERROR),
            code: Some(lsp_types::NumberOrString::String("vblua-too-large".to_string())),
            message: "script exceeds 1 MiB".to_string(),
            ..Default::default()
        }];
    }
    let lua = mlua::Lua::new();
    // Name the chunk after the document so diagnostics never leak host paths.
    let short = uri.rsplit('/').next().unwrap_or("editor.lua");
    match lua.load(text).set_name(short).into_function() {
        Ok(_) => vec![],
        Err(e) => vec![lsp_types::Diagnostic {
            range: zero_range(error_line(&e.to_string())),
            severity: Some(lsp_types::DiagnosticSeverity::ERROR),
            code: Some(lsp_types::NumberOrString::String("vblua-syntax".to_string())),
            message: short_message(&e.to_string()),
            ..Default::default()
        }],
    }
}

fn zero_range(line: u32) -> lsp_types::Range {
    lsp_types::Range {
        start: lsp_types::Position { line, character: 0 },
        end: lsp_types::Position { line, character: 0 },
    }
}

/// Best-effort line extraction from `...:LINE: ...` (0-based for LSP).
fn error_line(msg: &str) -> u32 {
    // mlua syntax errors look like `[string "doc"]:3: ...` (1-based).
    msg.rsplit_once(':')
        .and_then(|(head, _)| head.rsplit_once(':'))
        .and_then(|(_, n)| n.trim().parse::<u32>().ok())
        .map(|n| n.saturating_sub(1))
        .unwrap_or(0)
}

fn short_message(msg: &str) -> String {
    msg.chars().take(512).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_code_has_no_diagnostics() {
        assert!(diagnose("f.lua", "local x = 1\nreturn x").is_empty());
    }

    #[test]
    fn broken_code_points_at_the_line() {
        // `local y =` is incomplete; Lua flags the `return` on line 3 (1-based) → 2.
        let ds = diagnose("f.lua", "local x = 1\nlocal y = \nreturn x");
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].range.start.line, 2);
        assert!(ds[0].message.contains("f.lua"), "{}", ds[0].message);
        assert!(!ds[0].message.contains("src/vblua"), "{}", ds[0].message);
        assert!(ds[0].message.len() <= 512);
    }

    #[test]
    fn completion_and_hover_share_entries() {
        let s = LspServer::new();
        assert!(s.complete("Bl").iter().any(|c| c.label == "Blur"));
        assert!(s.hover("Blur").unwrap().contains("Linear blur"));
        assert!(s.hover("Nope").is_none());
        assert!(!s.complete("").is_empty());
    }

    #[test]
    fn completion_kinds_parse_as_graph_kinds() {
        // Anti-drift: every CLASS entry must be a real scriptable kind.
        for e in API_ENTRIES {
            if e.kind == lsp_types::CompletionItemKind::CLASS {
                assert!(
                    super::super::graph::parse_kind(e.label).is_ok(),
                    "{} not parseable",
                    e.label
                );
            }
        }
    }
}
