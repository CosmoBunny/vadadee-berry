//! VBLua runtime lifecycle (Phase 2):
//!
//! ## Input
//!
//! Lua source text + an optional project (`execute_with_project`).
//!
//! ## Output
//!
//! Script result string, queued command batches applied to the project,
//! console lines, and a [`NodeReport`] for precise undo.
//!
//! ## Errors
//!
//! Every failure is a [`VbluaError`](super::error::VbluaError) — never a panic.
//!
//! ## Example
//!
//! ```rust
//! use vadadee_berry::vblua::{SandboxPolicy, VbRuntime};
//! let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
//! assert_eq!(rt.execute("x.lua", "return 40 + 2").unwrap(), "42");
//! ```
//! `Create -> Initialize -> Load -> Execute -> Pause/Resume -> Reset -> Shutdown`.
//!
//! The app stays fully functional with no script: the runtime is created
//! lazily, and every failure surfaces as [`VbluaError`] — never a panic, never
//! a host crash. Mutations apply as one batch after the script returns, so one
//! script run can become one undo transaction (Phase 16).

use std::sync::{Arc, Mutex};

use super::api::{register, ApiHost};
use super::context::{DocumentCommand, DocumentSnapshot};
use super::error::{VbluaError, from_mlua};
use super::sandbox::SandboxPolicy;
use crate::document::{AnimationTimeline, Document, ProjectFile};

/// Lifecycle state. `Paused` blocks `execute`; `Shutdown` drops the Lua state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lifecycle {
    #[default]
    Created,
    Ready,
    Paused,
    Shutdown,
}

/// Embedded Lua runtime. Owns the interpreter; borrows the [`Document`] only
/// for the duration of `execute_with_document` (snapshot in, commands out).
/// What a node-command batch created/removed (precise undo ingredients).
#[derive(Debug, Default)]
pub struct NodeReport {
    pub created_nodes: Vec<crate::document::Node>,
    pub removed: Vec<super::editor::RemovedNode>,
}

pub struct VbRuntime {
    lua: Option<mlua::Lua>,
    host: Arc<Mutex<ApiHost>>,
    policy: SandboxPolicy,
    state: Lifecycle,
    /// Script console (`vblua.log` + Lua `print`), drained by the UI.
    pub console: Vec<String>,
    last_report: NodeReport,
    /// Unified hook state: budget (reset per run), tracer, breakpoints.
    hook: std::sync::Arc<super::sandbox::HookState>,
}

/// One batch across every subsystem.
pub(crate) struct Drained {
    commands: Vec<DocumentCommand>,
    graph_commands: Vec<super::graph::GraphCommand>,
    anim_commands: Vec<super::animation::AnimCommand>,
    shader_commands: Vec<super::shader::ShaderCommand>,
    video_commands: Vec<super::video::VideoCommand>,
    file_commands: Vec<super::file::FileCommand>,
    node_commands: Vec<super::editor::NodeCommand>,
}

impl VbRuntime {
    /// Create + initialize (fresh interpreter, API registered, sandbox armed).
    pub fn new(policy: SandboxPolicy) -> Result<Self, VbluaError> {
        let host = Arc::new(Mutex::new(ApiHost {
            snapshot: DocumentSnapshot::default(),
            has_document: false,
            commands: Vec::new(),
            graphs: super::graph::GraphSnapshot::default(),
            graph_commands: Vec::new(),
            graph_kinds: std::collections::HashMap::new(),
            anim: super::animation::AnimSnapshot::default(),
            anim_commands: Vec::new(),
            shaders: super::shader::ShaderSnapshot::default(),
            shader_commands: Vec::new(),
            video: super::video::VideoSnapshot::default(),
            video_commands: Vec::new(),
            assets: super::assets::AssetSnapshot::default(),
            picker: None,
            has_nodestore: false,
            file_commands: Vec::new(),
            templates: super::scripted::TemplateRegistry::default(),
            addons: super::addons::AddonManager::new(super::addons::AddonManager::default_dir()),
            pending_addons: Vec::new(),
            editor: super::editor::EditorSnapshot::default(),
            node_commands: Vec::new(),
            transactional: false,
            rollback_requested: false,
            events: super::events::EventBus::default(),
            images: std::collections::HashMap::new(),
            image_next: 0,
            kernels: std::collections::HashMap::new(),
            kernel_next: 0,
            asset_bytes: std::collections::HashMap::new(),
            hook_state: None,
            ui: super::ui_widgets::UiRegistry::default(),
            ui_callbacks: std::collections::HashMap::new(),
            ui_events: Vec::new(),
            shader_sources: std::collections::HashMap::new(),
            policy: policy.clone(),
            console: Vec::new(),
            playback_frame: 0,
            playback_fps: 60,
            timeline_touched: false,
            timeline_declared: None,
        }));
        let mut rt = Self {
            lua: None,
            last_report: NodeReport::default(),
            hook: std::sync::Arc::new(super::sandbox::HookState::with_budget(policy.max_instructions)),
            host,
            policy,
            state: Lifecycle::Created,
            console: vec![format!(
                "VBLua {} (Lua 5.4) ready.",
                super::VBLUA_VERSION
            )],
        };
        rt.reset()?;
        Ok(rt)
    }

    pub fn state(&self) -> Lifecycle {
        self.state
    }

    /// Install the platform picker hook (desktop rfd, mobile SAF/picker).
    /// Survives `reset()`; `None` restores the unwired state.
    pub fn set_picker(&self, picker: Option<super::file::PickerHook>) {
        self.host.lock().unwrap().picker = picker;
    }

    /// Set the read-only playback position scripts observe via
    /// `vblua.timeline.time/frame/fps`. Frames are canonical (fps=0 → 60).
    pub fn set_playback(&self, frame: usize, fps: u32) {
        let mut h = self.host.lock().unwrap();
        h.playback_frame = frame as u64;
        h.playback_fps = fps.max(1);
    }

    /// Drain the timeline-eval marks left by one script run
    /// (`(touched_time, declared_range)`).
    pub fn take_timeline_marks(&self) -> (bool, Option<(f64, f64, bool)>) {
        let mut h = self.host.lock().unwrap();
        let touched = h.timeline_touched;
        let declared = h.timeline_declared;
        h.timeline_touched = false;
        h.timeline_declared = None;
        (touched, declared)
    }

    /// Whether a platform picker is wired.
    pub fn has_picker(&self) -> bool {
        self.host.lock().unwrap().picker.is_some()
    }

    /// Point the addon manager at a directory (tests / portable installs).
    pub fn set_addons_dir(&self, dir: Option<std::path::PathBuf>) {
        self.host.lock().unwrap().addons = super::addons::AddonManager::new(dir);
    }

    /// Number of declared extension panels (host decides whether to draw).
    pub fn ui_panel_count(&self) -> usize {
        self.host.lock().unwrap().ui.panels.len()
    }

    /// Cloned panel descriptors for the renderer (no host lock held).
    pub fn host_panels_snapshot(&self) -> Vec<super::ui_widgets::UiPanel> {
        let h = self.host.lock().unwrap();
        let mut ids: Vec<_> = h.ui.panels.keys().cloned().collect();
        ids.sort();
        ids.into_iter()
            .filter_map(|id| h.ui.panels.get(&id).cloned())
            .collect()
    }

    /// Update a slider's live value (renderer write-back).
    pub fn ui_set_num(&self, widget_id: u64, value: f64) {
        let mut h = self.host.lock().unwrap();
        for panel in h.ui.panels.values_mut() {
            if let Some(w) = panel.widgets.iter_mut().find(|w| w.id == widget_id) {
                w.num_value = value;
            }
        }
    }

    /// Update a checkbox's live value (renderer write-back).
    pub fn ui_set_bool(&self, widget_id: u64, value: bool) {
        let mut h = self.host.lock().unwrap();
        for panel in h.ui.panels.values_mut() {
            if let Some(w) = panel.widgets.iter_mut().find(|w| w.id == widget_id) {
                w.bool_value = value;
            }
        }
    }

    /// Queue a renderer event (button click, slider/checkbox change).
    pub fn ui_push_event(&self, ev: super::ui_widgets::UiEvent) {
        let mut h = self.host.lock().unwrap();
        if h.ui_events.len() < 1024 {
            h.ui_events.push(ev);
        }
    }

    /// Run addon sources queued by `enable()` (fresh snapshots already in).
    /// Failures disable the addon and land in the console — never the host.
    fn run_pending_addons(&mut self) {
        let pending = {
            let mut h = self.host.lock().unwrap();
            std::mem::take(&mut h.pending_addons)
        };
        if pending.is_empty() {
            return;
        }
        let Some(lua) = &self.lua else { return };
        let outcomes: Vec<(String, Result<(), String>)> = pending
            .into_iter()
            .map(|(id, source)| {
                let r = lua
                    .load(&source)
                    .set_name(format!("{id}/addon.lua"))
                    .eval::<mlua::Value>()
                    .map(|_| ())
                    .map_err(|e| from_mlua(&format!("{id}/addon.lua"), e).to_string());
                (id, r)
            })
            .collect();
        for (id, r) in outcomes {
            match r {
                Ok(()) => {
                    self.console.push(format!("addons: '{id}' running"));
                    self.dispatch_events(vec![super::events::FiredEvent::AddonEnabled(id)]);
                }
                Err(msg) => {
                    self.console.push(format!("addons: '{id}' failed: {msg}"));
                    self.host.lock().unwrap().addons.set_enabled(&id, false).ok();
                }
            }
        }
        self.flush_console();
    }
    /// Fire a host lifecycle event (`document-open`, `before-export`).
    /// Syncs snapshots, dispatches subscribers once, applies their work.
    /// Unknown events are refused (subscribe list is the contract).
    pub fn fire_app_event(
        &mut self,
        project: &mut ProjectFile,
        event: &str,
    ) -> Result<(), VbluaError> {
        if !super::events::KNOWN_EVENTS.contains(&event) {
            return Err(VbluaError::Api {
                api: "events".into(),
                message: format!("unknown event '{event}'"),
            });
        }
        self.hook.reset_budget();
        self.sync_host_for_project(project);
        self.dispatch_events(vec![super::events::FiredEvent::App(event.to_string())]);
        let queues = self.take_queues();
        self.last_report = Self::apply_drained(project, queues);
        Ok(())
    }

