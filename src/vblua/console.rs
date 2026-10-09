//! VBLua dev console — temporary in-app test surface (NOT the Phase 13 UI API).
//!
//! A single `egui::Window` with a script box, Run/Reset buttons, and output.
//! Writes go through the standard command queue and land in ONE undo entry,
//! exactly like headless `execute_with_document`.

use super::runtime::VbRuntime;
use super::sandbox::SandboxPolicy;
use crate::document::ProjectFile;
use crate::history::{History, ProjectEdit};

/// In-app console state. Lives on `VadadeeBerryApp` (created lazily).
pub struct VbluaConsole {
    runtime: Option<VbRuntime>,
    pub script: String,
    output: Vec<String>,
    init_error: Option<String>,
    picker: Option<super::file::PickerHook>,
    new_timeline_name: String,
}

impl Default for VbluaConsole {
    fn default() -> Self {
        Self {
            runtime: None,
            script: String::from(
                "-- VBLua dev console (document.write granted here; default policy is read-only)\nlocal d = vblua.document.current()\nif d == nil then\n  vblua.log(\"no document open\")\n  return nil\nend\nvblua.log(\"doc: \" .. d.name .. \" \" .. d.width .. \"x\" .. d.height)\nreturn d.name\n",
            ),
            output: vec!["VBLua console — press Run (Ctrl+Enter).".to_string()],
            init_error: None,
            picker: None,
            new_timeline_name: String::new(),
        }
    }
}

impl VbluaConsole {
    /// Install the platform picker hook for future runs (desktop rfd).
    pub fn set_picker(&mut self, picker: Option<super::file::PickerHook>) {
        self.picker = picker.clone();
        if let Some(rt) = self.runtime.as_mut() {
            rt.set_picker(picker);
        }
    }

    fn ensure_runtime(&mut self) -> bool {
        if self.runtime.is_some() {
            return true;
        }
        // Dev console is explicitly permissive (document + picker); the
        // default headless policy stays read-only.
        let mut policy = SandboxPolicy::permissive_for_tests();
        policy.grant(crate::vblua::Capability::FilesystemRead);
        match VbRuntime::new(policy) {
            Ok(rt) => {
                rt.set_picker(self.picker.clone());
                self.runtime = Some(rt);
                true
            }
            Err(e) => {
                self.init_error = Some(format!("VBLua init failed: {e}"));
                self.output.push(format!("VBLua init failed: {e}"));
                false
            }
        }
    }

    fn run(
        &mut self,
        project: &mut ProjectFile,
        history: &mut History,
        playback_frame: usize,
        playback_fps: u32,
    ) {
        if !self.ensure_runtime() {
            return;
        }
        let before = Before::capture(project);
        let result = {
            let rt = self.runtime.as_mut().unwrap();
            rt.set_playback(playback_frame, playback_fps);
            rt.execute_with_project("console.lua", &self.script.clone(), Some(project))
        };
        // A run that declares a timeline range auto-registers the console
        // script as the "Console" timeline entry (upsert, undo-coalesced) —
        // no separate panel trip needed. Plain runs leave the registry alone.
        {
            let rt = self.runtime.as_mut().unwrap();
            let (_, declared) = rt.take_timeline_marks();
            if declared.is_some() {
                self.upsert_console_entry(project, history);
            }
        }
        // Drain script console lines into the output view.
        {
            let rt = self.runtime.as_mut().unwrap();
            self.output.extend(rt.console.drain(..));
        }
        match result {
            Ok(out) => {
                self.output.push(format!("=> {out}"));
                let report = self.runtime.as_mut().unwrap().take_node_report();
                self.commit(project, history, before, report);
            }
            Err(e) => self.output.push(e.render()),
        }
        if self.output.len() > 300 {
            let drain = self.output.len() - 300;
            self.output.drain(..drain);
        }
    }

