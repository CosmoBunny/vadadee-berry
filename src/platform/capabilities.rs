//! Presentation classification: device class, capabilities, metrics.
//!
//! Deliberately separate from editor state (`Document`, selection, tools):
//! these describe *how to present*, never *what is being edited*.

/// Presentation device class, derived from the available viewport — never
/// from `target_os` (a resizable desktop window can be phone-compact; an
/// Android tablet is not a phone).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiDeviceClass {
    /// Compact width: phone shell (bottom toolbar + sheets).
    Phone,
    /// Medium width: tablet shell (rail + canvas + properties).
    Tablet,
    /// Large width: full desktop shell.
    #[default]
    Desktop,
}

/// Width breakpoints in logical pixels, following common compact/medium/
/// expanded conventions. Classification uses the SMALLER viewport dimension:
/// a phone in landscape is still a phone (844×390 → Phone), and a tablet in
/// landscape is still a tablet (1280×800 → Tablet). Height alone never
/// promotes a class.
pub const PHONE_MAX_SIDE: f32 = 600.0;
pub const TABLET_MAX_SIDE: f32 = 1024.0;

/// Classify from the available viewport size (egui points, i.e. CSS-like px).
/// Zero/negative viewports (headless tests) fall back to [`UiDeviceClass::Desktop`].
pub fn classify_device(viewport: egui::Vec2) -> UiDeviceClass {
    if viewport.x <= 0.0 || viewport.y <= 0.0 {
        return UiDeviceClass::Desktop;
    }
    let m = viewport.x.min(viewport.y);
    if m < PHONE_MAX_SIDE {
        UiDeviceClass::Phone
    } else if m < TABLET_MAX_SIDE {
        UiDeviceClass::Tablet
    } else {
        UiDeviceClass::Desktop
    }
}

/// What the OS / device offers. One instance per process from
/// [`super::current_capabilities`]; UI branches on fields, not on `cfg`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformCapabilities {
    /// Finger/stylus touch input produces touch events.
    pub touch: bool,
    /// Active stylus with pressure/tilt.
    pub stylus: bool,
    /// Physical keyboard reliably present.
    pub keyboard: bool,
    /// Unrestricted filesystem paths usable.
    pub filesystem: bool,
    /// OS-level text input with IME/composition callbacks.
    pub native_text_input: bool,
    /// System file picker available.
    pub file_picker: bool,
    /// OS screen-capture APIs available.
    pub screen_capture: bool,
    /// Files can arrive via share/open-in (no stable path).
    pub external_files: bool,
    /// System share sheet available for outbound files.
    pub share_sheet: bool,
}

impl PlatformCapabilities {
    pub fn desktop() -> Self {
        Self {
            touch: false,
            stylus: false,
            keyboard: true,
            filesystem: true,
            native_text_input: false,
            file_picker: true,
            screen_capture: true,
            external_files: false,
            share_sheet: false,
        }
    }

    pub fn android() -> Self {
        Self {
            touch: true,
            stylus: true,
            keyboard: false,
            filesystem: false,
            native_text_input: true,
            file_picker: true,
            screen_capture: false,
            external_files: true,
            share_sheet: true,
        }
    }

    pub fn ios() -> Self {
        Self {
            touch: true,
            stylus: true,
            keyboard: false,
            filesystem: false,
            native_text_input: true,
            file_picker: true,
            screen_capture: false,
            external_files: true,
            share_sheet: true,
        }
    }
}

/// System-reserved screen regions (notch, home indicator, status bar,
/// keyboard). All mobile chrome lays out relative to these — never hard-coded
/// top/bottom margins.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SafeAreaInsets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl SafeAreaInsets {
    pub const ZERO: Self = Self {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };

    /// Read from egui's viewport (safe area reported by the OS, if any).
    pub fn from_context(ctx: &egui::Context) -> Self {
        // egui does not yet surface platform safe-area insets; viewports that
        // report them can be plumbed here later. Keyboard height is tracked
        // separately by the shell (IME rect) and added by callers.
        let _ = ctx;
        Self::ZERO
    }

    pub fn with_keyboard(mut self, keyboard_height: f32) -> Self {
        self.bottom = self.bottom.max(keyboard_height);
        self
    }
}

/// Semantic touch sizing. The *target* is finger-sized; icons stay small.
/// Desktop widgets must not be forced onto these metrics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiMetrics {
    /// Usable touch target (48dp default; 44–52 acceptable range).
    pub touch_target: f32,
    /// Icon glyph size inside the target (20–24dp).
    pub toolbar_icon: f32,
    /// Toolbar button extent (target + visual padding).
    pub toolbar_button_size: f32,
    /// Panel content padding.
    pub panel_padding: f32,
    /// Vertical rhythm between sections.
    pub section_spacing: f32,
    /// Bottom-sheet drag handle height.
    pub sheet_handle: f32,
}

impl UiMetrics {
    pub fn phone() -> Self {
        Self {
            touch_target: 48.0,
            toolbar_icon: 22.0,
            toolbar_button_size: 52.0,
            panel_padding: 16.0,
            section_spacing: 12.0,
            sheet_handle: 24.0,
        }
    }

