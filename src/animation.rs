//! UI motion via [kramaframe](https://github.com/CosmoBunny/kramaframe).
use egui::{Color32, Context, Rect};
use kramaframe::keylist::TRES16Bits;
use kramaframe::prelude::{KeyFrameFunction, KeyList};
use kramaframe::{BTclasslist, BTframelist, KramaFrame};

use crate::tools::ToolKind;
use crate::ui::ActionTab;

const ID: u32 = 0;

/// Horizontal slide duration for the action bar.
pub const ACTION_BAR_SLIDE_SECS: f32 = 0.48;
/// Fixed simulation step (decoupled from wall clock).
const ACTION_BAR_MAX_DT: f32 = 1.0 / 60.0;
/// Max catch-up steps if the event loop was idle (prevents one-frame completion).
const ACTION_BAR_MAX_STEPS_PER_FRAME: u32 = 3;

pub struct UiAnimation {
    engine: KramaFrame<BTclasslist, BTframelist<TRES16Bits, i16>>,
    prev_action_bar_open: bool,
    action_bar_t: f32,
    action_bar_from: f32,
    action_bar_to: f32,
    action_bar_elapsed: f32,
    action_bar_running: bool,
    prev_action_tab: ActionTab,
    prev_tool: ToolKind,
    prev_status_message: String,
    pub status_tool_outgoing: String,
    pub status_tool_incoming: String,
    status_tool_width_out: f32,
    status_tool_width_in: f32,
    status_tool_width_settled: f32,
    pub status_msg_outgoing: String,
    pub status_msg_incoming: String,
    status_msg_width_out: f32,
    status_msg_width_in: f32,
    status_msg_width_settled: f32,
    status_slide_distance: f32,
    prev_coords_text: String,
    pub coords_outgoing: String,
    pub coords_incoming: String,
    coords_width_out: f32,
    coords_width_in: f32,
    coords_width_settled: f32,
    prev_has_coords: bool,
    coords_from_w: f32,
    coords_to_w: f32,
    prev_on_path_offer: bool,
    prev_on_path_container: bool,
    on_path_container_from: f32,
    on_path_container_to: f32,
    status_tool_target_in: bool,
    status_msg_target_in: bool,
    coords_target_in: bool,
    prev_show_timeline: bool,
    pub timeline_t: f32,
    timeline_from: f32,
    timeline_to: f32,
    timeline_elapsed: f32,
    pub timeline_running: bool,
}

impl Default for UiAnimation {
    fn default() -> Self {
        Self::new()
    }
}