    /// Register the console script as the "Console" timeline entry.
    /// Same source twice = no-op (no undo flood while iterating).
    fn upsert_console_entry(&mut self, project: &mut ProjectFile, history: &mut History) {
        const NAME: &str = "Console";
        if let Some(s) = project
            .document
            .timeline_scripts
            .iter()
            .find(|s| s.name == NAME)
        {
            if s.source == self.script {
                return;
            }
        }
        if project.document.timeline_scripts.len() >= 32
            && !project
                .document
                .timeline_scripts
                .iter()
                .any(|s| s.name == NAME)
        {
            self.output
                .push("(timeline registry full — delete a script first)".to_string());
            return;
        }
        let before = project.document.clone();
        if let Some(s) = project
            .document
            .timeline_scripts
            .iter_mut()
            .find(|s| s.name == NAME)
        {
            s.source = self.script.chars().take(65536).collect();
        } else {
            project
                .document
                .timeline_scripts
                .push(crate::document::TimelineScript::new(NAME, self.script.clone()));
        }
        let after = project.document.clone();
        history.push_applied(project, ProjectEdit::PatchDocument { before, after });
        self.output.push(
            "(registered as timeline script \"Console\" — plays with the timeline)".to_string(),
        );
    }

    /// Record undo entries for whatever a script run / UI callback changed.
    /// One batch in = one undo entry per store out.
    fn commit(
        &mut self,
        project: &mut ProjectFile,
        history: &mut History,
        before: Before,
        report: crate::vblua::runtime::NodeReport,
    ) {
        if project.document.title != before.doc.title
            || project.document.width != before.doc.width
            || project.document.height != before.doc.height
            || project.document.layers.len() != before.doc.layers.len()
            || graph_signature(&project.document) != before.graphs
            || shader_signature(&project.document) != before.shaders
            || video_signature(&project.document) != before.video
        {
            let after = project.document.clone();
            history.push_applied(
                project,
                ProjectEdit::PatchDocument {
                    before: before.doc,
                    after,
                },
            );
            self.output.push("(document changed — 1 undo entry)".to_string());
        }
        if super::animation::timeline_signature(&project.anim_timeline)
            != super::animation::timeline_signature(&before.tl)
        {
            let after = project.anim_timeline.clone();
            history.push_applied(
                project,
                ProjectEdit::PatchTimeline {
                    before: before.tl,
                    after,
                },
            );
            self.output.push("(timeline changed — 1 undo entry)".to_string());
        }
        // Created nodes (imports, duplicates): one undo entry.
        if !report.created_nodes.is_empty() {
            history.push_applied(
                project,
                ProjectEdit::InsertNodesApplied {
                    nodes: report.created_nodes,
                },
            );
            self.output.push("(nodes added — 1 undo entry)".to_string());
        }
        // Deleted nodes: exact RemoveNodes entries (content + anim + layer).
        for r in report.removed {
            let anims = r
                .anim
                .map(|a| vec![(r.node.id, a)])
                .unwrap_or_default();
            history.push_applied(
                project,
                ProjectEdit::RemoveNodes {
                    removed: vec![(r.node.id, r.node)],
                    removed_anims: anims,
                    layer_index: r.layer_index,
                    layer_nodes_before: r.layer_nodes_before,
                    ne_proxy_before: vec![],
                },
            );
            self.output.push("(nodes removed — 1 undo entry)".to_string());
        }
    }

