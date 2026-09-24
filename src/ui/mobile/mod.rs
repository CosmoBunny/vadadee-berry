//! Mobile presentation shell (phone + tablet).
//!
//! A different shell over the same editor core — never a compressed desktop
//! UI. All document mutation goes through the existing commands/tools on
//! `VadadeeBerryApp` (`do_undo`, `request_export_image`, `ToolKind`, …);
//! nothing here owns the `Document` or duplicates editing logic.
//!
//! Phase coverage: shell chrome (top bar, bottom toolbar, bottom sheets),
//! canvas hosting, tool mapping, gesture intents, navigation. Inspector,
//! layers, text editor, export sheet and timeline sheets arrive in later
//! phases as additional sheet content — the sheet infrastructure is here.

pub mod bottom_sheet;
pub mod bottom_toolbar;
pub mod gestures;
pub mod layers;
pub mod navigation;
pub mod shell;
pub mod state;
pub mod tool_palette;
pub mod tools;
pub mod top_bar;

pub use bottom_sheet::{MobileBottomSheet, SheetDetent};
pub use gestures::{EditorIntent, collect_frame_intents, intents_from_multitouch};
pub use layers::{LayerAction, LayerRow, layer_rows, show_layer_sheet};
pub use navigation::MobileNavigation;
pub use shell::MobileShell;
pub use state::{MobileSheet, MobileUiState};
pub use tool_palette::show_tool_palette;
pub use tools::MobileTool;