    /// Re-sync snapshots from the project (so callbacks see current state).
    fn sync_host_for_project(&self, p: &ProjectFile) {
        let mut h = self.host.lock().unwrap();
        h.snapshot = DocumentSnapshot::capture(&p.document);
        h.has_document = true;
        h.graphs = super::graph::GraphSnapshot::capture(&p.document);
        h.graph_kinds.clear();
        for layer in &p.document.layers {
            if let Some(g) = layer.node_graph.as_ref() {
                for (id, node) in &g.nodes {
                    h.graph_kinds.insert(id.to_string(), node.kind.clone());
                }
            }
        }
        h.anim = super::animation::AnimSnapshot::capture(p);
        h.shaders = super::shader::ShaderSnapshot::capture(&p.document);
        h.video = super::video::VideoSnapshot::capture(&p.document);
        h.assets = super::assets::AssetSnapshot::capture(p);
        h.has_nodestore = true;
    }

    /// Run pending widget callbacks, then apply any queued commands.
    /// Callback errors become console lines — never host panics.
    pub fn poll_ui_callbacks(&mut self, project: Option<&mut ProjectFile>) {
        if let Some(p) = project.as_deref() {
            self.sync_host_for_project(p);
        }
        self.hook.reset_budget();
        let events = {
            let mut h = self.host.lock().unwrap();
            std::mem::take(&mut h.ui_events)
        };
        if events.is_empty() {
            self.flush_console();
            return;
        }
        let _ = &self.lua;
        for ev in events {
            let (key, arg) = {
                let h = self.host.lock().unwrap();
                let (wid, arg) = match &ev {
                    super::ui_widgets::UiEvent::Clicked(id) => (*id, None),
                    super::ui_widgets::UiEvent::NumChanged(id, v) => (*id, Some(*v)),
                    super::ui_widgets::UiEvent::BoolChanged(id, b) => {
                        (*id, Some(if *b { 1.0 } else { 0.0 }))
                    }
                };
                (h.ui_callbacks.get(&wid).cloned(), arg)
            };
            let Some(f) = key else { continue };
            let r = match arg {
                Some(v) => f.call::<()>(v),
                None => f.call::<()>(()),
            };
            if let Err(e) = r {
                self.console.push(format!("ui callback error: {e}"));
            }
        }
        self.run_pending_addons();
        self.flush_console();
        // Settle: apply callback work, report, fire events once.
        let ok: Result<String, VbluaError> = Ok(String::new());
        self.settle(project, &ok);
    }

    pub fn pause(&mut self) {
        if self.state == Lifecycle::Ready {
            self.state = Lifecycle::Paused;
        }
    }

    pub fn resume(&mut self) {
        if self.state == Lifecycle::Paused {
            self.state = Lifecycle::Ready;
        }
    }

    /// Drop the interpreter and build a fresh one (clears globals + console).
    pub fn reset(&mut self) -> Result<(), VbluaError> {
        let lua = mlua::Lua::new();
        self.hook = super::sandbox::arm_execution_limits(&lua, &self.policy);
        self.host.lock().unwrap().hook_state = Some(self.hook.clone());
        self.policy
            .apply(&lua)
            .map_err(|e| VbluaError::Internal(format!("sandbox setup failed: {e}")))?;
        register(&lua, self.host.clone())
            .map_err(|e| VbluaError::Internal(format!("API registration failed: {e}")))?;
        self.install_print_hook(&lua);
        self.lua = Some(lua);
        {
            let mut h = self.host.lock().unwrap();
            h.commands.clear();
            h.console.clear();
            h.file_commands.clear();
        }
        self.state = Lifecycle::Ready;
        Ok(())
    }

    /// Permanently release the interpreter. Any later `execute` fails cleanly.
    pub fn shutdown(&mut self) {
        self.lua = None;
        self.state = Lifecycle::Shutdown;
    }

    /// `print(...)` routes to the script console (capped), like `vblua.log`.
    fn install_print_hook(&self, lua: &mlua::Lua) {
        let host = self.host.clone();
        let print_fn = lua.create_function(move |_, args: mlua::MultiValue| {
            let mut parts = Vec::new();
            for a in args {
                parts.push(match a {
                    mlua::Value::String(s) => {
                        s.to_str().map(|s| s.to_string()).unwrap_or_default()
                    }
                    mlua::Value::Integer(i) => i.to_string(),
                    mlua::Value::Number(n) => n.to_string(),
                    mlua::Value::Boolean(b) => b.to_string(),
                    mlua::Value::Nil => "nil".into(),
                    other => format!("{other:?}"),
                });
            }
            let line = parts.join("\t");
            let mut h = host.lock().unwrap();
            h.console.push(line.chars().take(4096).collect());
            if h.console.len() > 512 {
                let drain = h.console.len() - 512;
                h.console.drain(..drain);
            }
            Ok(())
        });
        if let Ok(f) = print_fn {
            let _ = lua.globals().set("print", f);
        }
    }

    /// Move host console lines into the runtime console.
    fn flush_console(&mut self) {
        let lines = {
            let mut h = self.host.lock().unwrap();
            std::mem::take(&mut h.console)
        };
        self.console.extend(lines);
        if self.console.len() > 1024 {
            let drain = self.console.len() - 1024;
            self.console.drain(..drain);
        }
    }

    /// Execute a script with no document (pure `vblua.*` + math).
    pub fn execute(&mut self, script: &str, code: &str) -> Result<String, VbluaError> {
        self.execute_with_project(script, code, None)
    }

    /// Execute with a document snapshot; queued commands apply to the document.
    /// Animation snapshot is empty (use [`Self::execute_with_project`] for it).
    /// Returns the script's converted result (`"nil"` for no value).
    pub fn execute_with_document(
        &mut self,
        script: &str,
        code: &str,
        mut doc: Option<&mut Document>,
    ) -> Result<String, VbluaError> {
        // Project-less path: no animation snapshot, no timeline apply.
        let result = self.execute_inner(script, code, doc.as_deref_mut(), None)?;
        Ok(result)
    }

    /// Execute with a full project: document + graph + animation snapshots in,
    /// all three command queues applied after. This is the primary entry point.
    pub fn execute_with_project(
        &mut self,
        script: &str,
        code: &str,
        project: Option<&mut ProjectFile>,
    ) -> Result<String, VbluaError> {
        self.check_runnable(script, code)?;
        // Snapshot in (immutable borrow ends before eval/apply).
        {
            let mut h = self.host.lock().unwrap();
            h.commands.clear();
            h.graph_commands.clear();
            h.anim_commands.clear();
            h.shader_commands.clear();
            h.video_commands.clear();
            h.shader_sources.clear();
            h.file_commands.clear();
            h.node_commands.clear();
            h.transactional = false;
            h.rollback_requested = false;
            h.timeline_touched = false;
            h.timeline_declared = None;
            match project.as_deref() {
                Some(p) => {
                    h.snapshot = DocumentSnapshot::capture(&p.document);
                    h.has_document = true;
                    h.graphs = super::graph::GraphSnapshot::capture(&p.document);
                    h.graph_kinds.clear();
                    for layer in &p.document.layers {
                        if let Some(g) = layer.node_graph.as_ref() {
                            for (id, node) in &g.nodes {
                                h.graph_kinds.insert(id.to_string(), node.kind.clone());
                            }
                        }
                    }
                    h.anim = super::animation::AnimSnapshot::capture(p);
                    h.shaders = super::shader::ShaderSnapshot::capture(&p.document);
                    h.video = super::video::VideoSnapshot::capture(&p.document);
                    h.assets = super::assets::AssetSnapshot::capture(p);
                    h.has_nodestore = true;
                    // Bounded asset-bytes cache for image.from_asset (32 MiB total).
                    h.asset_bytes.clear();
                    let mut budgeted: usize = 0;
                    for (id, node) in &p.nodes.map {
                        if budgeted >= 32 * 1024 * 1024 {
                            break;
                        }
                        if let crate::document::NodeKind::Image { bytes, .. } = &node.kind
                            && bytes.len() <= 4 * 1024 * 1024
                        {
                            budgeted += bytes.len();
                            h.asset_bytes.insert(format!("node:{id}"), bytes.clone());
                        }
                    }
                    h.editor = super::editor::EditorSnapshot::capture(&p.document, &p.nodes);
                    for (id, node) in &p.nodes.map {
                        if let Some(row) = h.editor.nodes.get_mut(&id.to_string()) {
                            row.name = node.name.clone();
                        }
                    }
                    for (id, node) in &p.nodes.map {
                        if let Some(row) = h.editor.nodes.get_mut(&id.to_string()) {
                            row.name = node.name.clone();
                        }
                    }
                }
                None => {
                    h.snapshot = DocumentSnapshot::default();
                    h.has_document = false;
                    h.graphs = super::graph::GraphSnapshot::default();
                    h.graph_kinds.clear();
                    h.anim = super::animation::AnimSnapshot::default();
                    h.shaders = super::shader::ShaderSnapshot::default();
                    h.video = super::video::VideoSnapshot::default();
                    h.assets = super::assets::AssetSnapshot::default();
                    h.has_nodestore = false;
                    h.file_commands.clear();
                }
            }
        }
        let result = self.eval_chunk(script, code);
        // Addon sources queued by enable() run now (fresh snapshots in).
        if result.is_ok() {
            self.run_pending_addons();
        }
        self.settle(project, &result);
        result
    }