    /// Render Lua extension panels + run their callbacks (Phase 13 host side).
    /// No panels declared draws nothing.
    pub fn show_panels(
        &mut self,
        project: &mut ProjectFile,
        history: &mut History,
        ctx: &egui::Context,
    ) {
        if self.panel_count() == 0 {
            return;
        }
        // Collect widget interactions first (no project borrow yet).
        enum Pending {
            Click(u64),
            Num(u64, f64),
            Bool(u64, bool),
        }
        let mut pending: Vec<Pending> = Vec::new();
        egui::Window::new("VBLua Panels (extensions)")
            .default_size([300.0, 400.0])
            .show(ctx, |ui| {
                let rt = self.runtime.as_ref().unwrap();
                for panel in rt.host_panels_snapshot() {
                    ui.collapsing(&panel.title, |ui| {
                        for w in panel.widgets {
                            match &w.kind {
                                super::ui_widgets::UiWidgetKind::Text { content } => {
                                    ui.label(content);
                                }
                                super::ui_widgets::UiWidgetKind::Button { label } => {
                                    if ui.button(label).clicked() {
                                        pending.push(Pending::Click(w.id));
                                    }
                                }
                                super::ui_widgets::UiWidgetKind::Slider { label, min, max } => {
                                    let mut v = w.num_value;
                                    if ui.add(egui::Slider::new(&mut v, *min..=*max).text(label)).changed() {
                                        pending.push(Pending::Num(w.id, v));
                                    }
                                }
                                super::ui_widgets::UiWidgetKind::Checkbox { label } => {
                                    let mut v = w.bool_value;
                                    if ui.checkbox(&mut v, label).changed() {
                                        pending.push(Pending::Bool(w.id, v));
                                    }
                                }
                            }
                        }
                    });
                }
            });
        if pending.is_empty() {
            return;
        }
        let before = Before::capture(project);
        {
            let rt = self.runtime.as_mut().unwrap();
            for p in pending {
                match p {
                    Pending::Click(id) => {
                        rt.ui_push_event(super::ui_widgets::UiEvent::Clicked(id))
                    }
                    Pending::Num(id, v) => {
                        rt.ui_set_num(id, v);
                        rt.ui_push_event(super::ui_widgets::UiEvent::NumChanged(id, v))
                    }
                    Pending::Bool(id, v) => {
                        rt.ui_set_bool(id, v);
                        rt.ui_push_event(super::ui_widgets::UiEvent::BoolChanged(id, v))
                    }
                }
            }
            rt.poll_ui_callbacks(Some(project));
            self.output.extend(rt.console.drain(..));
            let report = rt.take_node_report();
            self.commit(project, history, before, report);
        }
    }

    fn panel_count(&self) -> usize {
        self.runtime
            .as_ref()
            .map(|rt| rt.ui_panel_count())
            .unwrap_or(0)
    }

    /// Fire a host lifecycle event from the application (open/export hooks).
    /// No-op when the runtime was never created (zero cost when unused).
    pub fn fire_event(
        &mut self,
        project: &mut ProjectFile,
        history: &mut History,
        event: &str,
    ) {
        if self.runtime.is_none() {
            return;
        }
        let before = Before::capture(project);
        let result = {
            let rt = self.runtime.as_mut().unwrap();
            rt.fire_app_event(project, event)
        };
        if let Err(e) = result {
            self.output.push(e.render());
            return;
        }
        let report = self.runtime.as_mut().unwrap().take_node_report();
        {
            let rt = self.runtime.as_mut().unwrap();
            self.output.extend(rt.console.drain(..));
        }
        self.commit(project, history, before, report);
    }

    /// Draw the floating window. Call with full `app` access from `chrome()`.
    pub fn show_window(
        &mut self,
        open: &mut bool,
        project: &mut ProjectFile,
        history: &mut History,
        playback_frame: usize,
        playback_fps: u32,
        ctx: &egui::Context,
    ) {
        if !*open {
            return;
        }
        let mut run_pressed = false;
        let mut reset_pressed = false;
        egui::Window::new("VBLua Console (dev)")
            .default_size([460.0, 380.0])
            .show(ctx, |ui| {
                ui.label("Script (Ctrl+Enter = Run):");
                ui.add(
                    egui::TextEdit::multiline(&mut self.script)
                        .desired_rows(10)
                        .code_editor()
                        .desired_width(f32::INFINITY),
                );
                ui.horizontal(|ui| {
                    if ui.button("Run").clicked() {
                        run_pressed = true;
                    }
                    if ui.button("Reset runtime").clicked() {
                        reset_pressed = true;
                    }
                    if ui.button("Clear output").clicked() {
                        self.output.clear();
                    }
                });
                if ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.ctrl) {
                    run_pressed = true;
                }
                ui.separator();
                ui.label("Output:");
                egui::ScrollArea::vertical()
                    .id_salt("vblua_console_output")
                    .max_height(160.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.set_max_width(ui.available_width());
                        for line in &self.output {
                            ui.label(line);
                        }
                    });
            });
        if reset_pressed
            && let Some(rt) = self.runtime.as_mut()
        {
            match rt.reset() {
                Ok(()) => self.output.push("(runtime reset)".to_string()),
                Err(e) => self.output.push(format!("Reset failed: {e}")),
            }
        }
        if run_pressed {
            self.run(project, history, playback_frame, playback_fps);
        }
    }

