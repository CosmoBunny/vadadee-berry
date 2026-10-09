//! # VBLua — Vadadee Berry's programmable application platform.
//!
//! VBLua is the embedded scripting layer of Vadadee Berry: Lua scripts drive
//! the document, node graph, animation, shaders, video, assets, and editor
//! through a deliberate, versioned, sandboxed API boundary. Rust owns memory,
//! rendering, GPU resources, and platform integration; Lua describes behavior.
//!
//! ```text
//! Lua → VBLua API → Rust abstraction → platform implementation
//! ```
//!
//! ## Ecosystem (actual vs planned)
//!
//! ```text
//!                  VBLua (today: module `vadadee_berry::vblua`)
//!                               │
//!                 ┌─────────────┼─────────────┐
//!                 ▼             ▼             ▼
//!         Vadadee Berry    (planned)      (planned)
//!         desktop/mobile   vblua CLI      vblua-lsp → editors
//! ```
//!
//! **Honest status (§33 rule: never claim what isn't built).** Today VBLua
//! ships as a module inside the `vadadee-berry` crate, embedding Lua 5.4 via
//! vendored `mlua` (no system Lua, no separate grammar crate). There is no
//! standalone `libvblua` crate, no `vblua` CLI binary, and no `vblua-lsp`
//! server yet. The intended extraction is:
//!
//! ```text
//! libvblua  = reusable language/runtime library (this module's future home)
//! vblua     = command-line interface over libvblua (planned)
//! vblua-lsp = LSP adapter over libvblua; editors NEVER load libvblua directly:
//!             Editor ──LSP/JSON-RPC/stdio──▶ vblua-lsp ──Rust API──▶ libvblua
//! ```
//!
//! ## Pipeline (what runs today)
//!
//! ```text
//! Lua source
//!   │  (Lua 5.4 grammar, vendored mlua — no separate parser/AST crate;
//!   │   a dedicated parser/semantic layer is planned, see below)
//!   ▼
//! `vblua.*` API boundary (this module: [`VbRuntime`], [`VbValue`])
//!   │  snapshots in (reads) / command batches out (writes)
//!   ▼
//! Rust project (`Document` + node store + timeline), one undo entry per store
//! ```
//!
//! Parsing answers *"is this syntactically valid?"* (today: the Lua 5.4
//! interpreter itself, errors as [`VbluaError::Syntax`]); semantic validation
//! answers *"does this program make sense?"* (today: call-time checks at the
//! API boundary — unknown ids, kinds, tracks, ports — as [`VbluaError::Api`]
//! or `Runtime`). A dedicated parser → AST → analyzer pipeline producing
//! structured diagnostics is **planned** (needed for `vblua-lsp`); until then
//! the LSP-relevant contract is: interpreter messages pass through untouched.
//!
//! ## Consumers
//!
//! - **Vadadee Berry** (implemented): dev console, extension panels, addons.
//! - **`vblua` CLI** (planned): run scripts headlessly against a project file.
//! - **`vblua-lsp`** (planned): completion/hover/diagnostics over the same
//!   API metadata the runtime uses (single source of truth —
//!   [`graph::parse_kind`], [`animation::TRACK_LABELS`], [`docs/vblua-api.md`](https://github.com/CosmoBunny/vadadee-berry/blob/main/docs/vblua-api.md)).
//! - **External applications** (planned): embed via the Rust crate API, never
//!   via unstable Rust ABI (no `cdylib` promise; a C ABI would be a separate
//!   deliberate artifact).
//!
//! ## Basic usage
//!
//! ```rust
//! use vadadee_berry::vblua::{SandboxPolicy, VbRuntime};
//!
//! // Create → execute → collect. The app works fine with no runtime at all.
//! let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
//! let out = rt.execute("hello.lua", r#"vblua.log("hi"); return 1 + 1"#).unwrap();
//! assert_eq!(out, "2");
//! ```
//!
//! With a project (document + graph + animation snapshots in, one batch out):
//!
//! ```rust,no_run
//! # use vadadee_berry::vblua::{SandboxPolicy, VbRuntime};
//! # use vadadee_berry::document::Document;
//! # let mut project = Document::new_empty_project();
//! let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
//! let out = rt.execute_with_project(
//!     "doc.lua",
//!     "local d = vblua.document.current() return d and d.name or 'none'",
//!     Some(&mut project),
//! ).unwrap();
//! ```
//!
//! ## Runtime isolation & ownership
//!
//! - One [`VbRuntime`] owns one Lua state; states are never shared between
//!   runtimes. `VbRuntime` is `!Sync` by construction (use one per thread).
//! - The runtime borrows the project only for the duration of
//!   `execute_with_project` (snapshot in, apply out). Lua never holds Rust
//!   references across calls — ids cross as strings, never pointers.
//! - Values live as long as the call that produced them; Lua callbacks
//!   (UI/widgets/events) are Rust-owned `mlua::Function` handles dropped
//!   with their panel.
//!
//! ## Diagnostics & source positions
//!
//! Every failure is a [`VbluaError`] (`Syntax/Runtime/Api/Permission/
//! Timeout/Resource/Internal`) rendered for UI via
//! [`VbluaError::render`]. Interpreter messages (including `script:line`
//! text) pass through verbatim. There is **no span/offset system yet** and
//! no UTF-16 conversion: positions for LSP (`libvblua` span → `vblua-lsp`
//! `Position` → editor) are **planned** alongside the parser. Do not claim
//! Unicode position correctness — it is not implemented.
//!
//! ## API metadata: single source of truth
//!
//! Runtime validation, capability gating, and human docs must not drift apart.
//! Canonical kind/track/port tables live in code ([`graph::parse_kind`],
//! [`animation::TRACK_LABELS`], [`animation::valid_track`]); the human
//! reference is `docs/vblua-api.md`; EmmyLua stubs for editors are a planned
//! generator output (today: hand-maintained examples only).
//!
//! ## Embedding guide (Rust application)
//!
//! ```text
//! Create runtime → Configure (policy/picker/addons dir) → Execute
//! → Collect result/errors → Reset or drop.
//! ```
//!
//! ```rust,no_run
//! # use vadadee_berry::vblua::{SandboxPolicy, VbRuntime};
//! // 1. Create with an explicit policy (deny by default: document.read only).
//! let mut policy = SandboxPolicy::default();
//! // 2. Configure: grant what YOUR host allows, wire YOUR picker.
//! // policy.grant(vadadee_berry::vblua::Capability::DocumentWrite);
//! // rt.set_picker(Some(my_picker_hook));
//! let mut rt = VbRuntime::new(policy).unwrap();
//! // 3. Execute; 4. collect.
//! match rt.execute("addon.lua", "return vblua.version()") {
//!     Ok(v) => println!("ok: {v}"),
//!     Err(e) => eprintln!("{}", e.render()),
//! }
//! // 5. Reset for a fresh interpreter, or drop. Pause/resume also available.
//! ```
//!
//! Host-provided behavior arrives two ways: [`PickerHook`] (platform file
//! picker → bytes; desktop wires `rfd`, mobile wires SAF/picker later) and
//! Lua callbacks (UI widgets, events) polled via
//! [`VbRuntime::poll_ui_callbacks`]. Argument conversion goes through
//! [`VbValue`] (numbers/strings/tables in; functions/threads/userdata
//! rejected); depth (64) and breadth (4096) caps bound hostile tables.
//!
//! ## Platform support
//!
//! `vblua` itself is platform-neutral (pure Rust + vendored Lua C, no
//! `rfd`/`arboard`/`tokio`/process/net; only app-private `std::fs` for addon
//! storage). It compiles for `aarch64-linux-android` (verified) and should
//! compile anywhere the host crate does. Platform behavior enters ONLY
//! through injected hooks (`PickerHook`, capabilities): desktop wires `rfd`,
//! Android/iOS report `vblua.has("file_picker") == false` until SAF/document
//! picker bridges land. An LSP server targets developer desktops; mobile apps
//! embed the runtime only — never an LSP server.
//!
//! ## Cargo features (actual)
//!
//! The host crate exposes `default = ["opencv"]` / `opencv` (system OpenCV
//! face detection). VBLua has **no features of its own**: `mlua`
//! (`lua54` + `vendored`) and `zip` (Stored-only `.vbaddon` bundles) are
//! unconditional so mobile builds never diverge.
//!
//! ## Stability
//!
//! Channel: `experimental` ([`VBLUA_MATURITY`]) — **no API stability is
//! promised**. `vblua.api_version()` (`"1"`) is the manifest major; addon
//! manifests pin it. Internal modules (`api`, `context`, `value`,
//! `runtime`, `sandbox`) are crate-visible intentionally: Vadadee Berry is
//! the first-party consumer, not a boundary to defend against.
//!
//! ## How do I…?
//!
//! - Parse VBLua? → No standalone parser yet (vendored Lua 5.4 parses at
//!   `execute`); a `Parser`/`Analyzer` API is planned for `libvblua`.
//! - Execute? → [`VbRuntime::execute`] / `execute_with_project`.
//! - Diagnostics? → [`VbluaError`] + [`VbluaError::render`].
//! - Register a host function? → `PickerHook` today; UI/event callbacks via
//!   `vblua.ui.*` / `vblua.events.*`; generic host-fn registration is planned.
//! - Node metadata? → [`graph::parse_kind`], `vblua.graph.*`, `vblua.node.*`.
//! - Start `vblua-lsp` / connect VS Code/Neovim/Zed? → **Planned.**
//!   Intended contract: `vblua-lsp` over stdio JSON-RPC; **never log to
//!   stdout** (stderr only); editors discover the binary via `PATH`/bundled
//!   server + workspace root; minimal Neovim sketch:
//!   `vim.lsp.config("vblua", { cmd = { "vblua-lsp" }, filetypes = { "vblua" } })`.
//! - Debug the LSP / logging? → Planned with the server (stderr, `--log-file`).
//! - Build? → `cargo check/test/doc` (this crate); mobile via the existing
//!   Android/iOS CI jobs.
//!
//! Status: Phases 1–19, 21–22 (compile), 24–28, 30–35 implemented (see
//! `docs/VBLUA_TODO.md`); native addons, iOS/device validation, full
//! debugger stepping, export automation, and `vblua-lsp` are future work.

