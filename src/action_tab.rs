//! Action-bar tab identity (P0 cycle-break).
//!
//! `ActionTab` lived in `ui.rs`, forcing `app.rs` (which owns
//! `action_tab`/`action_tab_order` state) to depend on the UI module while
//! four UI-ish modules depend on the app — the `app ↔ ui` cycle. The enum
//! itself is pure (slugs, labels, icons), so it moves here; only the two
//! app-dependent strip helpers stay in `ui.rs` as free functions.

use crate::icons;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActionTab {
    Export,
    #[default]
    Layer,
    ColorStroke,
    Objects,
    Geometry,
    /// Raster paint layers, masks, float transform, brush engine.
    Paint,
    PathMagic,
    Animation,
    /// Graph parameters for Node Editor layers (animatable).
    Parameter,
}

impl ActionTab {
    /// Wire slug for collaboration UI sync.
    pub fn collab_slug(self) -> &'static str {
        match self {
            Self::Export => "export",
            Self::Layer => "layer",
            Self::ColorStroke => "color_stroke",
            Self::Objects => "objects",
            Self::Geometry => "geometry",
            Self::Paint => "paint",
            Self::PathMagic => "path_magic",
            Self::Animation => "animation",
            Self::Parameter => "parameter",
        }
    }

    pub fn from_collab_slug(s: &str) -> Option<Self> {
        match s {
            "export" => Some(Self::Export),
            "layer" => Some(Self::Layer),
            "color_stroke" => Some(Self::ColorStroke),
            "objects" => Some(Self::Objects),
            "geometry" => Some(Self::Geometry),
            "paint" => Some(Self::Paint),
            "path_magic" => Some(Self::PathMagic),
            "animation" => Some(Self::Animation),
            "parameter" => Some(Self::Parameter),
            _ => None,
        }
    }

    pub fn all_tabs() -> Vec<Self> {
        vec![
            Self::Export,
            Self::Layer,
            Self::ColorStroke,
            Self::Objects,
            Self::Geometry,
            Self::Paint,
            Self::PathMagic,
            Self::Animation,
            Self::Parameter,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Export => "Export",
            Self::Layer => "Layer",
            Self::ColorStroke => "Color & stroke",
            Self::Objects => "Objects",
            Self::Geometry => "Geometry",
            Self::Paint => "Paint",
            Self::PathMagic => "Path magic",
            Self::Animation => "Animation",
            Self::Parameter => "Parameter",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Export => "⤓",
            Self::Layer => icons::LAYER,
            Self::ColorStroke => icons::COLOR,
            Self::Objects => icons::OBJECT,
            Self::Geometry => icons::RECT,
            Self::Paint => icons::RASTER_BRUSH,
            Self::PathMagic => icons::PATH_MAGIC,
            Self::Animation => "",
            Self::Parameter => icons::PARAMETER,
        }
    }
}