impl UiAnimation {
    pub fn new() -> Self {
        let mut engine = KramaFrame::default();
        engine.extend_iter_classlist([
            (
                "slide",
                KeyFrameFunction::new_cubic_bezier_f32(1.0, 0.0, 0.6, 1.0),
            ),
            ("fade", KeyFrameFunction::EaseInOut),
            ("ease", KeyFrameFunction::Ease),
            ("easeout", KeyFrameFunction::EaseOut),
            ("tab", KeyFrameFunction::EaseOut),
            ("pulse", KeyFrameFunction::Quadratic),
            ("intro_toolbar", KeyFrameFunction::EaseOut),
            ("intro_menubar", KeyFrameFunction::EaseOut),
            ("intro_status", KeyFrameFunction::EaseOut),
            ("intro_canvas", KeyFrameFunction::EaseInOut),
            ("tool_pulse", KeyFrameFunction::EaseOut),
            ("tab_fade", KeyFrameFunction::EaseOut),
            ("tab_slide", KeyFrameFunction::new_cubic_bezier_f32(1.0, 0.0, 0.6, 1.0)),
            ("status_sign", KeyFrameFunction::EaseInOut),
            ("status_tool_sign", KeyFrameFunction::EaseInOut),
            ("coords_sign", KeyFrameFunction::EaseInOut),
            ("coords_presence", KeyFrameFunction::EaseInOut),
            (
                "on_path_offer",
                KeyFrameFunction::new_cubic_bezier_f32(0.34, 1.45, 0.64, 1.0),
            ),
            ("on_path_container", KeyFrameFunction::EaseOut),
        ]);
        engine.framelist.extend([
            ("slide", KeyList::new(ID, TRES16Bits::from_millis(280))),
            ("fade", KeyList::new(ID, TRES16Bits::from_millis(220))),
            ("ease", KeyList::new(ID, TRES16Bits::from_millis(200))),
            ("easeout", KeyList::new(ID, TRES16Bits::from_millis(200))),
            ("tab", KeyList::new(ID, TRES16Bits::from_millis(180))),
            ("pulse", KeyList::new(ID, TRES16Bits::from_millis(240))),
            (
                "intro_toolbar",
                KeyList::new(ID, TRES16Bits::from_millis(420)),
            ),
            (
                "intro_menubar",
                KeyList::new(ID, TRES16Bits::from_millis(360)),
            ),
            (
                "intro_status",
                KeyList::new(ID, TRES16Bits::from_millis(340)),
            ),
            (
                "intro_canvas",
                KeyList::new(ID, TRES16Bits::from_millis(520)),
            ),
            (
                "tool_pulse",
                KeyList::new(ID, TRES16Bits::from_millis(260)),
            ),
            ("tab_fade", KeyList::new(ID, TRES16Bits::from_millis(200))),
            ("tab_slide", KeyList::new(ID, TRES16Bits::from_millis(220))),
            ("status_sign", KeyList::new(ID, TRES16Bits::from_millis(360))),
            (
                "status_tool_sign",
                KeyList::new(ID, TRES16Bits::from_millis(360)),
            ),
            ("coords_sign", KeyList::new(ID, TRES16Bits::from_millis(360))),
            ("coords_presence", KeyList::new(ID, TRES16Bits::from_millis(300))),
            ("on_path_offer", KeyList::new(ID, TRES16Bits::from_millis(320))),
            (
                "on_path_container",
                KeyList::new(ID, TRES16Bits::from_millis(380)),
            ),
        ]);

        let mut anim = Self {
            engine,
            prev_action_bar_open: true,
            action_bar_t: 0.0,
            action_bar_from: 0.0,
            action_bar_to: 1.0,
            action_bar_elapsed: 0.0,
            action_bar_running: false,
            prev_action_tab: ActionTab::default(),
            prev_tool: ToolKind::Select,
            prev_status_message: String::new(),
            status_tool_outgoing: "Select".into(),
            status_tool_incoming: "Select".into(),
            status_tool_width_out: 56.0,
            status_tool_width_in: 56.0,
            status_tool_width_settled: 56.0,
            status_msg_outgoing: String::new(),
            status_msg_incoming: String::new(),
            status_msg_width_out: 80.0,
            status_msg_width_in: 80.0,
            status_msg_width_settled: 80.0,
            status_slide_distance: 120.0,
            prev_coords_text: "...".into(),
            coords_outgoing: "...".into(),
            coords_incoming: "...".into(),
            coords_width_out: 120.0,
            coords_width_in: 120.0,
            coords_width_settled: 120.0,
            prev_has_coords: false,
            coords_from_w: 0.0,
            coords_to_w: 0.0,
            prev_on_path_offer: false,
            prev_on_path_container: false,
            on_path_container_from: 0.0,
            on_path_container_to: 0.0,
            status_tool_target_in: true,
            status_msg_target_in: true,
            coords_target_in: true,
            prev_show_timeline: false,
            timeline_t: 0.0,
            timeline_from: 0.0,
            timeline_to: 0.0,
            timeline_elapsed: 0.0,
            timeline_running: false,
        };
        anim.play_intro();
        anim
    }

    pub fn tick(&mut self, ctx: &Context) {
        let dt_ms = (ctx.input(|i| i.stable_dt) * 1000.0).clamp(1.0, 48.0) as u16;
        self.engine
            .update_progress(TRES16Bits::from_millis(dt_ms));
        if !self.engine.is_animating("status_sign", ID) {
            self.status_msg_width_settled = if self.status_msg_target_in {
                self.status_msg_width_in
            } else {
                self.status_msg_width_out
            };
        }
        if !self.engine.is_animating("status_tool_sign", ID) {
            self.status_tool_width_settled = if self.status_tool_target_in {
                self.status_tool_width_in
            } else {
                self.status_tool_width_out
            };
        }
        if !self.engine.is_animating("coords_sign", ID) {
            self.coords_width_settled = if self.coords_target_in {
                self.coords_width_in
            } else {
                self.coords_width_out
            };
        }
        if !self.engine.is_animating("coords_presence", ID) {
            self.coords_from_w = self.coords_to_w;
        }
        if !self.engine.is_animating("on_path_container", ID) {
            self.on_path_container_from = self.on_path_container_to;
        }
    }