    pub fn tablet() -> Self {
        Self {
            touch_target: 44.0,
            toolbar_icon: 22.0,
            toolbar_button_size: 48.0,
            panel_padding: 16.0,
            section_spacing: 12.0,
            sheet_handle: 24.0,
        }
    }

    pub fn desktop() -> Self {
        Self {
            touch_target: 28.0,
            toolbar_icon: 16.0,
            toolbar_button_size: 32.0,
            panel_padding: 8.0,
            section_spacing: 8.0,
            sheet_handle: 0.0,
        }
    }

    pub fn for_device(device: UiDeviceClass) -> Self {
        match device {
            UiDeviceClass::Phone => Self::phone(),
            UiDeviceClass::Tablet => Self::tablet(),
            UiDeviceClass::Desktop => Self::desktop(),
        }
    }
}

/// Everything a mobile component needs to lay out: resolved once per frame
/// at the shell root and passed down. No `if width < 500` in widgets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiLayout {
    pub device: UiDeviceClass,
    pub safe_area: SafeAreaInsets,
    pub metrics: UiMetrics,
    /// Available canvas area in points (viewport minus chrome).
    pub viewport: egui::Vec2,
    /// True when height > width.
    pub portrait: bool,
}

impl UiLayout {
    pub fn resolve(device: UiDeviceClass, safe_area: SafeAreaInsets, viewport: egui::Vec2) -> Self {
        Self {
            device,
            safe_area,
            metrics: UiMetrics::for_device(device),
            viewport,
            portrait: viewport.y >= viewport.x,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec viewport matrix (§33): classification must match class per size.
    #[test]
    fn spec_viewports_classify_correctly() {
        let cases = [
            ((320.0, 568.0), UiDeviceClass::Phone),
            ((360.0, 800.0), UiDeviceClass::Phone),
            ((390.0, 844.0), UiDeviceClass::Phone),
            ((430.0, 932.0), UiDeviceClass::Phone),
            ((844.0, 390.0), UiDeviceClass::Phone),
            ((800.0, 1280.0), UiDeviceClass::Tablet),
            ((1280.0, 800.0), UiDeviceClass::Tablet),
            ((1920.0, 1080.0), UiDeviceClass::Desktop),
        ];
        for ((w, h), want) in cases {
            assert_eq!(classify_device(egui::vec2(w, h)), want, "viewport {w}x{h}");
        }
    }

    #[test]
    fn landscape_phone_stays_phone_and_flags_orientation() {
        let layout = UiLayout::resolve(
            classify_device(egui::vec2(844.0, 390.0)),
            SafeAreaInsets::ZERO,
            egui::vec2(844.0, 390.0),
        );
        assert_eq!(layout.device, UiDeviceClass::Phone);
        assert!(!layout.portrait);
    }

    #[test]
    fn degenerate_viewport_falls_back_to_desktop() {
        assert_eq!(
            classify_device(egui::vec2(0.0, 0.0)),
            UiDeviceClass::Desktop
        );
        assert_eq!(
            classify_device(egui::vec2(-10.0, 500.0)),
            UiDeviceClass::Desktop
        );
    }

    #[test]
    fn breakpoints_are_exclusive_lower_bounds() {
        assert_eq!(
            classify_device(egui::vec2(599.0, 900.0)),
            UiDeviceClass::Phone
        );
        assert_eq!(
            classify_device(egui::vec2(600.0, 900.0)),
            UiDeviceClass::Tablet
        );
        assert_eq!(
            classify_device(egui::vec2(1023.0, 1400.0)),
            UiDeviceClass::Tablet
        );
        assert_eq!(
            classify_device(egui::vec2(1024.0, 1400.0)),
            UiDeviceClass::Desktop
        );
        // Short side decides, either orientation.
        assert_eq!(
            classify_device(egui::vec2(1400.0, 1023.0)),
            UiDeviceClass::Tablet
        );
        assert_eq!(
            classify_device(egui::vec2(900.0, 599.0)),
            UiDeviceClass::Phone
        );
    }

    #[test]
    fn phone_metrics_meet_touch_target_rules() {
        let m = UiMetrics::phone();
        assert!((44.0..=52.0).contains(&m.touch_target));
        assert!((20.0..=24.0).contains(&m.toolbar_icon));
        assert!(m.toolbar_button_size >= m.touch_target);
    }

    #[test]
    fn keyboard_extends_bottom_inset() {
        let insets = SafeAreaInsets {
            bottom: 20.0,
            ..SafeAreaInsets::ZERO
        }
        .with_keyboard(300.0);
        assert_eq!(insets.bottom, 300.0);
        let kept = SafeAreaInsets {
            bottom: 400.0,
            ..SafeAreaInsets::ZERO
        }
        .with_keyboard(300.0);
        assert_eq!(kept.bottom, 400.0);
    }

    #[test]
    fn platform_capability_shapes() {
        let d = PlatformCapabilities::desktop();
        assert!(d.keyboard && d.filesystem && !d.touch && !d.share_sheet);
        let a = PlatformCapabilities::android();
        assert!(a.touch && a.share_sheet && a.external_files && !a.filesystem);
        let i = PlatformCapabilities::ios();
        assert!(i.touch && i.share_sheet && i.native_text_input && !i.keyboard);
    }
}
