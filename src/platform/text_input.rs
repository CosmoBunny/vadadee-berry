//! Cross-platform native text input (soft keyboard + IME).
//!
//! The existing Android integration called `winit` Android APIs directly
//! from UI code. That behavior is preserved but now lives behind
//! [`NativeTextInput`], so iOS can be added without touching widgets and
//! the Document/text renderer never learns about any OS.

/// What the editor tells the OS when text editing begins / while it runs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextInputState {
    pub text: String,
    /// Selection/cursor in chars. The legacy Android behavior parks the
    /// cursor at the end (`len..len`); preserved by the Android impl.
    pub cursor_start: usize,
    pub cursor_end: usize,
}

/// Narrow native text-input interface. One implementation per platform;
/// editor code only sees this trait.
pub trait NativeTextInput: Send {
    /// Editing began: publish initial state, show the soft keyboard.
    fn show(&mut self, state: &TextInputState);
    /// Read the current native text, if the platform reports one.
    fn native_text(&self) -> Option<String>;
    /// Push editor-side text to the OS (keeps IME in sync while typing in
    /// app widgets).
    fn push_text(&mut self, state: &TextInputState);
    /// Editing ended: hide the soft keyboard.
    fn hide(&mut self);
}

/// Platforms without OS text input (desktop): everything is a no-op and
/// [`NativeTextInput::native_text`] always returns `None`, so the existing
/// egui text path stays in charge.
#[derive(Debug, Default)]
pub struct DesktopTextInput;

impl NativeTextInput for DesktopTextInput {
    fn show(&mut self, _state: &TextInputState) {}
    fn native_text(&self) -> Option<String> {
        None
    }
    fn push_text(&mut self, _state: &TextInputState) {}
    fn hide(&mut self) {}
}

/// Android implementation: same `winit` soft-input calls the editor always
/// made, moved out of UI code. Cursor parks at end, no compose region —
/// byte-for-byte the legacy behavior.
#[cfg(target_os = "android")]
#[derive(Debug, Default)]
pub struct AndroidTextInput;

#[cfg(target_os = "android")]
impl NativeTextInput for AndroidTextInput {
    fn show(&mut self, state: &TextInputState) {
        if let Some(android_app) = crate::ANDROID_APP.get() {
            let len = state.text.chars().count();
            android_app.set_text_input_state(
                winit::platform::android::activity::input::TextInputState {
                    text: state.text.clone(),
                    selection: winit::platform::android::activity::input::TextSpan {
                        start: len,
                        end: len,
                    },
                    compose_region: None,
                },
            );
            android_app.show_soft_input(true);
        }
    }

    fn native_text(&self) -> Option<String> {
        crate::ANDROID_APP
            .get()
            .map(|android_app| android_app.text_input_state().text)
    }

    fn push_text(&mut self, state: &TextInputState) {
        self.show(state);
    }

    fn hide(&mut self) {
        if let Some(android_app) = crate::ANDROID_APP.get() {
            android_app.hide_soft_input(false);
        }
    }
}

/// iOS stub: no `UIKit` bridge exists in this codebase yet, so this is a
/// no-op placeholder with the same shape as Android. Implementing it means
/// filling in these four methods against `UITextInput` — no widget or
/// Document changes required.
#[derive(Debug, Default)]
pub struct IosTextInput;

impl NativeTextInput for IosTextInput {
    fn show(&mut self, _state: &TextInputState) {
        // TODO(ios): begin UITextInput session + show keyboard.
    }
    fn native_text(&self) -> Option<String> {
        // TODO(ios): return the marked-text-aware editor string.
        None
    }
    fn push_text(&mut self, _state: &TextInputState) {
        // TODO(ios): push editor text to the input session.
    }
    fn hide(&mut self) {
        // TODO(ios): resign first responder.
    }
}

/// Build the right implementation for this binary. iOS is selected by target
/// whitelist below once an iOS build exists; until then it compiles in as a
/// typed stub on every non-Android target (keeps the trait honest).
pub fn create_text_input() -> Box<dyn NativeTextInput> {
    #[cfg(target_os = "android")]
    {
        Box::new(AndroidTextInput)
    }
    #[cfg(target_os = "ios")]
    {
        Box::new(IosTextInput)
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        Box::new(DesktopTextInput)
    }
}

/// Reconcile loop shared by all platforms, factored out of the old Android
/// inline block: returns the text the document should hold, if it changed.
pub fn reconcile_text(
    input: &dyn NativeTextInput,
    ui_text: &str,
    last_sent: &mut String,
    push: &mut dyn FnMut(&TextInputState),
    patch: &mut dyn FnMut(&str),
) {
    if let Some(native) = input.native_text() {
        if native != *last_sent {
            *last_sent = native.clone();
            patch(&native);
        } else if ui_text != *last_sent {
            *last_sent = ui_text.to_string();
            let len = ui_text.chars().count();
            push(&TextInputState {
                text: ui_text.to_string(),
                cursor_start: len,
                cursor_end: len,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Desktop impl never claims native text: egui stays in charge.
    #[test]
    fn desktop_input_is_transparent() {
        let mut input = DesktopTextInput;
        assert_eq!(input.native_text(), None);
        let state = TextInputState {
            text: "hi".into(),
            cursor_start: 2,
            cursor_end: 2,
        };
        input.show(&state);
        input.push_text(&state);
        input.hide();
        assert_eq!(input.native_text(), None);
    }

    /// Native change wins and patches the document text.
    #[test]
    fn reconcile_prefers_native_change() {
        struct Scripted {
            native: String,
        }
        impl NativeTextInput for Scripted {
            fn show(&mut self, _: &TextInputState) {}
            fn native_text(&self) -> Option<String> {
                Some(self.native.clone())
            }
            fn push_text(&mut self, _: &TextInputState) {}
            fn hide(&mut self) {}
        }
        let input = Scripted {
            native: "native".into(),
        };
        let mut last = "old".to_string();
        let mut pushed = Vec::new();
        let mut patched = Vec::new();
        reconcile_text(
            &input,
            "ui",
            &mut last,
            &mut |s| pushed.push(s.text.clone()),
            &mut |t| patched.push(t.to_string()),
        );
        assert_eq!(last, "native");
        assert_eq!(patched, vec!["native"]);
        assert!(pushed.is_empty());
    }

    /// No native change + editor-side edit pushes to the OS.
    #[test]
    fn reconcile_pushes_editor_edits() {
        struct Steady;
        impl NativeTextInput for Steady {
            fn show(&mut self, _: &TextInputState) {}
            fn native_text(&self) -> Option<String> {
                Some("same".into())
            }
            fn push_text(&mut self, _: &TextInputState) {}
            fn hide(&mut self) {}
        }
        let input = Steady;
        let mut last = "same".to_string();
        let mut pushed = Vec::new();
        let mut patched = Vec::new();
        reconcile_text(
            &input,
            "edited",
            &mut last,
            &mut |s| pushed.push(s.text.clone()),
            &mut |t| patched.push(t.to_string()),
        );
        assert_eq!(last, "edited");
        assert_eq!(pushed, vec!["edited"]);
        assert!(patched.is_empty());
    }

    /// Platforms without native text do nothing (egui owns the text).
    #[test]
    fn reconcile_ignores_absent_native() {
        let input = DesktopTextInput;
        let mut last = "x".to_string();
        let mut pushed = 0;
        let mut patched = 0;
        reconcile_text(&input, "y", &mut last, &mut |_| pushed += 1, &mut |_| {
            patched += 1
        });
        assert_eq!(last, "x");
        assert_eq!((pushed, patched), (0, 0));
    }
}