    fn begin_timeline_slide(&mut self, to: f32) {
        self.timeline_from = self.timeline_t;
        self.timeline_to = to.clamp(0.0, 1.0);
        self.timeline_elapsed = 0.0;
        self.timeline_running = true;
    }

    pub fn advance_timeline_slide(&mut self, ctx: &Context) {
        if !self.timeline_running {
            return;
        }
        let raw_dt = ctx.input(|i| i.unstable_dt).max(0.0);
        let steps = ((raw_dt / ACTION_BAR_MAX_DT).ceil() as u32)
            .clamp(1, ACTION_BAR_MAX_STEPS_PER_FRAME);
        for _ in 0..steps {
            self.timeline_elapsed += ACTION_BAR_MAX_DT;
            self.apply_timeline_pose();
            if !self.timeline_running {
                break;
            }
        }
    }

    fn apply_timeline_pose(&mut self) {
        let u = (self.timeline_elapsed / ACTION_BAR_SLIDE_SECS).min(1.0);
        self.timeline_t =
            self.timeline_from + (self.timeline_to - self.timeline_from) * u;
        if u >= 1.0 {
            self.timeline_t = self.timeline_to;
            self.timeline_running = false;
        }
    }

    fn settle_timeline_pose(&mut self, show_timeline: bool) {
        if self.timeline_running {
            return;
        }
        let target = if show_timeline { 1.0 } else { 0.0 };
        self.timeline_t = target;
    }

    fn begin_action_bar_slide(&mut self, to: f32) {
        self.action_bar_from = self.action_bar_t;
        self.action_bar_to = to.clamp(0.0, 1.0);
        self.action_bar_elapsed = 0.0;
        self.action_bar_running = true;
    }

    pub fn advance_action_bar_slide(&mut self, ctx: &Context) {
        if !self.action_bar_running {
            return;
        }
        let raw_dt = ctx.input(|i| i.unstable_dt).max(0.0);
        let steps = ((raw_dt / ACTION_BAR_MAX_DT).ceil() as u32)
            .clamp(1, ACTION_BAR_MAX_STEPS_PER_FRAME);
        for _ in 0..steps {
            self.action_bar_elapsed += ACTION_BAR_MAX_DT;
            self.apply_action_bar_pose();
            if !self.action_bar_running {
                break;
            }
        }
    }

    fn apply_action_bar_pose(&mut self) {
        let u = (self.action_bar_elapsed / ACTION_BAR_SLIDE_SECS).min(1.0);
        // Linear travel so the panel fully clears the work area (easing only for fade).
        self.action_bar_t =
            self.action_bar_from + (self.action_bar_to - self.action_bar_from) * u;
        if u >= 1.0 {
            self.action_bar_t = self.action_bar_to;
            self.action_bar_running = false;
        }
    }

    fn settle_action_bar_pose(&mut self, action_bar_open: bool) {
        if self.action_bar_running {
            return;
        }
        let target = if action_bar_open { 1.0 } else { 0.0 };
        self.action_bar_t = target;
    }