/// Timeline Scripts panel (R3b): registry list, recorded ranges, ACTIVE state,
/// enable toggles, delete, and add-current-console-script. CRUD is undoable.
pub fn show_timeline_window(
    &mut self,
    open: &mut bool,
    project: &mut ProjectFile,
    history: &mut History,
    states: &mut std::collections::HashMap<uuid::Uuid, super::timeline_eval::ScriptEvalState>,
    errors: &[(String, String)],
    frame: usize,
    fps: u32,
    ctx: &egui::Context,
) {
    if !*open {
        return;
    }
    enum Act {
        Toggle(uuid::Uuid, bool),
        Delete(uuid::Uuid),
        Add,
    }
    let mut act: Option<Act> = None;
    egui::Window::new("Timeline Scripts")
        .id(egui::Id::new("timeline_scripts_panel_v2"))
        .open(open)
        .default_width(340.0)
        .max_width(380.0)
        .show(ctx, |ui| {
        ui.label("Scripts evaluate against the playhead on frame changes (no undo entries).");
        ui.label("Console runs declaring timeline.range() register here as \"Console\" automatically.");
        ui.separator();
        let rows: Vec<(uuid::Uuid, String, bool)> = project
            .document
            .timeline_scripts
            .iter()
            .map(|s| (s.id, s.name.clone(), s.enabled))
            .collect();
        if rows.is_empty() {
            ui.label("No timeline scripts. Add the current console script below.");
        }
        for (id, name, enabled) in &rows {
            ui.horizontal(|ui| {
                let mut en = *enabled;
                if ui.checkbox(&mut en, "").changed() {
                    act = Some(Act::Toggle(*id, en));
                }
                let short: String = name.chars().take(32).collect();
                ui.label(short);
                let st = states.get(id);
                let (range_text, active) = match st {
                    None => ("unrun".to_string(), false),
                    Some(s) if !s.temporal => ("static (ran once)".to_string(), false),
                    Some(s) => match s.range {
                        None => ("temporal (no range)".to_string(), *enabled),
                        Some((a, b, f)) => {
                            let unit = if f { "frames" } else { "seconds" };
                            let on = *enabled
                                && super::timeline_eval::range_contains((a, b, f), frame, fps);
                            (format!("{a}..{b} {unit}"), on)
                        }
                    },
                };
                ui.label(range_text);
                if active {
                    ui.label("ACTIVE");
                }
                if ui.button("Delete").clicked() {
                    act = Some(Act::Delete(*id));
                }
            });
        }
        ui.separator();
        ui.label("Name");
        ui.text_edit_singleline(&mut self.new_timeline_name);
        if ui.button("Add current console script").clicked() {
            act = Some(Act::Add);
        }
        if !errors.is_empty() {
            ui.separator();
            ui.label("Last eval errors:");
            for (name, msg) in errors.iter().take(5) {
                ui.label(format!("{name}: {msg}"));
            }
        }
    });
    match act {
        None => {}
        Some(Act::Toggle(id, en)) => {
            let before = project.document.clone();
            if let Some(s) = project.document.timeline_scripts.iter_mut().find(|s| s.id == id) {
                s.enabled = en;
                let after = project.document.clone();
                history.push_applied(project, ProjectEdit::PatchDocument { before, after });
            }
        }
        Some(Act::Delete(id)) => {
            let before = project.document.clone();
            let n0 = project.document.timeline_scripts.len();
            project.document.timeline_scripts.retain(|s| s.id != id);
            if project.document.timeline_scripts.len() != n0 {
                let after = project.document.clone();
                history.push_applied(project, ProjectEdit::PatchDocument { before, after });
                states.remove(&id);
            }
        }
        Some(Act::Add) => {
            if project.document.timeline_scripts.len() >= 32 {
                return;
            }
            let name = if self.new_timeline_name.trim().is_empty() {
                format!("Script {}", project.document.timeline_scripts.len() + 1)
            } else {
                self.new_timeline_name.chars().take(128).collect()
            };
            let before = project.document.clone();
            project
                .document
                .timeline_scripts
                .push(crate::document::TimelineScript::new(name, self.script.clone()));
            let after = project.document.clone();
            history.push_applied(project, ProjectEdit::PatchDocument { before, after });
            self.new_timeline_name.clear();
        }
    }
}
}

