//! Mobile tool presentation mapping.
//!
//! No editing logic lives here: each [`MobileTool`] maps 1:1 onto an
//! existing [`crate::tools::ToolKind`]. Desktop-only tools surface as
//! [`MobileTool::More`] (tool sheet) instead of being duplicated.

use crate::tools::ToolKind;

/// Phone-priority tools. Order matches the bottom toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MobileTool {
    #[default]
    Select,
    Move,
    Shape,
    Pen,
    Brush,
    Text,
    Fill,
    Eraser,
    /// Overflow: full palette incl. desktop/power tools.
    More,
}

impl MobileTool {
    /// Phone bottom toolbar: Select / Move / Text / Brush / Shape, then the
    /// `More` overflow. Everything else lives in the Tools sheet.
    pub const BAR: [Self; 5] = [
        Self::Select,
        Self::Move,
        Self::Text,
        Self::Brush,
        Self::Shape,
    ];

    /// The shared editor tool this presents. `Move` has no dedicated editor
    /// tool: moving happens through Select-drag, so it maps to Select and
    /// the canvas gesture layer interprets drags as moves.
    pub fn tool_kind(self) -> ToolKind {
        match self {
            Self::Select | Self::Move => ToolKind::Select,
            Self::Shape => ToolKind::Rectangle,
            Self::Pen => ToolKind::Pen,
            Self::Brush => ToolKind::Brush,
            Self::Text => ToolKind::Text,
            Self::Fill => ToolKind::BucketFill,
            Self::Eraser => ToolKind::Eraser,
            Self::More => ToolKind::Select,
        }
    }

    /// Reverse map for toolbar highlight. Desktop/power tools (Node, Arc,
    /// Plotter, Smudge, …) have no dedicated phone button → `More`.
    pub fn from_kind(kind: ToolKind) -> Self {
        match kind {
            ToolKind::Select => Self::Select,
            ToolKind::Rectangle
            | ToolKind::Circle
            | ToolKind::Ellipse
            | ToolKind::Line
            | ToolKind::Polygon => Self::Shape,
            ToolKind::Pen => Self::Pen,
            ToolKind::Brush | ToolKind::RasterBrush => Self::Brush,
            ToolKind::Text => Self::Text,
            ToolKind::BucketFill => Self::Fill,
            ToolKind::Eraser => Self::Eraser,
            _ => Self::More,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Move => "Move",
            Self::Shape => "Shape",
            Self::Pen => "Pen",
            Self::Brush => "Brush",
            Self::Text => "Text",
            Self::Fill => "Fill",
            Self::Eraser => "Eraser",
            Self::More => "More",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_maps_to_shared_tools_without_duplicates() {
        for tool in MobileTool::BAR {
            // Every bar button drives an existing editor tool.
            let _ = tool.tool_kind();
        }
        // No mobile-only editing logic: More parks on Select.
        assert_eq!(MobileTool::More.tool_kind(), ToolKind::Select);
    }

    #[test]
    fn move_is_select_drag() {
        assert_eq!(MobileTool::Move.tool_kind(), ToolKind::Select);
    }

    #[test]
    fn roundtrip_for_bar_tools() {
        for tool in MobileTool::BAR {
            // Mapping back lands on the same button, except Move (which is a
            // gesture mode over Select, highlighted as Select).
            if tool == MobileTool::Move {
                assert_eq!(MobileTool::from_kind(tool.tool_kind()), MobileTool::Select);
            } else {
                assert_eq!(MobileTool::from_kind(tool.tool_kind()), tool);
            }
        }
    }

    #[test]
    fn desktop_tools_overflow_to_more() {
        assert_eq!(MobileTool::from_kind(ToolKind::Node), MobileTool::More);
        assert_eq!(MobileTool::from_kind(ToolKind::Plotter), MobileTool::More);
        assert_eq!(
            MobileTool::from_kind(ToolKind::Eyedropper),
            MobileTool::More
        );
    }
}