    pub fn sync(
        &mut self,
        action_bar_open: bool,
        show_timeline: bool,
        active_tool: ToolKind,
        action_tab: ActionTab,
        status_message: &str,
        status_message_width: f32,
        tool_label_width: f32,
        coords_text: &str,
        coords_width: f32,
    ) {
        if action_bar_open != self.prev_action_bar_open {
            self.begin_action_bar_slide(if action_bar_open { 1.0 } else { 0.0 });
            self.prev_action_bar_open = action_bar_open;
        } else {
            self.settle_action_bar_pose(action_bar_open);
        }
        if show_timeline != self.prev_show_timeline {
            self.begin_timeline_slide(if show_timeline { 1.0 } else { 0.0 });
            self.prev_show_timeline = show_timeline;
        } else {
            self.settle_timeline_pose(show_timeline);
        }
        if active_tool != self.prev_tool {
            let label = active_tool.label();
            let is_reverse = if self.status_tool_target_in {
                label == self.status_tool_outgoing
            } else {
                label == self.status_tool_incoming
            };

            if is_reverse {
                self.engine.reverse_animate("status_tool_sign", ID);
                self.status_tool_target_in = !self.status_tool_target_in;
            } else {
                self.status_tool_outgoing = self.prev_tool.label().to_owned();
                self.status_tool_incoming = active_tool.label().to_owned();
                self.status_tool_width_out = self.status_tool_width_settled;
                self.status_tool_width_in = tool_label_width;
                self.engine.restart_progress("status_tool_sign", ID);
                self.status_tool_target_in = true;
            }
            self.engine.restart_progress("tool_pulse", ID);
            self.prev_tool = active_tool;
        }
        if action_tab != self.prev_action_tab {
            self.prev_action_tab = action_tab;
        }
        if status_message != self.prev_status_message {
            self.status_msg_outgoing = self.prev_status_message.clone();
            self.status_msg_incoming = status_message.to_owned();
            self.status_msg_width_out = self.status_msg_width_settled;
            self.status_msg_width_in = status_message_width;
            self.status_slide_distance = self.status_msg_width_out.max(status_message_width) + 40.0;
            self.engine.restart_progress("status_sign", ID);
            self.status_msg_target_in = true;
            self.prev_status_message = status_message.to_owned();
        }
        if coords_text != self.prev_coords_text {
            let is_reverse = if self.coords_target_in {
                coords_text == self.coords_outgoing
            } else {
                coords_text == self.coords_incoming
            };

            if is_reverse {
                self.engine.reverse_animate("coords_sign", ID);
                self.coords_target_in = !self.coords_target_in;
            } else {
                self.coords_outgoing = self.prev_coords_text.clone();
                self.coords_incoming = coords_text.to_owned();
                self.coords_width_out = self.coords_width_settled;
                self.coords_width_in = coords_width;
                self.engine.restart_progress("coords_sign", ID);
                self.coords_target_in = true;
            }
            self.prev_coords_text = coords_text.to_owned();
        }
        let has_coords = coords_width > 1.0;
        if has_coords != self.prev_has_coords {
            self.coords_from_w = if has_coords { 0.0 } else { self.coords_width_settled.max(self.coords_to_w) };
            self.coords_to_w = if has_coords { coords_width } else { 0.0 };
            self.engine.restart_progress("coords_presence", ID);
            self.prev_has_coords = has_coords;
        }
        self.coords_width_in = coords_width;
    }

    pub fn sync_on_path(&mut self, offer_visible: bool, container_visible: bool) {
        if offer_visible && !self.prev_on_path_offer {
            self.engine.restart_progress("on_path_offer", ID);
        }
        if !offer_visible {
            self.engine.set_progress_max("on_path_offer", ID);
        }
        self.prev_on_path_offer = offer_visible;

        if container_visible != self.prev_on_path_container {
            self.on_path_container_from = self.on_path_container_expand();
            self.on_path_container_to = if container_visible { 1.0 } else { 0.0 };
            self.engine.restart_progress("on_path_container", ID);
            self.prev_on_path_container = container_visible;
        }
    }

    pub fn on_path_offer_pop(&self) -> f32 {
        if !self.prev_on_path_offer {
            return 0.0;
        }
        self.range_inclusive("on_path_offer", 0.0, 1.0)
    }

    pub fn on_path_container_expand(&self) -> f32 {
        if self.engine.is_animating("on_path_container", ID) {
            let t = self.range_inclusive("on_path_container", 0.0, 1.0);
            self.on_path_container_from
                + (self.on_path_container_to - self.on_path_container_from) * t
        } else {
            self.on_path_container_to
        }
    }

    pub fn on_path_container_alpha(&self) -> f32 {
        egui::emath::easing::cubic_out(self.on_path_container_expand())
    }