    /// Settle one batch: apply (or roll back), report, fire events once.
    /// Event callbacks may queue a SECOND batch (applied, reported, undone)
    /// but never fire further events — one level, no infinite loops.
    fn settle(
        &mut self,
        project: Option<&mut ProjectFile>,
        result: &Result<String, VbluaError>,
    ) {
        // Transaction rollback: drop everything, but still report the error event.
        let rollback = result.is_err() && self.host.lock().unwrap().rollback_requested;
        let queues = self.take_queues();
        if rollback {
            self.dispatch_events(vec![super::events::FiredEvent::ScriptError(
                result.as_ref().unwrap_err().to_string(),
            )]);
            return;
        }
        let Some(p) = project else { return };
        self.last_report = Self::apply_drained(p, queues);
        let mut evs = Vec::new();
        if let Err(e) = result {
            evs.push(super::events::FiredEvent::ScriptError(e.to_string()));
        }
        if !self.last_report.created_nodes.is_empty() {
            evs.push(super::events::FiredEvent::NodesCreated(
                self.last_report
                    .created_nodes
                    .iter()
                    .map(|n| n.id.to_string())
                    .collect(),
            ));
        }
        if !self.last_report.removed.is_empty() {
            evs.push(super::events::FiredEvent::NodesDeleted(
                self.last_report
                    .removed
                    .iter()
                    .map(|r| r.node.id.to_string())
                    .collect(),
            ));
        }
        evs.push(super::events::FiredEvent::AfterRun);
        self.dispatch_events(evs);
        // Second (and final) drain for event-callback work.
        let more = self.take_queues();
        let extra = Self::apply_drained(p, more);
        self.last_report.created_nodes.extend(extra.created_nodes);
        self.last_report.removed.extend(extra.removed);
    }

    /// Call subscriber functions for fired events (errors become console lines).
    fn dispatch_events(&mut self, events: Vec<super::events::FiredEvent>) {
        if events.is_empty() {
            return;
        }
        let Some(lua) = &self.lua else { return };
        let mut calls: Vec<(mlua::Function, mlua::Value)> = Vec::new();
        {
            let h = self.host.lock().unwrap();
            for ev in &events {
                let fns = h
                    .events
                    .subscribers
                    .get(ev.name().as_str())
                    .cloned()
                    .unwrap_or_default();
                for f in fns {
                    let arg: mlua::Result<mlua::Value> = match ev {
                        super::events::FiredEvent::AfterRun => Ok(mlua::Value::Nil),
                        super::events::FiredEvent::NodesCreated(ids)
                        | super::events::FiredEvent::NodesDeleted(ids) => {
                            match lua.create_table() {
                                Ok(t) => {
                                    let mut ok = true;
                                    for (i, id) in ids.iter().enumerate() {
                                        if t.set(i + 1, id.clone()).is_err() {
                                            ok = false;
                                        }
                                    }
                                    if ok {
                                        Ok(mlua::Value::Table(t))
                                    } else {
                                        Err(mlua::Error::external("event table build failed"))
                                    }
                                }
                                Err(e) => Err(e),
                            }
                        }
                        super::events::FiredEvent::AddonEnabled(id)
                        | super::events::FiredEvent::ScriptError(id)
                        | super::events::FiredEvent::App(id) => {
                            match lua.create_string(id) {
                                Ok(st) => Ok(mlua::Value::String(st)),
                                Err(e) => Err(e),
                            }
                        }
                    };
                    match arg {
                        Ok(a) => calls.push((f, a)),
                        Err(e) => self.console.push(format!("event error: {e}")),
                    }
                }
            }
        }
        for (f, arg) in calls {
            let r = match arg {
                mlua::Value::Nil => f.call::<()>(()),
                other => f.call::<()>(other),
            };
            if let Err(e) = r {
                self.console.push(format!("event error: {e}"));
            }
        }
        self.flush_console();
    }

    /// Take every queued command (all subsystems, one batch).
    pub(crate) fn take_queues(&self) -> Drained {
        let mut h = self.host.lock().unwrap();
        Drained {
            commands: std::mem::take(&mut h.commands),
            graph_commands: std::mem::take(&mut h.graph_commands),
            anim_commands: std::mem::take(&mut h.anim_commands),
            shader_commands: std::mem::take(&mut h.shader_commands),
            video_commands: std::mem::take(&mut h.video_commands),
            file_commands: std::mem::take(&mut h.file_commands),
            node_commands: std::mem::take(&mut h.node_commands),
        }
    }

    /// Apply one drained batch to the project, collecting the node report.
    pub(crate) fn apply_drained(project: &mut ProjectFile, q: Drained) -> NodeReport {
        use super::editor::NodeApplyOutcome;
        let mut report = NodeReport::default();
        for cmd in &q.graph_commands {
            cmd.apply_to(&mut project.document);
        }
        Self::apply_commands(&mut project.document, &q.commands);
        for cmd in &q.anim_commands {
            cmd.apply_to(&mut project.anim_timeline);
        }
        for cmd in &q.shader_commands {
            cmd.apply_to(&mut project.document);
        }
        for cmd in &q.video_commands {
            cmd.apply_to(&mut project.document);
        }
        for cmd in &q.file_commands {
            if let Some(node) =
                cmd.apply_to_project(&mut project.document, &mut project.nodes)
            {
                report.created_nodes.push(node);
            }
        }
        for cmd in &q.node_commands {
            match cmd.apply_to_project(
                &mut project.document,
                &mut project.nodes,
                &mut project.anim_timeline,
            ) {
                NodeApplyOutcome::Created(node) => report.created_nodes.push(node),
                NodeApplyOutcome::Removed(r) => report.removed.push(r),
                _ => {}
            }
        }
        report
    }

    /// Take the node report from the last batch (console undo ingredients).
    pub fn take_node_report(&mut self) -> NodeReport {
        std::mem::take(&mut self.last_report)
    }

    fn execute_inner(
        &mut self,
        script: &str,
        code: &str,
        mut doc: Option<&mut Document>,
        timeline: Option<&mut AnimationTimeline>,
    ) -> Result<String, VbluaError> {
        let _ = timeline;
        self.check_runnable(script, code)?;
        // Snapshot in, queue cleared (animation stays empty on this path).
        {
            let mut h = self.host.lock().unwrap();
            h.commands.clear();
            h.graph_commands.clear();
            h.anim_commands.clear();
            h.shader_commands.clear();
            h.video_commands.clear();
            // Doc-only path has no node store: assets stay empty, pick() refuses.
            h.file_commands.clear();
            h.has_nodestore = false;
            h.shader_sources.clear();
            h.anim = super::animation::AnimSnapshot::default();
            h.shaders = super::shader::ShaderSnapshot::default();
            // Doc-only path has no node store: assets + anim stay empty.
            h.assets = super::assets::AssetSnapshot::default();
            match doc.as_deref_mut() {
                Some(d) => {
                    h.snapshot = DocumentSnapshot::capture(d);
                    h.has_document = true;
                    h.graphs = super::graph::GraphSnapshot::capture(d);
                    h.graph_kinds.clear();
                    for layer in &d.layers {
                        if let Some(g) = layer.node_graph.as_ref() {
                            for (id, node) in &g.nodes {
                                h.graph_kinds.insert(id.to_string(), node.kind.clone());
                            }
                        }
                    }
                    h.shaders = super::shader::ShaderSnapshot::capture(d);
                    h.video = super::video::VideoSnapshot::capture(d);
                }
                None => {
                    h.snapshot = DocumentSnapshot::default();
                    h.has_document = false;
                    h.graphs = super::graph::GraphSnapshot::default();
                    h.graph_kinds.clear();
                }
            }
        }
        let result = self.eval_chunk(script, code);
        // Batch out: graph + document + shaders + video (partial work kept).
        let (commands, graph_commands, shader_commands, video_commands): (
            Vec<DocumentCommand>,
            Vec<super::graph::GraphCommand>,
            Vec<super::shader::ShaderCommand>,
            Vec<super::video::VideoCommand>,
        ) = {
            let mut h = self.host.lock().unwrap();
            (
                std::mem::take(&mut h.commands),
                std::mem::take(&mut h.graph_commands),
                std::mem::take(&mut h.shader_commands),
                std::mem::take(&mut h.video_commands),
            )
        };
        if let Some(d) = doc {
            for cmd in &graph_commands {
                cmd.apply_to(d);
            }
            Self::apply_commands(d, &commands);
            for cmd in &shader_commands {
                cmd.apply_to(d);
            }
            for cmd in &video_commands {
                cmd.apply_to(d);
            }
        }
        result
    }

    fn check_runnable(&self, script: &str, code: &str) -> Result<(), VbluaError> {
        let _ = script;
        if self.state == Lifecycle::Shutdown {
            return Err(VbluaError::Internal("runtime is shut down".into()));
        }
        if self.state == Lifecycle::Paused {
            return Err(VbluaError::Permission {
                capability: "execute".into(),
                message: "runtime is paused".into(),
            });
        }
        if code.len() > 1_000_000 {
            return Err(VbluaError::Resource {
                resource: "script".into(),
                message: "script exceeds 1 MiB".into(),
            });
        }
        Ok(())
    }

    fn eval_chunk(&mut self, script: &str, code: &str) -> Result<String, VbluaError> {
        self.hook.reset_budget();
        let Some(lua) = &self.lua else {
            return Err(VbluaError::Internal("interpreter missing".into()));
        };
        // `print` may have been clobbered by a previous script — restore it.
        self.install_print_hook(lua);
        match lua.load(code).set_name(script).eval::<mlua::Value>() {
            Ok(v) => {
                self.flush_console();
                Ok(match v {
                    mlua::Value::Nil => "nil".into(),
                    mlua::Value::Integer(i) => i.to_string(),
                    mlua::Value::Number(n) => n.to_string(),
                    mlua::Value::Boolean(b) => b.to_string(),
                    mlua::Value::String(s) => {
                        s.to_str().map(|s| s.to_string()).unwrap_or_default()
                    }
                    other => format!("{other:?}"),
                })
            }
            Err(e) => {
                self.flush_console();
                Err(from_mlua(script, e))
            }
        }
    }