/// (clips,) per AV layer — cheap change detector for undo.
fn video_signature(doc: &crate::document::Document) -> Vec<(usize, usize)> {
    doc.layers
        .iter()
        .filter(|l| l.kind == crate::document::LayerKind::AV)
        .map(|l| {
            (
                l.av_clips.len(),
                l.av_clips
                    .iter()
                    .map(|c| (c.video_timeline_start * 10.0) as i32 as usize)
                    .sum(),
            )
        })
        .collect()
}

/// (passes, enabled) per layer — cheap change detector for undo.
fn shader_signature(doc: &crate::document::Document) -> Vec<(usize, usize)> {
    doc.layers
        .iter()
        .map(|l| {
            (
                l.shading_passes.len(),
                l.shading_passes.iter().filter(|p| p.enabled).count(),
            )
        })
        .collect()
}

/// Pre-execution snapshot for undo detection (console + panel callbacks).
struct Before {
    doc: crate::document::Document,
    graphs: Vec<(usize, usize)>,
    shaders: Vec<(usize, usize)>,
    video: Vec<(usize, usize)>,
    tl: crate::document::AnimationTimeline,
}

impl Before {
    fn capture(project: &ProjectFile) -> Self {
        Self {
            doc: project.document.clone(),
            graphs: graph_signature(&project.document),
            shaders: shader_signature(&project.document),
            video: video_signature(&project.document),
            tl: project.anim_timeline.clone(),
        }
    }
}

/// (nodes, links) per NodeEditor layer — cheap change detector for undo.
fn graph_signature(doc: &crate::document::Document) -> Vec<(usize, usize)> {
    doc.layers
        .iter()
        .filter(|l| l.kind == crate::document::LayerKind::NodeEditor)
        .map(|l| {
            l.node_graph
                .as_ref()
                .map(|g| (g.nodes.len(), g.links.len()))
                .unwrap_or((0, 0))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

    fn test_project() -> ProjectFile {
        Document::new_empty_project()
    }

    #[test]
    fn run_with_range_registers_console_entry_once() {
        let mut console = VbluaConsole::default();
        console.script = "vblua.timeline.range(0, 5, \"seconds\")\nreturn 1".into();
        let mut project = test_project();
        let mut history = History::default();
        console.run(&mut project, &mut history, 0, 60);
        assert_eq!(project.document.timeline_scripts.len(), 1);
        assert_eq!(project.document.timeline_scripts[0].name, "Console");
        // Identical re-run: still one entry, and a single undo clears it.
        console.run(&mut project, &mut history, 0, 60);
        assert_eq!(project.document.timeline_scripts.len(), 1);
        assert!(history.undo(&mut project));
        assert!(project.document.timeline_scripts.is_empty());
    }

    #[test]
    fn plain_run_leaves_registry_alone() {
        let mut console = VbluaConsole::default();
        console.script = "return 1 + 1".into();
        let mut project = test_project();
        let mut history = History::default();
        console.run(&mut project, &mut history, 0, 60);
        assert!(project.document.timeline_scripts.is_empty());
    }
}