    /// First tab in the strip: full fade + slide-in for panel content.
    pub fn on_tab_change(&mut self) {
        self.engine.restart_progress("tab", ID);
        self.engine.restart_progress("tab_fade", ID);
        self.engine.restart_progress("tab_slide", ID);
    }

    /// Second/third (and beyond): cross-fade only — no slide toward first position.
    pub fn on_tab_change_secondary(&mut self) {
        self.engine.restart_progress("tab", ID);
        self.engine.restart_progress("tab_fade", ID);
        self.engine.set_progress_max("tab_slide", ID);
    }

    pub fn is_active(&self) -> bool {
        self.needs_repaint()
    }

    /// True while a visible transition still needs another frame. Avoid calling
    /// `request_repaint` when this is false so the GPU can idle.
    pub fn needs_repaint(&self) -> bool {
        if self.action_bar_running || self.timeline_running {
            return true;
        }
        const TRACKS: &[&str] = &[
            "intro_toolbar",
            "intro_menubar",
            "intro_status",
            "intro_canvas",
            "status_sign",
            "status_tool_sign",
            "coords_sign",
            "coords_presence",
            "tab_fade",
            "tab_slide",
            "tool_pulse",
            "on_path_offer",
            "on_path_container",
        ];
        TRACKS.iter().any(|class| self.engine.is_animating(class, ID))
    }

    pub fn action_bar_slide_running(&self) -> bool {
        self.action_bar_running
    }

    pub fn action_bar_open_t(&self) -> f32 {
        self.action_bar_t.clamp(0.0, 1.0)
    }

    /// Panel opacity (eased); goes fully transparent when hidden.
    pub fn action_bar_opacity(&self) -> f32 {
        egui::emath::easing::cubic_out(self.action_bar_open_t())
    }

    fn range_inclusive(&self, class: &'static str, lo: f32, hi: f32) -> f32 {
        self.engine
            .get_value_byrange_inclusive(class, ID, lo..=hi)
    }

    pub fn menubar_alpha(&self) -> f32 {
        self.range_inclusive("intro_menubar", 0.0, 1.0)
    }

    pub fn toolbar_alpha(&self) -> f32 {
        self.range_inclusive("intro_toolbar", 0.0, 1.0)
    }

    pub fn status_alpha(&self) -> f32 {
        self.range_inclusive("intro_status", 0.0, 1.0)
    }

    pub fn canvas_alpha(&self) -> f32 {
        self.range_inclusive("intro_canvas", 0.0, 1.0)
    }

    pub fn tab_content_alpha(&self) -> f32 {
        self.range_inclusive("tab_fade", 0.72, 1.0)
    }

    pub fn tab_content_offset(&self) -> f32 {
        self.range_inclusive("tab_slide", 10.0, 0.0)
    }

    pub fn tool_highlight(&self) -> f32 {
        self.range_inclusive("tool_pulse", 0.0, 1.0)
    }

    pub fn tab_label_alpha(&self, selected: bool) -> f32 {
        if selected {
            self.range_inclusive("tab", 0.82, 1.0)
        } else {
            0.72
        }
    }

    pub fn status_slide_out(&self) -> f32 {
        -self.range_inclusive("status_sign", 0.0, self.status_slide_distance)
    }

    pub fn status_slide_in(&self) -> f32 {
        self.range_inclusive("status_sign", self.status_slide_distance, 0.0)
    }

    pub fn status_tool_slide_out(&self, span: f32) -> f32 {
        -self.range_inclusive("status_tool_sign", 0.0, span)
    }

    pub fn status_tool_slide_in(&self, span: f32) -> f32 {
        self.range_inclusive("status_tool_sign", span, 0.0)
    }

    pub fn status_tool_seg_width(&self) -> f32 {
        if self.engine.is_animating("status_tool_sign", ID) {
            self.range_inclusive(
                "status_tool_sign",
                self.status_tool_width_out,
                self.status_tool_width_in,
            )
        } else {
            self.status_tool_width_settled
        }
    }

    pub fn status_message_seg_width(&self) -> f32 {
        if self.engine.is_animating("status_sign", ID) {
            self.range_inclusive(
                "status_sign",
                self.status_msg_width_out,
                self.status_msg_width_in,
            )
        } else {
            self.status_msg_width_settled
        }
    }