    /// Apply a staged batch. Returns the number of commands that changed state.
    /// The caller wraps this in one undo transaction (Phase 16).
    pub fn apply_commands(doc: &mut Document, commands: &[DocumentCommand]) -> usize {
        commands.iter().filter(|c| c.apply_to(doc)).count()
    }

    /// Apply a staged graph batch. Returns the number of effective changes.
    pub fn apply_graph_commands(
        doc: &mut Document,
        commands: &[super::graph::GraphCommand],
    ) -> usize {
        commands.iter().filter(|c| c.apply_to(doc)).count()
    }

    /// Apply a staged timeline batch. Returns the number of effective changes.
    pub fn apply_anim_commands(
        timeline: &mut crate::document::AnimationTimeline,
        commands: &[super::animation::AnimCommand],
    ) -> usize {
        commands.iter().filter(|c| c.apply_to(timeline)).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_doc() -> Document {
        Document {
            title: "t".into(),
            width: 100.0,
            height: 100.0,
            layers: vec![],
            active_layer_index: 0,
            defs: Default::default(),
            path_effects: Default::default(),
            tiling_effects: Default::default(),
            circular_effects: Default::default(),
            clip_masks: Default::default(),
            boolean_effects: Default::default(),
            page_color: [1.0, 1.0, 1.0, 1.0],
            page_unit: Default::default(),
            timeline_markers: Vec::new(),
            timeline_scripts: Vec::new(),
        }
    }

    #[test]
    fn hello_script_runs() {
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let out = rt
            .execute("hello.lua", r#"vblua.log("Hello"); return vblua.version()"#)
            .unwrap();
        assert_eq!(out, super::super::VBLUA_VERSION);
        assert!(rt.console.iter().any(|l| l.contains("Hello")));
    }

    #[test]
    fn syntax_error_is_structured() {
        let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
        let err = rt.execute("bad.lua", "local x = ").unwrap_err();
        assert!(matches!(err, VbluaError::Syntax { .. }), "{err:?}");
    }

    #[test]
    fn runtime_error_does_not_crash_host() {
        let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
        let err = rt.execute("boom.lua", r#"nil_fn()"#).unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
        // Host still works afterwards.
        assert_eq!(rt.execute("ok.lua", "return 1").unwrap(), "1");
    }

    #[test]
    fn document_read_and_queued_write_batch() {
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut doc = test_doc();
        let out = rt
            .execute_with_document(
                "doc.lua",
                r#"
                local d = vblua.document.current()
                vblua.document.rename("New")
                vblua.document.resize(200, 300)
                return d.title .. ":" .. d.width
                "#,
                Some(&mut doc),
            )
            .unwrap();
        assert!(out == "t:100" || out == "t:100.0", "got {out}");
        assert_eq!(doc.title, "New");
        assert_eq!((doc.width, doc.height), (200.0, 300.0));
        // Read-only policy denies writes with a structured error; doc unchanged.
        let mut ro = VbRuntime::new(SandboxPolicy::default()).unwrap();
        let mut doc2 = test_doc();
        let err = ro
            .execute_with_document(
                "ro.lua",
                r#"vblua.document.rename("X"); return vblua.document.current().title"#,
                Some(&mut doc2),
            )
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
        assert_eq!(doc2.title, "t");
    }

    #[test]
    fn pause_resume_reset_shutdown() {
        let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
        rt.pause();
        assert!(rt.execute("x.lua", "return 1").is_err());
        rt.resume();
        assert_eq!(rt.execute("x.lua", "return 1").unwrap(), "1");
        rt.reset().unwrap();
        assert_eq!(rt.execute("x.lua", "return 1").unwrap(), "1");
        rt.shutdown();
        assert!(rt.execute("x.lua", "return 1").is_err());
    }

    #[test]
    fn sandbox_strips_io() {
        let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
        let out = rt.execute("io.lua", "return io.open").unwrap();
        assert_eq!(out, "nil");
    }

    #[test]
    fn math_helpers_work() {
        let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
        assert_eq!(
            rt.execute("m.lua", "return vblua.math.clamp(5, 0, 3)").unwrap(),
            "3"
        );
        assert_eq!(
            rt.execute("m.lua", "return vblua.math.lerp(0, 10, 0.5)").unwrap(),
            "5"
        );
    }

    #[test]
    fn graph_create_connect_same_run() {
        use crate::document::Layer;
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut doc = test_doc();
        doc.layers
            .push(Layer::new_node_editor_layer(uuid::Uuid::new_v4(), "NE".into()));
        doc.layers[0].ensure_node_graph();
        let out = rt
            .execute_with_document(
                "g.lua",
                r#"
                local a = vblua.graph.create("Blur", {x=10, y=20})
                local b = vblua.graph.create("Blur", {x=200, y=20})
                vblua.graph.connect(a, "out", b, "in")
                vblua.graph.rename(a, "First")
                return vblua.graph.get(a).name
                "#,
                Some(&mut doc),
            )
            .unwrap();
        assert_eq!(out, "First");
        let g = doc.layers[0].node_graph.as_ref().unwrap();
        // Seed Output Object + 2 blurs.
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(g.links.len(), 1);
    }

    #[test]
    fn graph_denies_images_and_needs_write() {
        use crate::document::Layer;
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut doc = test_doc();
        doc.layers
            .push(Layer::new_node_editor_layer(uuid::Uuid::new_v4(), "NE".into()));
        doc.layers[0].ensure_node_graph();
        let err = rt
            .execute_with_document("g.lua", r#"return vblua.graph.create("Image")"#, Some(&mut doc))
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
        // Read-only policy: create is denied.
        let mut ro = VbRuntime::new(SandboxPolicy::default()).unwrap();
        let err = ro
            .execute_with_document(
                "g.lua",
                r#"return vblua.graph.create("Blur")"#,
                Some(&mut doc),
            )
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
    }

    #[test]
    fn animation_keyframes_end_to_end() {        use crate::document::{Fill, Node, NodeStore};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut project = ProjectFile::new(test_doc(), NodeStore::default());
        let node = Node::rect(0.0, 0.0, 10.0, 10.0, Fill::None);
        let nid = node.id.to_string();
        project.nodes.insert(node);
        let out = rt
            .execute_with_project(
                "a.lua",
                &format!(
                    r#"
                vblua.animation.set_keyframe("{nid}", "rotation", 0, 0)
                vblua.animation.set_keyframe("{nid}", "rotation", 30, 360, "bezier")
                return vblua.animation.sample("{nid}", "rotation", 15)
                "#
                ),
                Some(&mut project),
            )
            .unwrap();
        assert!(out == "180" || out == "180.0", "got {out}");
        // keyframes() lists both, in frame order.
        let out = rt
            .execute_with_project(
                "a.lua",
                &format!(
                    r#"return #vblua.animation.keyframes("{nid}", "rotation")"#
                ),
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "2");
        // Unknown track / id / interp are structured errors.
        for bad in [
            format!(r#"vblua.animation.set_keyframe("{nid}", "radius", 0, 1)"#),
            r#"vblua.animation.set_keyframe("00000000-0000-0000-0000-000000000000", "rotation", 0, 1)"#.to_string(),
            format!(r#"vblua.animation.set_keyframe("{nid}", "rotation", 0, 1, "step")"#),
        ] {
            let err = rt
                .execute_with_project("a.lua", &bad, Some(&mut project))
                .unwrap_err();
            assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
        }
        // Removal reports existence and clears the track.
        let out = rt
            .execute_with_project(
                "a.lua",
                &format!(
                    r#"
                vblua.animation.remove_keyframe("{nid}", "rotation", 0)
                return #vblua.animation.keyframes("{nid}", "rotation")
                "#
                ),
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "1");
        assert_eq!(
            super::super::animation::timeline_signature(&project.anim_timeline),
            1
        );
    }

    #[test]
    fn graph_params_end_to_end() {
        use crate::document::Layer;
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut doc = test_doc();
        doc.layers
            .push(Layer::new_node_editor_layer(uuid::Uuid::new_v4(), "NE".into()));
        doc.layers[0].ensure_node_graph();
        let out = rt
            .execute_with_document(
                "p.lua",
                r#"
                local v = vblua.graph.create("Value")
                vblua.graph.set_param(v, "value", 42)
                local e = vblua.graph.create("ExprX")
                vblua.graph.set_param(e, "expr", "x*2")
                return vblua.graph.get_param(v, "value") .. ":" .. vblua.graph.get(e).params.expr
                "#,
                Some(&mut doc),
            )
            .unwrap();
        assert!(out == "42:x*2" || out == "42.0:x*2", "got {out}");
        // Rejections are structured errors, doc untouched by them.
        let err = rt
            .execute_with_document(
                "p.lua",
                r#"
                local v = vblua.graph.create("Value")
                return vblua.graph.set_param(v, "value", "not-a-number")
                "#,
                Some(&mut doc),
            )
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
    }

    #[test]
    fn kinematics_pure_math_end_to_end() {
        let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
        // Chain solve -> fk round-trips near the target (no document needed).
        let out = rt
            .execute(
                "k.lua",
                r#"
                local arm = vblua.kinematics.chain({ 100, 80, 60 })
                local angles = arm.solve({ x = 150, y = 40 })
                local joints = arm.fk(angles)
                local tip = joints[#joints]
                return math.abs(tip.x - 150) < 1 and math.abs(tip.y - 40) < 1
                "#,
            )
            .unwrap();
        assert_eq!(out, "true");
        // Analytic two-bone + helpers.
        let out = rt
            .execute(
                "k.lua",
                r#"
                local s = vblua.kinematics.ik2(100, 80, { x = 0, y = 0 }, { x = 150, y = 40 })
                local pts = vblua.kinematics.fk({ 100, 80 }, { s.a1, s.a2 })
                local tip = pts[3]
                return math.abs(tip.x - 150) < 0.01 and vblua.kinematics.damp(0, 10, 5, 1) > 9
                "#,
            )
            .unwrap();
        assert_eq!(out, "true");
        // Bad chains fail loudly.
        let err = rt.execute("k.lua", r#"return vblua.kinematics.chain({})"#).unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
    }

    #[test]
    fn shader_create_validate_uniforms_end_to_end() {
        use crate::document::{Layer, NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut doc = test_doc();
        doc.layers
            .push(Layer::new_image(uuid::Uuid::new_v4(), "L".into(), true, false, vec![]));
        let mut project = ProjectFile::new(doc, NodeStore::default());
        // Pure validation needs no document.
        assert!(
            rt.execute("s.lua", "return vblua.shader.validate('void main() { gl_FragColor = 1; }')")
                .is_err()
        );
        let out = rt
            .execute_with_project(
                "s.lua",
                r#"
                local p = vblua.shader.create(nil, "Glow")
                vblua.shader.set_uniforms(p, { 0.0, 0.5 })
                return #vblua.shader.passes()
                "#,
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "1");
        let layer_passes = &project.document.layers[0].shading_passes;
        assert_eq!(layer_passes.len(), 1);
        assert!(!layer_passes[0].enabled);
        assert_eq!(layer_passes[0].uniforms, vec![0.0, 0.5]);
        // GLSL source is rejected, doc untouched by it.
        let err = rt
            .execute_with_project(
                "s.lua",
                r#"
                local p = vblua.shader.create()
                return vblua.shader.set_source(p, "void main() { gl_FragColor = 1; }")
                "#,
                Some(&mut project),
            )
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
    }

    #[test]
    fn video_move_trim_split_end_to_end() {
        use crate::document::{AvClip, Layer, NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut doc = test_doc();
        doc.layers.push(Layer::new_av_layer(
            uuid::Uuid::new_v4(),
            "AV".into(),
            String::new(),
        ));
        let cid = uuid::Uuid::new_v4();
        doc.layers[0].av_clips.push(AvClip {
            id: cid,
            name: "Take".into(),
            media_path: "take.mp4".into(),
            video_start_offset: 0.0,
            video_play_length: 20.0,
            video_timeline_start: 5.0,
            media_source_duration: Some(30.0),
            track_row: 0,
            source_node_ids: vec![],
            muted: false,
            locked: false,
        });
        let mut project = ProjectFile::new(doc, NodeStore::default());
        let out = rt
            .execute_with_project(
                "v.lua",
                &format!(
                    r#"
                local c = vblua.video.get("{cid}")
                vblua.video.move("{cid}", 10.0)
                vblua.video.trim("{cid}", {{ offset = 2.0, length = 8.0 }})
                return c.kind .. ":" .. c.start
                "#
                ),
                Some(&mut project),
            )
            .unwrap();
        assert!(out == "video:5" || out == "video:5.0", "got {out}");
        let c = &project.document.layers[0].av_clips[0];
        assert_eq!((c.video_timeline_start, c.video_start_offset), (10.0, 2.0));
        // Split returns the new id; add_clip stays denied.
        let out = rt
            .execute_with_project(
                "v.lua",
                &format!(
                    r#"
                local right = vblua.video.split("{cid}", 14.0)
                return #vblua.video.clips() .. ":" .. tostring(right ~= nil)
                "#
                ),
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "2:true");
        let err = rt
            .execute_with_project(
                "v.lua",
                r#"return vblua.video.add_clip("video.mp4")"#,
                Some(&mut project),
            )
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
    }

    #[test]
    fn assets_inventory_end_to_end() {
        use crate::document::{AvClip, Fill, Layer, Node, NodeKind, NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
        let mut doc = test_doc();
        doc.layers.push(Layer::new_av_layer(
            uuid::Uuid::new_v4(),
            "AV".into(),
            String::new(),
        ));
        let mut project = ProjectFile::new(doc, NodeStore::default());
        let mut img = Node::rect(0.0, 0.0, 10.0, 10.0, Fill::None);
        img.kind = NodeKind::Image {
            x: 0.0,
            y: 0.0,
            width: 640.0,
            height: 480.0,
            bytes: vec![0u8; 64],
            collab_asset_sha256: None,
        };
        project.nodes.insert(img);
        project.document.layers[0].av_clips.push(AvClip {
            id: uuid::Uuid::new_v4(),
            name: "Take".into(),
            media_path: "/host/secret/take.mp4".into(),
            video_start_offset: 0.0,
            video_play_length: 20.0,
            video_timeline_start: 0.0,
            media_source_duration: Some(30.0),
            track_row: 0,
            source_node_ids: vec![],
            muted: false,
            locked: false,
        });
        // Read-only policy is enough (inventory never mutates).
        let out = rt
            .execute_with_project(
                "a.lua",
                r#"
                local items = vblua.assets.list()
                local leaked = false
                for _, it in ipairs(items) do
                    local s = tostring(it.ref) .. tostring(it.name)
                    if s:find("secret") or s:find("mp4") then leaked = true end
                end
                return #items .. ":" .. tostring(leaked)
                "#,
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "2:false");
        let err = rt
            .execute_with_project("a.lua", r#"return vblua.assets.import("x.png")"#, Some(&mut project))
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
    }

    #[test]
    fn file_pick_imports_image_end_to_end() {
        use crate::document::{Layer, NodeKind, NodeStore, ProjectFile};
        use std::sync::Arc;
        // Fake platform picker: 2x3 red PNG bytes, no filesystem involved.
        let png = {
            use image::ImageEncoder;
            let img = image::RgbaImage::from_pixel(2, 3, image::Rgba([1, 2, 3, 255]));
            let mut buf = Vec::new();
            let enc = image::codecs::png::PngEncoder::new(&mut buf);
            enc.write_image(img.as_raw(), 2, 3, image::ExtendedColorType::Rgba8)
                .unwrap();
            buf
        };
        let hook_png = png.clone();
        let hook: super::super::file::PickerHook = Arc::new(move |_| {
            Ok(super::super::file::PickedFile {
                name: "fake.png".into(),
                bytes: hook_png.clone(),
            })
        });
        let mut policy = SandboxPolicy::permissive_for_tests();
        policy.grant(super::super::Capability::FilesystemRead);
        let mut rt = VbRuntime::new(policy).unwrap();
        rt.set_picker(Some(hook));
        assert!(rt.has_picker());
        let mut doc = test_doc();
        doc.layers.push(Layer::new_image(
            uuid::Uuid::new_v4(),
            "L".into(),
            true,
            false,
            vec![],
        ));
        let mut project = ProjectFile::new(doc, NodeStore::default());
        let out = rt
            .execute_with_project(
                "f.lua",
                r#"
                assert(vblua.file.status().picker == true)
                local id = vblua.file.pick({ filters = { "png" } })
                local info = vblua.assets.info("node:" .. id)
                return info.name .. ":" .. info.width .. "x" .. info.height
                "#,
                Some(&mut project),
            )
            .unwrap();
        assert!(out == "fake.png:2x3" || out == "fake.png:2.0x3.0", "got {out}");
        let (id, node) = project.nodes.map.iter().next().unwrap();
        assert!(matches!(node.kind, NodeKind::Image { .. }));
        assert!(project.document.layers[0].nodes.contains(id));
        // No hook -> clean error; no capability -> permission error.
        let mut bare = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let err = bare
            .execute("f.lua", r#"return vblua.file.pick({ filters = { "png" } })"#)
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
    }

    #[test]
    fn ui_panels_and_callbacks_end_to_end() {
        use crate::document::{NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut project = ProjectFile::new(test_doc(), NodeStore::default());
        // Declare: panel + text + button (with callback) + slider (with callback).
        rt.execute_with_project(
            "u.lua",
            r#"
            vblua.ui.panel("tools", "My Tool")
            vblua.ui.text("tools", "hello")
            vblua.ui.button("tools", "Generate", function()
              vblua.document.rename("FromButton")
            end)
            vblua.ui.slider("tools", "Amount", 0, 100, 50, function(v)
              vblua.log("amount " .. v)
            end)
            return #vblua.ui.panels()
            "#,
            Some(&mut project),
        )
        .unwrap();
        assert_eq!(rt.ui_panel_count(), 1);
        // Simulate renderer: click button (widget 2), drag slider (widget 3).
        rt.ui_push_event(super::super::ui_widgets::UiEvent::Clicked(2));
        rt.ui_push_event(super::super::ui_widgets::UiEvent::NumChanged(3, 75.0));
        rt.poll_ui_callbacks(Some(&mut project));
        assert_eq!(project.document.title, "FromButton");
        assert!(rt.console.iter().any(|l| l.contains("amount 75")), "{:?}", rt.console);
        // Read-only policy: panel declaration is denied.
        let mut ro = VbRuntime::new(SandboxPolicy::default()).unwrap();
        let err = ro
            .execute("u.lua", r#"return vblua.ui.panel("x", "X")"#)
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
    }

    #[test]
    fn scripted_templates_define_and_spawn() {
        use crate::document::{Layer, NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut doc = test_doc();
        doc.layers.push(Layer::new_node_editor_layer(
            uuid::Uuid::new_v4(),
            "NE".into(),
        ));
        doc.layers[0].ensure_node_graph();
        let mut project = ProjectFile::new(doc, NodeStore::default());
        let out = rt
            .execute_with_project(
                "n.lua",
                r#"
                vblua.node.define("SoftGlow", {
                  nodes = {
                    { kind = "Blur", x = 0, y = 0 },
                    { kind = "Brightness", x = 200, y = 0, name = "Lift" },
                  },
                  links = {
                    { from = 1, from_port = "out", to = 2, to_port = "in" },
                  },
                })
                local ids = vblua.node.spawn("SoftGlow", { x = 10, y = 20 })
                return #ids .. ":" .. vblua.graph.get(ids[2]).name
                "#,
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "2:Lift");
        let g = project.document.layers[0].node_graph.as_ref().unwrap();
        // Seed Output + 2 spawned; 1 internal link.
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(g.links.len(), 1);
        // Bad specs fail at define; unknown spawns fail at spawn.
        for bad in [
            r#"vblua.node.define("B", { nodes = { { kind = "Image" } } })"#,
            r#"vblua.node.define("B", { nodes = {} })"#,
            r#"return vblua.node.spawn("Missing")"#,
        ] {
            let err = rt
                .execute_with_project("n.lua", bad, Some(&mut project))
                .unwrap_err();
            assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
        }
    }

    #[test]
    fn batch_procedural_end_to_end() {
        use crate::document::{Layer, NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut doc = test_doc();
        doc.layers.push(Layer::new_node_editor_layer(
            uuid::Uuid::new_v4(),
            "NE".into(),
        ));
        doc.layers[0].ensure_node_graph();
        let mut project = ProjectFile::new(doc, NodeStore::default());
        // A canvas node to keyframe (graph ids are not animation targets).
        let cn = crate::document::Node::rect(0.0, 0.0, 10.0, 10.0, crate::document::Fill::None);
        let cnid = cn.id.to_string();
        project.nodes.insert(cn);
        // 100 nodes in ONE call from a Rust-computed grid.
        let out = rt
            .execute_with_project(
                "b.lua",
                &format!(
                    r#"
                local pts = vblua.batch.grid({{ x = 0, y = 0 }}, 10, 60, 60, 100)
                local specs = {{}}
                for i, p in ipairs(pts) do
                  specs[i] = {{ kind = "Value", x = p.x, y = p.y, params = {{ value = i }} }}
                end
                local ids = vblua.batch.create_nodes(specs)
                vblua.batch.keyframes("{cnid}", "rotation", {{{{ 0, 0 }}, {{ 60, 360 }}}})
                return #ids .. ":" .. #vblua.animation.keyframes("{cnid}", "rotation")
                "#
                ),
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "100:2");
        let g = project.document.layers[0].node_graph.as_ref().unwrap();
        assert_eq!(g.nodes.len(), 101); // seed Output + 100
        // Over-budget batches fail loudly.
        let err = rt
            .execute_with_project(
                "b.lua",
                r#"
                local specs = {}
                for i = 1, 600 do specs[i] = { kind = "Value" } end
                return vblua.batch.create_nodes(specs)
                "#,
                Some(&mut project),
            )
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
    }

    #[test]
    fn editor_transactions_and_node_ops() {
        use crate::document::{Fill, Node, NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut project = ProjectFile::new(test_doc(), NodeStore::default());
        let node = Node::rect(0.0, 0.0, 10.0, 10.0, Fill::None);
        let nid = node.id.to_string();
        project.nodes.insert(node);
        // Transaction commit: rename applies.
        rt.execute_with_project(
            "e.lua",
            &format!(
                r#"vblua.editor.transaction(function() vblua.editor.rename_node("{nid}", "Hero") end)"#
            ),
            Some(&mut project),
        )
        .unwrap();
        assert_eq!(project.nodes.map.get(&nid.parse::<uuid::Uuid>().unwrap()).unwrap().name, "Hero");
        // Transaction rollback: error discards the whole batch.
        let err = rt
            .execute_with_project(
                "e.lua",
                &format!(
                    r#"
                vblua.editor.transaction(function()
                  vblua.editor.rename_node("{nid}", "Gone")
                  error("boom")
                end)"#
                ),
                Some(&mut project),
            )
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
        assert_eq!(project.nodes.map.get(&nid.parse::<uuid::Uuid>().unwrap()).unwrap().name, "Hero");
        // Duplicate + delete with report ingredients.
        let out = rt
            .execute_with_project(
                "e.lua",
                &format!(r#"return vblua.editor.duplicate_node("{nid}")"#),
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out.len(), 36);
        let report = rt.take_node_report();
        assert_eq!(report.created_nodes.len(), 1);
        let copy = report.created_nodes[0].id.to_string();
        rt.execute_with_project(
            "e.lua",
            &format!(r#"return vblua.editor.delete_node("{copy}")"#),
            Some(&mut project),
        )
        .unwrap();
        let report = rt.take_node_report();
        assert_eq!(report.removed.len(), 1);
        assert_eq!(report.removed[0].node.id.to_string(), copy);
        // Unknown ids fail loudly.
        let err = rt
            .execute_with_project(
                "e.lua",
                r#"return vblua.editor.delete_node("00000000-0000-0000-0000-000000000000")"#,
                Some(&mut project),
            )
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
    }

    #[test]
    fn infinite_loops_become_timeouts_not_hangs() {
        let mut policy = SandboxPolicy::default();
        policy.max_instructions = 100_000;
        let mut rt = VbRuntime::new(policy).unwrap();
        let err = rt.execute("loop.lua", "while true do end").unwrap_err();
        assert!(matches!(err, VbluaError::Timeout { .. }), "{err:?}");
        // Host survives and budgets reset per run.
        assert_eq!(rt.execute("ok.lua", "return 1").unwrap(), "1");
    }

    #[test]
    fn permissions_surface_lists_grants() {
        let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
        let out = rt
            .execute("p.lua", "return #vblua.permissions()")
            .unwrap();
        assert_eq!(out, "1"); // document.read only
    }

    #[test]
    fn addons_enable_runs_code_with_granted_permissions() {
        use crate::document::{NodeStore, ProjectFile};
        let dir = std::env::temp_dir().join(format!("vblua-e2e-addon-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("glow")).unwrap();
        std::fs::write(
            dir.join("glow").join("manifest.lua"),
            r#"return { id = "example.glow", name = "Glow", version = "1.0.0", vblua = "1", permissions = { "ui" } }"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("glow").join("addon.lua"),
            r#"vblua.ui.panel("glow", "Glow Tool")"#,
        )
        .unwrap();
        // Default policy denies ui: enable must grant it via the manifest.
        let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
        rt.set_addons_dir(Some(dir.clone()));
        let mut project = ProjectFile::new(test_doc(), NodeStore::default());
        let out = rt
            .execute_with_project(
                "a.lua",
                r#"vblua.addons.refresh() vblua.addons.enable("example.glow") return #vblua.addons.list()"#,
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "1");
        assert_eq!(rt.ui_panel_count(), 1);
        assert!(rt.console.iter().any(|l| l.contains("granted")), "{:?}", rt.console);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn capability_probe_for_mobile_branching() {
        let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
        // Capabilities mirror the policy; platform features reflect wiring.
        let out = rt
            .execute(
                "h.lua",
                r#"return tostring(vblua.has("document.read")) .. ":" .. tostring(vblua.has("document.write")) .. ":" .. tostring(vblua.has("file_picker")) .. ":" .. tostring(vblua.has("nope"))"#,
            )
            .unwrap();
        assert_eq!(out, "true:false:false:false");
    }

    #[test]
    fn events_fire_once_with_no_recursion() {
        use crate::document::{NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut project = ProjectFile::new(test_doc(), NodeStore::default());
        // Subscribe: node-created queues ANOTHER node (must not refire events).
        rt.execute_with_project(
            "e.lua",
            r#"
            FIRES = 0
            vblua.events.on("node-created", function(ids)
              FIRES = FIRES + 1
              vblua.log("created:" .. #ids)
            end)
            vblua.events.on("error", function(m) vblua.log("saw-error") end)
            return vblua.editor.duplicate_node(vblua.editor.nodes()[1] and vblua.editor.nodes()[1].id or "x")
            "#,
            Some(&mut project),
        )
        .unwrap_or_default();
        // No canvas nodes exist: duplicate errors -> error event fires, nothing loops.
        assert!(rt.console.iter().any(|l| l.contains("saw-error")), "{:?}", rt.console);
        assert_eq!(rt.ui_panel_count(), 0);
    }

    #[test]
    fn node_created_event_carries_new_ids() {
        use crate::document::{Fill, Node, NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut project = ProjectFile::new(test_doc(), NodeStore::default());
        let node = Node::rect(0.0, 0.0, 5.0, 5.0, Fill::None);
        project.nodes.insert(node);
        rt.execute_with_project(
            "e.lua",
            r#"
            SEEN = {}
            vblua.events.on("node-created", function(ids)
              for _, id in ipairs(ids) do SEEN[#SEEN+1] = id end
            end)
            local me = vblua.editor.nodes()[1].id
            vblua.editor.duplicate_node(me)
            return #SEEN
            "#,
            Some(&mut project),
        )
        .unwrap();
        // Callback ran after apply: one created id observed, exactly once.
        let out = rt
            .execute_with_project("e.lua", "return #SEEN", Some(&mut project))
            .unwrap();
        assert_eq!(out, "1");
    }

    #[test]
    fn dangerous_stdlib_stays_stripped() {
        let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
        let out = rt
            .execute(
                "s.lua",
                r#"return tostring(dofile) .. tostring(loadfile) .. tostring(require) .. tostring(io.open) .. tostring(os.execute) .. tostring(package.loadlib)"#,
            )
            .unwrap();
        assert_eq!(out, "nilnilnilnilnilnil");
    }

    #[test]
    fn debugger_traces_and_breakpoints() {
        let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
        // Trace records chunk lines.
        let out = rt
            .execute(
                "dbg.lua",
                r#"
                vblua.debug.trace(function()
                  local x = 1
                  x = x + 1
                  return x
                end)
                local evs = vblua.debug.trace_events()
                return #evs > 0 and evs[1].line >= 1
                "#,
            )
            .unwrap();
        assert_eq!(out, "true");
        // Breakpoint aborts with line info (registered on line 1, hit on line 3).
        let err = rt
            .execute(
                "bp.lua",
                "vblua.debug.breakpoint('bp.lua', 3)\nlocal x = 1\nreturn x",
            )
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("breakpoint"), "{msg}");
        rt.execute("clear.lua", "vblua.debug.clear_breakpoints() return 1")
            .unwrap();
        // Errors inside trace() still propagate.
        let err = rt
            .execute("t.lua", "vblua.debug.trace(function() error('x') end)")
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
    }

    #[test]
    fn app_events_dispatch_from_host() {
        use crate::document::{NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut project = ProjectFile::new(test_doc(), NodeStore::default());
        rt.execute_with_project(
            "e.lua",
            r#"
            vblua.events.on("document-open", function()
              vblua.document.rename("Hooked")
            end)"#,
            Some(&mut project),
        )
        .unwrap();
        assert_eq!(project.document.title, "t");
        rt.fire_app_event(&mut project, "document-open").unwrap();
        assert_eq!(project.document.title, "Hooked");
        // Unknown app events are refused.
        assert!(rt.fire_app_event(&mut project, "nope").is_err());
    }

    #[test]
    fn graph_ports_and_canonical_kinds() {
        use crate::document::{Layer, NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut doc = test_doc();
        doc.layers.push(Layer::new_node_editor_layer(
            uuid::Uuid::new_v4(),
            "NE".into(),
        ));
        doc.layers[0].ensure_node_graph();
        let mut project = ProjectFile::new(doc, NodeStore::default());
        let out = rt
            .execute_with_project(
                "g.lua",
                r#"
                local id = vblua.graph.create("Blur")
                local ports = vblua.graph.ports(id)
                local ins = {}
                for _, p in ipairs(ports) do
                  if p.dir == "input" then ins[#ins+1] = p.id end
                  if p.dir == "output" then outp = p.id end
                end
                return vblua.graph.get(id).kind .. ":" .. outp .. ":" .. table.concat(ins, "+")
                "#,
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "Linear Blur:out:in+amount");
        // Duplicate lands on source + offset (24 default), reported with x/y.
        let out = rt
            .execute_with_project(
                "g.lua",
                r#"
                local id = vblua.graph.create("Value", { x = 100, y = 50 })
                local copy = vblua.graph.duplicate(id)
                local n = vblua.graph.get(copy)
                return n.x .. "," .. n.y
                "#,
                Some(&mut project),
            )
            .unwrap();
        assert!(out == "124,74" || out == "124.0,74.0", "got {out}");
        // Unknown nodes fail loudly.
        let err = rt
            .execute_with_project(
                "g.lua",
                r#"return vblua.graph.ports("00000000-0000-0000-0000-000000000000")"#,
                Some(&mut project),
            )
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
    }

    #[test]
    fn image_analysis_end_to_end() {
        let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
        // Pure red 2x2: Rec.709 average is 0.2126 on any backend.
        let out = rt
            .execute(
                "img.lua",
                r#"
                local img = vblua.image.solid(2, 2, { 1, 0, 0, 1 })
                local b = vblua.image.average_brightness(img)
                local c = vblua.image.average_color(img)
                local s = vblua.image.sample(img, 0, 0)
                local r = vblua.image.region_average(img, 0, 0, 1, 1)
                local g = vblua.image.grayscale(img)
                local gb = vblua.image.average_brightness(g)
                return tostring(math.abs(b - 0.2126) < 0.02) .. tostring(c[1] == 1)
                  .. tostring(s[1] == 1) .. tostring(math.abs(r - b) < 1e-6)
                  .. tostring(math.abs(gb - b) < 0.02) .. vblua.image.backend()
                "#,
            )
            .unwrap();
        assert!(out.starts_with("truetruetruetruetrue"), "{out}");
        // Kernel validation: WGSL compute ok, GLSL/fragments rejected.
        let out = rt
            .execute(
                "k.lua",
                r#"
                local k = vblua.kernel.create({
                  name = "sobel", language = "wgsl",
                  source = "@compute @workgroup_size(8,8)\nfn main() {}",
                  parameters = { strength = 1.5 },
                })
                return vblua.kernel.info(k).name
                "#,
            )
            .unwrap();
        assert_eq!(out, "sobel");
        let err = rt
            .execute("k.lua", r#"return vblua.kernel.create({ name = "x", source = "void main() {}" })"#)
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
    }

    #[test]
    fn image_apply_runs_on_gpu_or_fails_cleanly() {
        let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
        let out = rt
            .execute(
                "apply.lua",
                r#"
                local img = vblua.image.solid(8, 8, { 0.5, 0.5, 0.5, 1 })
                local k = vblua.kernel.create({
                  name = "balance", language = "wgsl",
                  source = [[
                    struct VbParams { values: array<vec4<f32>, 16> },
                    @group(0) @binding(0) var input_tex: texture_2d<f32>;
                    @group(0) @binding(1) var output_tex: texture_storage_2d<rgba8unorm, write>;
                    @group(0) @binding(2) var<uniform> params: VbParams;
                    @compute @workgroup_size(8, 8)
                    fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                      let px = textureLoad(input_tex, gid.xy, 0);
                      textureStore(output_tex, gid.xy, vec4<f32>(px.r * 2.0, px.g, px.b, px.a));
                    }
                  ]],
                  parameters = {},
                })
                local ok, res = pcall(vblua.image.apply, img, k, {})
                if not ok then return "clean-error" end
                local s = vblua.image.sample(res, 0, 0)
                return tostring(s[1] == 1)
                "#,
            )
            .unwrap();
        // GPU present: red doubled to 1. Elsewhere: structured error, never a panic.
        assert!(out == "true" || out == "clean-error", "{out}");
    }

    #[test]
    fn milestone_speed_zoom_chain_wires() {
        use crate::document::{Layer, NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut doc = test_doc();
        doc.layers.push(Layer::new_node_editor_layer(
            uuid::Uuid::new_v4(),
            "NE".into(),
        ));
        doc.layers[0].ensure_node_graph();
        let mut project = ProjectFile::new(doc, NodeStore::default());
        // Video → Speed → ZoomVideo → Output (aliases resolve, ports validate).
        let out = rt
            .execute_with_project(
                "m.lua",
                r#"
                local speed = vblua.graph.create("Speed", { x = 100, y = 0 })
                local zoom = vblua.graph.create("ZoomVideo", { x = 300, y = 0 })
                local rev = vblua.graph.create("Reverse", { x = 500, y = 0 })
                vblua.graph.connect(speed, "out", zoom, "in")
                vblua.graph.connect(zoom, "out", rev, "in")
                return vblua.graph.get(rev).kind
                "#,
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "Reverse");
        // Speed takes no scriptable params (amounts arrive via wires).
        let err = rt
            .execute_with_project(
                "m.lua",
                r#"
                local speed = vblua.graph.create("Speed")
                return vblua.graph.set_param(speed, "nope", 1)
                "#,
                Some(&mut project),
            )
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
        let out = rt
            .execute_with_project(
                "m.lua",
                r#"
                local speed = vblua.graph.create("Speed")
                local zoom = vblua.graph.create("ZoomVideo")
                vblua.graph.connect(speed, "out", zoom, "in")
                -- script 1 already added one "Zoom": this is the second.
                local outs = vblua.graph.find("Zoom")
                return #outs
                "#,
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "2");
        // ...while the same-layer chain resolves with zoom fields applied.
        let g = project.document.layers[0].node_graph.as_ref().unwrap();
        assert!(g.nodes.len() >= 3);
    }

    #[test]
    fn spatial_kinds_create_and_wire() {
        use crate::document::{Layer, NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut doc = test_doc();
        doc.layers.push(Layer::new_node_editor_layer(
            uuid::Uuid::new_v4(),
            "NE".into(),
        ));
        doc.layers[0].ensure_node_graph();
        let mut project = ProjectFile::new(doc, NodeStore::default());
        let out = rt
            .execute_with_project(
                "s.lua",
                r#"
                local t = vblua.graph.create("Transform")
                local c = vblua.graph.create("Crop")
                local fh = vblua.graph.create("FlipHorizontal")
                local fv = vblua.graph.create("FlipVertical")
                vblua.graph.connect(t, "out", c, "in")
                vblua.graph.connect(c, "out", fh, "in")
                vblua.graph.connect(fh, "out", fv, "in")
                return vblua.graph.get(t).kind .. ":" .. vblua.graph.get(fv).kind
                "#,
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "Transform:Flip Vertical");
    }

    #[test]
    fn clip_mute_lock_duplicate() {
        use crate::document::{AvClip, Layer, NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut doc = test_doc();
        doc.layers.push(Layer::new_av_layer(
            uuid::Uuid::new_v4(),
            "AV".into(),
            String::new(),
        ));
        let cid = uuid::Uuid::new_v4();
        doc.layers[0].av_clips.push(AvClip {
            id: cid,
            name: "Take".into(),
            media_path: "take.mp4".into(),
            video_start_offset: 0.0,
            video_play_length: 20.0,
            video_timeline_start: 5.0,
            media_source_duration: Some(30.0),
            track_row: 0,
            source_node_ids: vec![],
            muted: false,
            locked: false,
        });
        let mut project = ProjectFile::new(doc, NodeStore::default());
        // Mute + lock, then verify locked clips refuse mutation.
        let out = rt
            .execute_with_project(
                "c.lua",
                &format!(
                    r#"
                vblua.video.set_muted("{cid}", true)
                vblua.video.set_locked("{cid}", true)
                local c = vblua.video.get("{cid}")
                return tostring(c.muted) .. tostring(c.locked)
                "#
                ),
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "truetrue");
        assert!(project.document.layers[0].av_clips[0].muted);
        let err = rt
            .execute_with_project(
                "c.lua",
                &format!(r#"return vblua.video.move("{cid}", 99.0)"#),
                Some(&mut project),
            )
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
        assert_eq!(project.document.layers[0].av_clips[0].video_timeline_start, 5.0);
        // Unlock, duplicate appends after the original sharing media.
        rt.execute_with_project(
            "c.lua",
            &format!(r#"vblua.video.set_locked("{cid}", false)"#),
            Some(&mut project),
        )
        .unwrap();
        let out = rt
            .execute_with_project(
                "c.lua",
                &format!(r#"local n = vblua.video.duplicate("{cid}") return #vblua.video.clips()"#),
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "2");
        let clips = &project.document.layers[0].av_clips;
        assert_eq!(clips[1].media_path, "take.mp4");
        assert_eq!(clips[1].video_timeline_start, 25.0);
    }

    #[test]
    fn ripple_delete_closes_gap() {
        use crate::document::{AvClip, Layer, NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut doc = test_doc();
        doc.layers.push(Layer::new_av_layer(
            uuid::Uuid::new_v4(),
            "AV".into(),
            String::new(),
        ));
        for (i, (start, len)) in [(0.0, 10.0), (10.0, 10.0), (20.0, 10.0)].iter().enumerate() {
            doc.layers[0].av_clips.push(AvClip {
                id: uuid::Uuid::new_v4(),
                name: format!("c{i}"),
                media_path: "m.mp4".into(),
                video_start_offset: 0.0,
                video_play_length: *len,
                video_timeline_start: *start,
                media_source_duration: Some(30.0),
                track_row: 0,
                source_node_ids: vec![],
                muted: false,
                locked: false,
            });
        }
        let mut project = ProjectFile::new(doc, NodeStore::default());
        let first = project.document.layers[0].av_clips[0].id.to_string();
        let out = rt
            .execute_with_project(
                "r.lua",
                &format!(
                    r#"
                vblua.video.ripple_delete("{first}")
                local cs = vblua.video.clips()
                return #cs .. ":" .. cs[1].start .. ":" .. cs[2].start
                "#
                ),
                Some(&mut project),
            )
            .unwrap();
        assert!(out == "2:0:10" || out == "2:0.0:10.0", "got {out}");
        let clips = &project.document.layers[0].av_clips;
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].video_timeline_start, 0.0);
        assert_eq!(clips[1].video_timeline_start, 10.0);
    }

    #[test]
    fn timeline_markers_end_to_end() {
        use crate::document::{NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let doc = test_doc();
        let mut project = ProjectFile::new(doc, NodeStore::default());
        // Add two markers out of order; snapshot stays sorted.
        let out = rt
            .execute_with_project(
                "m.lua",
                r#"
                local b = vblua.video.add_marker("Outro", 30.0)
                local a = vblua.video.add_marker("Intro", 5.0)
                local ms = vblua.video.markers()
                vblua.video.move_marker(b, 40.0)
                vblua.video.rename_marker(a, "Start")
                return #ms .. ":" .. ms[1].name .. ":" .. ms[2].name
                "#,
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "2:Intro:Outro");
        let markers = &project.document.timeline_markers;
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[0].name, "Start");
        assert_eq!(markers[1].time_sec, 40.0);
        // Remove one; unknown ids error.
        let out = rt
            .execute_with_project(
                "m.lua",
                r#"
                local ms = vblua.video.markers()
                vblua.video.remove_marker(ms[1].id)
                return #vblua.video.markers()
                "#,
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "1");
        assert_eq!(project.document.timeline_markers.len(), 1);
        let err = rt
            .execute_with_project(
                "m.lua",
                r#"return vblua.video.move_marker("00000000-0000-0000-0000-000000000000", 1.0)"#,
                Some(&mut project),
            )
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
    }

    #[test]
    fn timeremap_curve_param_round_trips() {
        use crate::document::{Layer, NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut doc = test_doc();
        doc.layers.push(Layer::new_node_editor_layer(
            uuid::Uuid::new_v4(),
            "NE".into(),
        ));
        doc.layers[0].ensure_node_graph();
        let mut project = ProjectFile::new(doc, NodeStore::default());
        // Create (identity: empty curve) -> set 2x ramp -> get round-trips canonical.
        let out = rt
            .execute_with_project(
                "t.lua",
                r#"
                local r = vblua.graph.create("TimeRemap", { x = 100, y = 0 })
                assert(#vblua.graph.get_param(r, "curve") == 0)
                vblua.graph.set_param(r, "curve", {{10, 20}, {0, 0}})
                local c = vblua.graph.get_param(r, "curve")
                return #c .. ":" .. c[1][1] .. ":" .. c[1][2] .. ":" .. c[2][1] .. ":" .. c[2][2]
                "#,
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "2:0.0:0.0:10.0:20.0");
        // Bad shapes error (not silent truncation).
        let err = rt
            .execute_with_project(
                "t.lua",
                r#"
                local r = vblua.graph.create("remap")
                return vblua.graph.set_param(r, "curve", {{0}})
                "#,
                Some(&mut project),
            )
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
    }

    #[test]
    fn document_layers_create_get_idempotent() {
        use crate::document::{NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let doc = test_doc();
        let mut project = ProjectFile::new(doc, NodeStore::default());
        // Create twice in one run: same id, one layer. get/layers agree.
        let out = rt
            .execute_with_project(
                "l.lua",
                r#"
                local a = vblua.document.create_layer({name = "Images", type = "image"})
                local b = vblua.document.create_layer({name = "Images", type = "image"})
                assert(a == b)
                local g = vblua.document.get_layer("Images")
                assert(g ~= nil and g.id == a and g.kind == "image")
                assert(vblua.document.get_layer("Missing") == nil)
                return #vblua.document.layers()
                "#,
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "1");
        assert_eq!(project.document.layers.len(), 1);
        assert_eq!(project.document.layers[0].name, "Images");
        // Incompatible type on the same name errors (no duplicate).
        let err = rt
            .execute_with_project(
                "l.lua",
                r#"return vblua.document.create_layer({name = "Images", type = "av"})"#,
                Some(&mut project),
            )
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
        assert_eq!(project.document.layers.len(), 1);
        // Unknown type errors loudly.
        let err = rt
            .execute_with_project(
                "l.lua",
                r#"return vblua.document.create_layer({name = "X", type = "nope"})"#,
                Some(&mut project),
            )
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
    }

    #[test]
    fn editor_object_traversal_and_mutation() {
        use crate::document::{Fill, Layer, Node, NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let mut doc = test_doc();
        doc.layers.push(Layer::new_image(
            uuid::Uuid::new_v4(),
            "L".into(),
            true,
            false,
            vec![],
        ));
        let mut project = ProjectFile::new(doc, NodeStore::default());
        let node = Node::rect(1.0, 2.0, 5.0, 5.0, Fill::None);
        let nid = node.id;
        project.nodes.insert(node);
        project.document.layers[0].nodes.push(nid);
        // Traverse by layer, read detail, mutate, re-read same run.
        let out = rt
            .execute_with_project(
                "o.lua",
                r#"
                local rows = vblua.editor.layer_nodes("L")
                assert(#rows == 1)
                local g = vblua.editor.get(rows[1].id)
                assert(g ~= nil and g.kind == "rect" and g.opacity == 1.0)
                vblua.editor.move_node(g.id, 100, 200)
                vblua.editor.set_opacity(g.id, 0.5)
                local g2 = vblua.editor.get(g.id)
                return g2.x .. ":" .. g2.y .. ":" .. g2.opacity
                "#,
                Some(&mut project),
            )
            .unwrap();
        assert!(out == "100.0:200.0:0.5", "got {out}");
        let n = project.nodes.map.get(&nid).unwrap();
        assert_eq!(n.transform.translation, [100.0, 200.0]);
        assert_eq!(n.style.opacity, 0.5);
        // Unknown layer / node / missing get behave.
        assert!(rt
            .execute_with_project(
                "o.lua",
                r#"return vblua.editor.get("00000000-0000-0000-0000-000000000000")"#,
                Some(&mut project),
            )
            .unwrap()
            == "nil");
        let err = rt
            .execute_with_project(
                "o.lua",
                r#"return vblua.editor.layer_nodes("Nope")"#,
                Some(&mut project),
            )
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
        let err = rt
            .execute_with_project(
                "o.lua",
                r#"return vblua.editor.move_node("00000000-0000-0000-0000-000000000000", 1, 1)"#,
                Some(&mut project),
            )
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
    }

    #[test]
    fn timeline_reads_playback() {
        use crate::document::{NodeStore, ProjectFile};
        let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let doc = test_doc();
        let mut project = ProjectFile::new(doc, NodeStore::default());
        // Frames canonical: 90 @ 30fps = 3.0s.
        rt.set_playback(90, 30);
        let out = rt
            .execute_with_project(
                "t.lua",
                r#"return vblua.timeline.frame() .. ":" .. vblua.timeline.fps() .. ":" .. vblua.timeline.time()"#,
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "90:30:3.0");
        // Defaults without set_playback: frame 0, 60fps.
        let mut rt2 = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
        let out = rt2
            .execute_with_project(
                "t.lua",
                r#"return vblua.timeline.frame() .. ":" .. vblua.timeline.fps() .. ":" .. vblua.timeline.time()"#,
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "0:60:0.0");
        // range() declares the run domain (R3a) and returns true.
        let out = rt
            .execute_with_project(
                "t.lua",
                r#"return vblua.timeline.range(0, 5, "seconds")"#,
                Some(&mut project),
            )
            .unwrap();
        assert_eq!(out, "true");
        let err = rt
            .execute_with_project(
                "t.lua",
                r#"return vblua.timeline.range(5, 1, "seconds")"#,
                Some(&mut project),
            )
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{err:?}");
    }
}