mod api;
pub mod addons;
pub mod assets;
pub mod batch;
pub mod file;
pub mod gpu;
pub mod image;
pub mod kernel;
pub mod ui_widgets;
pub mod animation;
pub mod console;
pub mod editor;
pub mod events;
mod context;
mod error;
pub mod graph;
pub mod kinematics;
pub mod lsp;
mod runtime;
mod sandbox;
pub mod scripted;
pub mod shader;
pub mod timeline_eval;
mod value;
pub mod video;

pub use api::ApiHost;pub use animation::{AnimCommand, AnimSnapshot};
pub use assets::AssetSnapshot;
pub use file::{FileCommand, PickFilter, PickedFile, PickerHook};
pub use console::VbluaConsole;
pub use graph::{GraphCommand, GraphSnapshot, ParamValue};
pub use shader::{ShaderCommand, ShaderSnapshot};
pub use video::{VideoCommand, VideoSnapshot};
pub use context::{DocumentCommand, DocumentSnapshot, LayerInfo};
pub use error::{VbluaError, from_mlua};
pub use runtime::{Lifecycle, VbRuntime};
pub use sandbox::{Capability, SandboxPolicy};
pub use value::VbValue;

/// VBLua implementation version (the `vblua.version()` string).
pub const VBLUA_VERSION: &str = "0.1.0";
/// Script-facing API version (`vblua.api_version()`, manifests pin this).
pub const API_VERSION: &str = "1";
/// Maturity channel (Phase 35): `experimental` → `preview` → `stable`.
/// While experimental, NO API stability is promised: minor versions may
/// rename or remove `vblua.*` functions (manifests pin majors only).
pub const VBLUA_MATURITY: &str = "experimental";