    pub fn coords_slide_out(&self, span: f32) -> f32 {
        -self.range_inclusive("coords_sign", 0.0, span)
    }

    pub fn coords_slide_in(&self, span: f32) -> f32 {
        self.range_inclusive("coords_sign", span, 0.0)
    }

    pub fn coords_seg_width(&self) -> f32 {
        if self.engine.is_animating("coords_presence", ID) {
            let t = self.range_inclusive("coords_presence", 0.0, 1.0);
            self.coords_from_w + (self.coords_to_w - self.coords_from_w) * t
        } else {
            self.coords_to_w
        }
    }

    fn play_intro(&mut self) {
        self.engine.restart_progress("intro_toolbar", ID);
        self.engine.restart_progress("intro_menubar", ID);
        self.engine.restart_progress("intro_status", ID);
        self.engine.restart_progress("intro_canvas", ID);
        self.action_bar_t = 0.0;
        self.begin_action_bar_slide(1.0);
        self.engine.restart_progress("tab_fade", ID);
        self.engine.set_progress_max("tab_fade", ID);
        self.engine.set_progress_max("tab_slide", ID);
        self.engine.set_progress_max("status_sign", ID);
        self.engine.set_progress_max("status_tool_sign", ID);
        self.engine.set_progress_max("coords_sign", ID);
        self.engine.set_progress_max("coords_presence", ID);
    }

    pub fn replay_intro(&mut self) {
        self.play_intro();
    }

    pub fn seed_status_board(&mut self, message: &str, width: f32, tool_width: f32) {
        self.prev_status_message = message.to_owned();
        let tool = ToolKind::Select.label().to_owned();
        self.status_tool_outgoing = tool.clone();
        self.status_tool_incoming = tool;
        self.status_tool_width_out = tool_width;
        self.status_tool_width_in = tool_width;
        self.status_tool_width_settled = tool_width;
        self.status_msg_outgoing = message.to_owned();
        self.status_msg_incoming = message.to_owned();
        self.status_msg_width_out = width;
        self.status_msg_width_in = width;
        self.status_msg_width_settled = width;
        self.status_slide_distance = width + 40.0;
        self.prev_coords_text = "...".into();
        self.coords_outgoing = "...".into();
        self.coords_incoming = "...".into();
        self.coords_width_out = 120.0;
        self.coords_width_in = 120.0;
        self.coords_width_settled = 120.0;
        self.prev_has_coords = false;
        self.coords_from_w = 0.0;
        self.coords_to_w = 0.0;
        self.engine.set_progress_max("status_sign", ID);
        self.engine.set_progress_max("status_tool_sign", ID);
        self.engine.set_progress_max("coords_sign", ID);
        self.engine.set_progress_max("coords_presence", ID);
        self.engine.set_progress_max("on_path_offer", ID);
        self.engine.set_progress_max("on_path_container", ID);
        self.status_tool_target_in = true;
        self.status_msg_target_in = true;
        self.coords_target_in = true;
    }
}

/// Slide by moving the **left** edge: docked open position → one full width + gap to the right.
pub fn action_bar_overlay_rect(work: Rect, card_w: f32, open_t: f32) -> Rect {
    use crate::theme;
    let inset = theme::overlay_work_rect(work);
    let gap = theme::chrome_gap();
    let open_left = inset.right() - card_w;
    let t = open_t.clamp(0.0, 1.0);
    let left = open_left + (1.0 - t) * (card_w + gap);
    Rect::from_min_size(
        egui::pos2(left, inset.top()),
        egui::vec2(card_w, inset.height()),
    )
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgba_premultiplied(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
        (a.a() as f32 + (b.a() as f32 - a.a() as f32) * t) as u8,
    )
}

trait KramaFrameExt {
    fn set_progress_max(&mut self, class: &'static str, id: u32);
}

impl KramaFrameExt for KramaFrame<BTclasslist, BTframelist<TRES16Bits, i16>> {
    fn set_progress_max(&mut self, class: &'static str, id: u32) {
        if let Some(keylist) = self.framelist.get_mut(class) {
            if let Some(progress) = keylist.get_mut(id) {
                progress.max();
            }
        }
    }
}
