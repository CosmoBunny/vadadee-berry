//! Mobile primary destinations. A deliberately small set: the phone UI
//! prioritizes canvas, selection, transform, layers, text, shapes, brush,
//! pen, fill/stroke, undo/redo and export. Everything else is progressively
//! disclosed through sheets, never competing for permanent screen space.

/// Primary destinations. Non-canvas destinations present sheets over the
/// canvas (the canvas keeps its state underneath).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MobileNavigation {
    /// Canvas-first default.
    #[default]
    Canvas,
    /// Layer sheet destination.
    Layers,
    /// Properties/inspector destination.
    Properties,
    /// Timeline destination.
    Timeline,
}

impl MobileNavigation {
    /// The sheet each destination presents, if any.
    pub fn sheet(self) -> Option<super::state::MobileSheet> {
        match self {
            Self::Canvas => None,
            Self::Layers => Some(super::state::MobileSheet::Layers),
            Self::Properties => Some(super::state::MobileSheet::Inspector),
            Self::Timeline => Some(super::state::MobileSheet::Timeline),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_is_default_and_sheetless() {
        assert_eq!(MobileNavigation::default(), MobileNavigation::Canvas);
        assert_eq!(MobileNavigation::Canvas.sheet(), None);
    }

    #[test]
    fn every_destination_maps() {
        assert!(MobileNavigation::Layers.sheet().is_some());
        assert!(MobileNavigation::Properties.sheet().is_some());
        assert!(MobileNavigation::Timeline.sheet().is_some());
    }
}
