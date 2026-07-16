//! Node Editor graph: typed nodes, ports, links, and layer parameters.
//! One graph per `LayerKind::NodeEditor` layer. Output Object is the continuous render sink.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Port data types for connection validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PortType {
    /// Still / processed image stream.
    RawImage,
    /// Video media (file path / timed frames). Distinct from Image in the UI.
    RawVideo,
    /// Audio or demuxed soundtrack.
    RawSound,
    /// Canonical septic session (`.sepscrr` / Screen Record layer).
    Septic,
    /// Mouse track / sample (needs Mouse Encoder for pos / shake / event).
    Mouse,
    /// Scalar real number.
    Real,
    /// Compound color (3 reals in UI / expand).
    Color,
    /// Compound position (2 reals).
    Position,
}

impl PortType {
    pub fn label(self) -> &'static str {
        match self {
            Self::RawImage => "Image",
            Self::RawVideo => "Video",
            Self::RawSound => "Sound",
            Self::Septic => "Septic",
            Self::Mouse => "Mouse",
            Self::Real => "Real",
            Self::Color => "Color",
            Self::Position => "Position",
        }
    }

    /// Whether `from` may connect into a port of type `to`.
    pub fn can_connect(from: Self, to: Self) -> bool {
        match (from, to) {
            (a, b) if a == b => true,
            // Video can feed image-effect chains (treat as image stream).
            (Self::RawVideo, Self::RawImage) => true,
            // Color/Position can feed Real only via expanded child ports later; not as whole.
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortDir {
    Input,
    Output,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortDef {
    pub id: String,
    pub name: String,
    pub ty: PortType,
    pub dir: PortDir,
}

/// Kind of graph node (catalog entry).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GraphNodeKind {
    // --- Object ---
    ObjectImage {
        #[serde(default)]
        path: String,
    },
    ObjectVideo {
        #[serde(default)]
        path: String,
    },
    ObjectAudio {
        #[serde(default)]
        path: String,
    },
    /// Reference application object(s); valid even if their layer is hidden.
    ObjectFromApp {
        #[serde(default)]
        node_ids: Vec<Uuid>,
    },
    /// Continuous video-like sink of the graph.
    OutputObject,
    /// Decode a video object at a driven time → image + sound. Blank when past end.
    VideoPlayer,
    /// Septic session object (browse `.sepscrr` or path from Screen Record layer).
    ObjectSeptic {
        #[serde(default)]
        path: String,
    },
    /// Mouse object — septic path / track reference. Use Mouse Encoder to extract signals.
    ObjectMouse {
        #[serde(default)]
        path: String,
    },
    /// Septic + Time → video, mouse, truth time.
    SepticPlayer,
    /// Mouse → pos, shakiness (time threshold + gain), event (0/1).
    MouseEncoder {
        #[serde(default = "default_mouse_time_threshold")]
        time_threshold: f64,
        #[serde(default = "default_mouse_gain")]
        gain: f64,
    },

    // --- Algebra ---
    Value {
        #[serde(default)]
        value: f64,
    },
    /// Scalar expression in `x` (and t/f helpers). Serde name kept as `Expr` for old projects.
    #[serde(alias = "Expr")]
    ExprX {
        #[serde(default = "default_expr_x")]
        expr: String,
    },
    /// Expression in `x`, `y` (and t/f).
    ExprXy {
        #[serde(default = "default_expr_xy")]
        expr: String,
    },
    /// Expression in `x`, `y`, `z` (and t/f).
    ExprXyz {
        #[serde(default = "default_expr_xyz")]
        expr: String,
    },
    Frame,
    Time,

    // --- Effect (subset) ---
    Brightness,
    ColorChanger,
    LinearBlur,
    /// Crop/zoom: image + position (center 0..1) + zoom factor (≥1; 1 = full frame).
    Zoom,
    Equalizer,
    Speed,
    /// Audio visualizer: `audio` + optional `frequency`/`gain` → real level 0..1.
    Visualizer {
        #[serde(default = "default_viz_gain")]
        gain: f64,
    },

    // --- Geometry ---
    GeoSize,
    GeoPlacement,
    GeoRotate,
    GeoTrapezoid,
    GeoMirror,
    GeoAdd,

    // --- Parameter references (driven from Parameter tab / anim) ---
    ParamReal {
        param_id: Uuid,
    },
    ParamColor {
        param_id: Uuid,
    },
    ParamPosition {
        param_id: Uuid,
    },
}

fn default_expr_x() -> String {
    "x".into()
}
fn default_expr_xy() -> String {
    "x+y".into()
}
fn default_expr_xyz() -> String {
    "x+y+z".into()
}

fn default_viz_gain() -> f64 {
    1.0
}
fn default_mouse_time_threshold() -> f64 {
    // Wider window = smoother shakiness envelope while still tracking tremor.
    0.20
}
fn default_mouse_gain() -> f64 {
    // Soft-exp residual metric; 6 sits in a comfortable mid-range.
    6.0
}

impl GraphNodeKind {
    pub fn category_label(&self) -> &'static str {
        match self {
            Self::ObjectImage { .. }
            | Self::ObjectVideo { .. }
            | Self::ObjectAudio { .. }
            | Self::ObjectFromApp { .. }
            | Self::ObjectSeptic { .. }
            | Self::ObjectMouse { .. }
            | Self::OutputObject => "Object",
            Self::VideoPlayer | Self::SepticPlayer | Self::MouseEncoder { .. } => "Player",
            Self::Value { .. }
            | Self::ExprX { .. }
            | Self::ExprXy { .. }
            | Self::ExprXyz { .. }
            | Self::Frame
            | Self::Time => "Algebra",
            Self::Brightness
            | Self::ColorChanger
            | Self::LinearBlur
            | Self::Zoom
            | Self::Equalizer
            | Self::Speed
            | Self::Visualizer { .. } => "Effect",
            Self::GeoSize
            | Self::GeoPlacement
            | Self::GeoRotate
            | Self::GeoTrapezoid
            | Self::GeoMirror
            | Self::GeoAdd => "Geometry",
            Self::ParamReal { .. } | Self::ParamColor { .. } | Self::ParamPosition { .. } => {
                "Parameter"
            }
        }
    }

    pub fn default_title(&self) -> &'static str {
        match self {
            Self::ObjectImage { .. } => "Image",
            Self::ObjectVideo { .. } => "Video",
            Self::ObjectAudio { .. } => "Audio",
            Self::ObjectFromApp { .. } => "App Object",
            Self::OutputObject => "Output Object",
            Self::VideoPlayer => "Video Player",
            Self::ObjectSeptic { .. } => "Septic",
            Self::ObjectMouse { .. } => "Mouse",
            Self::SepticPlayer => "Septic Player",
            Self::MouseEncoder { .. } => "Mouse Player",
            Self::Value { .. } => "Value",
            Self::ExprX { .. } => "Expr X",
            Self::ExprXy { .. } => "Expr XY",
            Self::ExprXyz { .. } => "Expr XYZ",
            Self::Frame => "Frame",
            Self::Time => "Time",
            Self::Brightness => "Brightness",
            Self::ColorChanger => "Color Changer",
            Self::LinearBlur => "Linear Blur",
            Self::Zoom => "Zoom",
            Self::Equalizer => "Equalizer",
            Self::Speed => "Speed",
            Self::Visualizer { .. } => "Visualizer",
            Self::GeoSize => "Size",
            Self::GeoPlacement => "Placement",
            Self::GeoRotate => "Rotate",
            Self::GeoTrapezoid => "Trapezoid",
            Self::GeoMirror => "Mirror",
            Self::GeoAdd => "Add",
            Self::ParamReal { .. } => "Param Real",
            Self::ParamColor { .. } => "Param Color",
            Self::ParamPosition { .. } => "Param Position",
        }
    }

    pub fn ports(&self) -> Vec<PortDef> {
        use PortDir::*;
        use PortType::*;
        match self {
            Self::ObjectImage { .. } | Self::ObjectFromApp { .. } => {
                vec![PortDef {
                    id: "out".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Output,
                }]
            }
            // Video is video-only (no mixed A/V on one pin). Soundtrack is a separate Sound pin.
            Self::ObjectVideo { .. } => vec![
                PortDef {
                    id: "out".into(),
                    name: "Video".into(),
                    ty: RawVideo,
                    dir: Output,
                },
                PortDef {
                    id: "sound".into(),
                    name: "Sound".into(),
                    ty: RawSound,
                    dir: Output,
                },
            ],
            Self::ObjectAudio { .. } => vec![PortDef {
                id: "out".into(),
                name: "Sound".into(),
                ty: RawSound,
                dir: Output,
            }],
            // Video + Time (+ Start/Duration) → Image; optional Audio in → timed Sound out.
            // Real outs: total media duration (s) + pixel size (W/H).
            Self::VideoPlayer => vec![
                PortDef {
                    id: "video".into(),
                    name: "Video".into(),
                    ty: RawVideo,
                    dir: Input,
                },
                PortDef {
                    id: "time".into(),
                    name: "Time".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "start".into(),
                    name: "Start".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "duration".into(),
                    name: "Clip".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "audio".into(),
                    name: "Audio".into(),
                    ty: RawSound,
                    dir: Input,
                },
                PortDef {
                    id: "out".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Output,
                },
                PortDef {
                    id: "sound".into(),
                    name: "Sound".into(),
                    ty: RawSound,
                    dir: Output,
                },
                PortDef {
                    id: "total_duration".into(),
                    name: "Duration".into(),
                    ty: Real,
                    dir: Output,
                },
                PortDef {
                    id: "width".into(),
                    name: "Width".into(),
                    ty: Real,
                    dir: Output,
                },
                PortDef {
                    id: "height".into(),
                    name: "Height".into(),
                    ty: Real,
                    dir: Output,
                },
            ],
            Self::ObjectSeptic { .. } => vec![PortDef {
                id: "out".into(),
                name: "Septic".into(),
                ty: Septic,
                dir: Output,
            }],
            Self::ObjectMouse { .. } => vec![PortDef {
                id: "out".into(),
                name: "Mouse".into(),
                ty: Mouse,
                dir: Output,
            }],
            // Septic + Time → video, mouse, sound (from session mp4), truth time
            Self::SepticPlayer => vec![
                PortDef {
                    id: "septic".into(),
                    name: "Septic".into(),
                    ty: Septic,
                    dir: Input,
                },
                PortDef {
                    id: "time".into(),
                    name: "Time".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "video".into(),
                    name: "Video".into(),
                    ty: RawVideo,
                    dir: Output,
                },
                PortDef {
                    id: "mouse".into(),
                    name: "Mouse".into(),
                    ty: Mouse,
                    dir: Output,
                },
                PortDef {
                    id: "sound".into(),
                    name: "Sound".into(),
                    ty: RawSound,
                    dir: Output,
                },
                PortDef {
                    id: "time_out".into(),
                    name: "Time".into(),
                    ty: Real,
                    dir: Output,
                },
            ],
            // Mouse → pos, shakiness, event (requires Encoder — not raw object alone)
            Self::MouseEncoder { .. } => vec![
                PortDef {
                    id: "mouse".into(),
                    name: "Mouse".into(),
                    ty: Mouse,
                    dir: Input,
                },
                PortDef {
                    id: "time".into(),
                    name: "Time".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "threshold".into(),
                    name: "Threshold".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "gain".into(),
                    name: "Gain".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "pos".into(),
                    name: "Pos".into(),
                    ty: Position,
                    dir: Output,
                },
                PortDef {
                    id: "x".into(),
                    name: "X".into(),
                    ty: Real,
                    dir: Output,
                },
                PortDef {
                    id: "y".into(),
                    name: "Y".into(),
                    ty: Real,
                    dir: Output,
                },
                PortDef {
                    id: "shakiness".into(),
                    name: "Shake".into(),
                    ty: Real,
                    dir: Output,
                },
                PortDef {
                    id: "event".into(),
                    name: "Event".into(),
                    ty: Real,
                    dir: Output,
                },
            ],
            Self::OutputObject => vec![
                PortDef {
                    id: "image".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Input,
                },
                PortDef {
                    id: "sound".into(),
                    name: "Sound".into(),
                    ty: RawSound,
                    dir: Input,
                },
                PortDef {
                    id: "run_till".into(),
                    name: "Run Till".into(),
                    ty: Real,
                    dir: Input,
                },
            ],
            Self::Value { .. } | Self::Frame | Self::Time | Self::ParamReal { .. } => {
                vec![PortDef {
                    id: "out".into(),
                    name: "Value".into(),
                    ty: Real,
                    dir: Output,
                }]
            }
            Self::ExprX { .. } => vec![
                PortDef {
                    id: "x".into(),
                    name: "x".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "out".into(),
                    name: "Result".into(),
                    ty: Real,
                    dir: Output,
                },
            ],
            Self::ExprXy { .. } => vec![
                PortDef {
                    id: "x".into(),
                    name: "x".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "y".into(),
                    name: "y".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "out".into(),
                    name: "Result".into(),
                    ty: Real,
                    dir: Output,
                },
            ],
            Self::ExprXyz { .. } => vec![
                PortDef {
                    id: "x".into(),
                    name: "x".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "y".into(),
                    name: "y".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "z".into(),
                    name: "z".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "out".into(),
                    name: "Result".into(),
                    ty: Real,
                    dir: Output,
                },
            ],
            Self::Brightness => vec![
                PortDef {
                    id: "in".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Input,
                },
                PortDef {
                    id: "amount".into(),
                    name: "Amount".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "out".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Output,
                },
            ],
            Self::Visualizer { .. } => vec![
                PortDef {
                    id: "audio".into(),
                    name: "Audio".into(),
                    ty: RawSound,
                    dir: Input,
                },
                PortDef {
                    id: "frequency".into(),
                    name: "Freq".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "gain".into(),
                    name: "Gain".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "out".into(),
                    name: "Level".into(),
                    ty: Real,
                    dir: Output,
                },
            ],
            Self::ColorChanger => vec![
                PortDef {
                    id: "in".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Input,
                },
                PortDef {
                    id: "hue".into(),
                    name: "Hue".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "sat".into(),
                    name: "Sat".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "out".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Output,
                },
            ],
            Self::LinearBlur => vec![
                PortDef {
                    id: "in".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Input,
                },
                PortDef {
                    id: "amount".into(),
                    name: "Radius".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "out".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Output,
                },
            ],
            Self::Zoom => vec![
                PortDef {
                    id: "in".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Input,
                },
                PortDef {
                    id: "pos".into(),
                    name: "Pos".into(),
                    ty: Position,
                    dir: Input,
                },
                PortDef {
                    id: "zoom".into(),
                    name: "Zoom".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "out".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Output,
                },
            ],
            Self::Equalizer => vec![
                PortDef {
                    id: "in".into(),
                    name: "Sound".into(),
                    ty: RawSound,
                    dir: Input,
                },
                PortDef {
                    id: "bass".into(),
                    name: "Bass".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "mid".into(),
                    name: "Mid".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "treble".into(),
                    name: "Treble".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "volume".into(),
                    name: "Volume".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "out".into(),
                    name: "Sound".into(),
                    ty: RawSound,
                    dir: Output,
                },
            ],
            Self::Speed => vec![
                PortDef {
                    id: "in".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Input,
                },
                PortDef {
                    id: "factor".into(),
                    name: "Factor".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "out".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Output,
                },
            ],
            // Geometry transforms operate on an image stream and take Real/Position controls.
            Self::GeoSize => vec![
                PortDef {
                    id: "in".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Input,
                },
                PortDef {
                    id: "w".into(),
                    name: "Width".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "h".into(),
                    name: "Height".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "out".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Output,
                },
            ],
            Self::GeoPlacement => vec![
                PortDef {
                    id: "in".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Input,
                },
                PortDef {
                    id: "x".into(),
                    name: "X".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "y".into(),
                    name: "Y".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "out".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Output,
                },
            ],
            Self::GeoRotate => vec![
                PortDef {
                    id: "in".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Input,
                },
                PortDef {
                    id: "angle".into(),
                    name: "Angle".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "out".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Output,
                },
            ],
            Self::GeoTrapezoid => vec![
                PortDef {
                    id: "in".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Input,
                },
                PortDef {
                    id: "top".into(),
                    name: "Top".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "bottom".into(),
                    name: "Bottom".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "out".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Output,
                },
            ],
            Self::GeoMirror => vec![
                PortDef {
                    id: "in".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Input,
                },
                PortDef {
                    id: "axis".into(),
                    name: "Axis".into(),
                    ty: Real,
                    dir: Input,
                },
                PortDef {
                    id: "out".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Output,
                },
            ],
            Self::GeoAdd => vec![
                PortDef {
                    id: "a".into(),
                    name: "Image A".into(),
                    ty: RawImage,
                    dir: Input,
                },
                PortDef {
                    id: "b".into(),
                    name: "Image B".into(),
                    ty: RawImage,
                    dir: Input,
                },
                PortDef {
                    id: "out".into(),
                    name: "Image".into(),
                    ty: RawImage,
                    dir: Output,
                },
            ],
            Self::ParamColor { .. } => vec![PortDef {
                id: "out".into(),
                name: "Color".into(),
                ty: Color,
                dir: Output,
            }],
            Self::ParamPosition { .. } => vec![PortDef {
                id: "out".into(),
                name: "Pos".into(),
                ty: Position,
                dir: Output,
            }],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: Uuid,
    pub name: String,
    pub kind: GraphNodeKind,
    /// Position in graph space (not document page space).
    pub x: f32,
    pub y: f32,
    #[serde(default)]
    pub error: Option<String>,
}

impl GraphNode {
    pub fn new(kind: GraphNodeKind, x: f32, y: f32) -> Self {
        let name = kind.default_title().to_string();
        Self {
            id: Uuid::new_v4(),
            name,
            kind,
            x,
            y,
            error: None,
        }
    }

    pub fn ports(&self) -> Vec<PortDef> {
        self.kind.ports()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphLink {
    pub id: Uuid,
    pub from_node: Uuid,
    pub from_port: String,
    pub to_node: Uuid,
    pub to_port: String,
}

impl GraphLink {
    pub fn new(from_node: Uuid, from_port: impl Into<String>, to_node: Uuid, to_port: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            from_node,
            from_port: from_port.into(),
            to_node,
            to_port: to_port.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GraphParamKind {
    #[default]
    Real,
    Color,
    Position,
}

/// Animatable parameter exposed on the ActionBar Parameter tab.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphParam {
    pub id: Uuid,
    pub name: String,
    pub kind: GraphParamKind,
    /// Real value, or R of color / X of position.
    #[serde(default)]
    pub v0: f64,
    /// G of color / Y of position.
    #[serde(default)]
    pub v1: f64,
    /// B of color.
    #[serde(default)]
    pub v2: f64,
    /// A of color (optional).
    #[serde(default = "default_one_f64")]
    pub v3: f64,
}

fn default_one_f64() -> f64 {
    1.0
}

impl GraphParam {
    pub fn new_real(name: impl Into<String>, value: f64) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            kind: GraphParamKind::Real,
            v0: value,
            v1: 0.0,
            v2: 0.0,
            v3: 1.0,
        }
    }

    pub fn new_color(name: impl Into<String>, r: f64, g: f64, b: f64) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            kind: GraphParamKind::Color,
            v0: r,
            v1: g,
            v2: b,
            v3: 1.0,
        }
    }

    pub fn new_position(name: impl Into<String>, x: f64, y: f64) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            kind: GraphParamKind::Position,
            v0: x,
            v1: y,
            v2: 0.0,
            v3: 1.0,
        }
    }

    /// Animation track labels stored on the Node Editor layer's [`crate::document::NodeAnimation`].
    /// Real → `param:{id}`; Color → `param:{id}:0..3` (rgba); Position → `param:{id}:0..1` (xy).
    pub fn anim_track_labels(&self) -> Vec<String> {
        let base = format!("param:{}", self.id);
        match self.kind {
            GraphParamKind::Real => vec![base],
            GraphParamKind::Color => (0..4).map(|i| format!("{base}:{i}")).collect(),
            GraphParamKind::Position => (0..2).map(|i| format!("{base}:{i}")).collect(),
        }
    }

    /// Component index for a track label (`None` for Real whole-value track).
    pub fn component_from_track_label(label: &str, param_id: Uuid) -> Option<Option<usize>> {
        let base = format!("param:{param_id}");
        if label == base {
            return Some(None);
        }
        let prefix = format!("{base}:");
        label.strip_prefix(&prefix).and_then(|s| s.parse().ok()).map(Some)
    }
}

/// Resolved image feeding Output Object (after walking pass-through effects).
#[derive(Debug, Clone, PartialEq)]
pub enum GraphImageSource {
    /// Document object ids (ObjectFromApp).
    AppObjects(Vec<Uuid>),
    /// Filesystem path (ObjectImage / ObjectVideo).
    FilePath(String),
    /// Nothing connected or unresolved.
    Empty,
}

/// Sound source resolved from Output Object `sound` input (P5).
#[derive(Debug, Clone, PartialEq)]
pub enum GraphSoundSource {
    FilePath(String),
    Empty,
}

/// Canvas/export evaluation of the Output Object sound chain (P5).
#[derive(Debug, Clone, PartialEq)]
pub struct GraphOutputSound {
    pub sound: GraphSoundSource,
    /// Linear gain (Equalizer.volume or default 1.0).
    pub volume: f64,
    /// Shelf gains in dB-ish units (−12..+12), stacked from Equalizer nodes.
    pub eq_bass: f64,
    pub eq_mid: f64,
    pub eq_treble: f64,
    /// Seek into the media file (VideoPlayer driven time). When set, playback
    /// uses this as the media position instead of the timeline playhead alone.
    pub media_time_sec: Option<f64>,
    /// Media seconds consumed per timeline second (Expr `x*2` → ~2.0). Audio is
    /// played at this rate so it stays locked to VideoPlayer frames.
    pub playback_rate: f64,
}

impl Default for GraphOutputSound {
    fn default() -> Self {
        Self {
            sound: GraphSoundSource::Empty,
            volume: 1.0,
            eq_bass: 0.0,
            eq_mid: 0.0,
            eq_treble: 0.0,
            media_time_sec: None,
            playback_rate: 1.0,
        }
    }
}

impl GraphOutputSound {
    pub fn path(&self) -> Option<&str> {
        match &self.sound {
            GraphSoundSource::FilePath(p) if !p.trim().is_empty() => Some(p.as_str()),
            _ => None,
        }
    }
}

/// Canvas-facing evaluation of the Output Object image chain (P2/P4).
#[derive(Debug, Clone, PartialEq)]
pub struct GraphOutputEval {
    pub image: GraphImageSource,
    /// Multiplicative brightness (Brightness.amount, default 1.0).
    pub brightness: f64,
    /// Contrast multiplier (default 1.0). Reserved / stack-friendly.
    pub contrast: f64,
    /// Saturation multiplier from ColorChanger.sat (default 1.0).
    pub saturation: f64,
    /// Hue shift in degrees from ColorChanger.hue (default 0).
    pub hue_shift: f64,
    /// Linear blur radius in pixels (LinearBlur.amount, default 0).
    pub blur_px: f64,
    /// Playback speed factor (Speed.factor, default 1.0) — video/time consumers.
    pub speed: f64,
    /// When set, `FilePath` is decoded as a video frame at this media time (seconds).
    /// Past duration → callers blank the image (`Empty`).
    pub video_time_sec: Option<f64>,
    /// Geometry: scale width/height multipliers (GeoSize), default 1.
    pub geo_scale_w: f64,
    pub geo_scale_h: f64,
    /// Geometry: placement offset (GeoPlacement).
    pub geo_off_x: f64,
    pub geo_off_y: f64,
    /// Geometry: rotation degrees (GeoRotate).
    pub geo_rot_deg: f64,
    /// Geometry: mirror axis (0=none, 1=horizontal, 2=vertical, 3=both).
    pub geo_mirror: f64,
    /// Zoom factor (≥1). `1.0` = identity (full frame).
    pub zoom: f64,
    /// Zoom center in normalized image coords (0..1), screen-style Y down.
    pub zoom_cx: f64,
    pub zoom_cy: f64,
    /// Whether any effect nodes were traversed.
    pub effects_on_path: bool,
}

impl Default for GraphOutputEval {
    fn default() -> Self {
        Self {
            image: GraphImageSource::Empty,
            brightness: 1.0,
            contrast: 1.0,
            saturation: 1.0,
            hue_shift: 0.0,
            blur_px: 0.0,
            speed: 1.0,
            video_time_sec: None,
            geo_scale_w: 1.0,
            geo_scale_h: 1.0,
            geo_off_x: 0.0,
            geo_off_y: 0.0,
            geo_rot_deg: 0.0,
            geo_mirror: 0.0,
            zoom: 1.0,
            zoom_cx: 0.5,
            zoom_cy: 0.5,
            effects_on_path: false,
        }
    }
}

impl GraphOutputEval {
    /// True when any **CPU** effect differs from identity (color / blur).
    /// Zoom is paint-time UV only — not pixel FX.
    pub fn needs_pixel_fx(&self) -> bool {
        (self.brightness - 1.0).abs() > 1e-6
            || (self.contrast - 1.0).abs() > 1e-6
            || (self.saturation - 1.0).abs() > 1e-6
            || self.hue_shift.abs() > 1e-6
            || self.blur_px > 0.01
    }

    /// Brightness alone can be applied as a vertex tint (free every frame).
    /// Contrast / sat / hue / blur still need a texture bake. Zoom is free UV.
    pub fn only_brightness_fx(&self) -> bool {
        self.blur_px <= 0.01
            && (self.contrast - 1.0).abs() < 1e-3
            && (self.saturation - 1.0).abs() < 1e-3
            && self.hue_shift.abs() < 1e-3
    }

    /// Needs a new baked texture (not free paint-time multiply / UV zoom).
    pub fn needs_texture_bake(&self) -> bool {
        self.blur_px > 0.01
            || (self.contrast - 1.0).abs() > 1e-3
            || (self.saturation - 1.0).abs() > 1e-3
            || self.hue_shift.abs() > 1e-3
    }

    /// Cache key for processed file textures (zoom is UV at paint — not in key).
    /// Blur uses **2 decimal places** so each animation frame can get its own look (10→0 over 60f).
    pub fn fx_cache_key(&self, path: &str) -> String {
        let t = self
            .video_time_sec
            .map(|t| format!("|t{t:.3}"))
            .unwrap_or_default();
        format!(
            "{path}{t}|b{:.3}|c{:.3}|s{:.3}|h{:.2}|bl{:.2}",
            self.brightness,
            self.contrast,
            self.saturation,
            self.hue_shift,
            self.blur_px,
        )
    }

    /// UV rect for paint-time zoom (0..1). Identity when zoom ≤ 1.
    /// Center `(zoom_cx, zoom_cy)` screen-style (Y down); factor ≥ 1.
    pub fn zoom_uv_rect(&self) -> (f32, f32, f32, f32) {
        let z = self.zoom.max(1.0) as f32;
        if z <= 1.001 {
            return (0.0, 0.0, 1.0, 1.0);
        }
        let half = 0.5 / z;
        let cx = (self.zoom_cx as f32).clamp(0.0, 1.0);
        let cy = (self.zoom_cy as f32).clamp(0.0, 1.0);
        let mut u0 = cx - half;
        let mut v0 = cy - half;
        let mut u1 = cx + half;
        let mut v1 = cy + half;
        if u0 < 0.0 {
            u1 -= u0;
            u0 = 0.0;
        }
        if v0 < 0.0 {
            v1 -= v0;
            v0 = 0.0;
        }
        if u1 > 1.0 {
            u0 -= u1 - 1.0;
            u1 = 1.0;
        }
        if v1 > 1.0 {
            v0 -= v1 - 1.0;
            v1 = 1.0;
        }
        (
            u0.clamp(0.0, 1.0),
            v0.clamp(0.0, 1.0),
            u1.clamp(0.0, 1.0),
            v1.clamp(0.0, 1.0),
        )
    }

    pub fn has_zoom(&self) -> bool {
        self.zoom > 1.001
    }

    /// Texture key for a timed video frame (or still path when no time).
    pub fn media_cache_key(&self, path: &str) -> String {
        match self.video_time_sec {
            Some(t) => format!("{path}|t{t:.3}"),
            None => path.to_string(),
        }
    }

    /// Light snap only (kill float noise) — **not** stepped levels.
    /// Blur keeps ~0.01 px resolution so frame-by-frame anim is continuous.
    pub fn quantized_for_cache(&self, _animating: bool) -> Self {
        let mut e = self.clone();
        e.blur_px = ((e.blur_px * 100.0).round() / 100.0).clamp(0.0, 64.0);
        e.brightness = (e.brightness * 1000.0).round() / 1000.0;
        e.contrast = (e.contrast * 1000.0).round() / 1000.0;
        e.saturation = (e.saturation * 1000.0).round() / 1000.0;
        e.hue_shift = (e.hue_shift * 100.0).round() / 100.0;
        e.zoom = ((e.zoom * 1000.0).round() / 1000.0).clamp(1.0, 64.0);
        e.zoom_cx = ((e.zoom_cx * 1000.0).round() / 1000.0).clamp(0.0, 1.0);
        e.zoom_cy = ((e.zoom_cy * 1000.0).round() / 1000.0).clamp(0.0, 1.0);
        // Snap video time to whole frames so texture cache hits while scrubbing/playing.
        e.video_time_sec = e.video_time_sec.map(|t| {
            let fps = 30.0_f64; // display quantize; actual decode uses app fps
            ((t * fps).floor() / fps).max(0.0)
        });
        e
    }
}

/// Load still image or a video frame at `video_time_sec` (seconds).
/// `fps` maps time → frame index for [`crate::video_decode::decode_frame_cached`].
pub fn load_graph_media_rgba(
    path: &str,
    video_time_sec: Option<f64>,
    fps: f32,
) -> Option<image::RgbaImage> {
    if let Some(t) = video_time_sec {
        if t < 0.0 {
            return None;
        }
        if let Some(dur) = crate::video_decode::probe_media_duration_secs(path) {
            if t >= dur as f64 - 1e-6 {
                return None; // finished → blank
            }
        }
        let fps = fps.max(1.0);
        let frame = (t * fps as f64).floor().max(0.0) as usize;
        let (w, h, rgba) = crate::video_decode::decode_frame_cached(path, frame, fps)?;
        return image::RgbaImage::from_raw(w, h, rgba);
    }
    // Still image (or first-frame-only if someone points ObjectVideo at a video without player).
    match image::open(path) {
        Ok(img) => Some(img.to_rgba8()),
        Err(_) => {
            // Try frame 0 of a video container (cached stream).
            let (w, h, rgba) =
                crate::video_decode::decode_frame_cached(path, 0, fps.max(1.0))?;
            image::RgbaImage::from_raw(w, h, rgba)
        }
    }
}

/// Continuous preview blur for animation: downsample→upsample scales with radius
/// so every frame looks slightly different (not discrete “levels”).
/// Cheap on 96–192² previews used by the node-editor FX path.
pub fn continuous_preview_blur_rgba(img: &mut image::RgbaImage, blur_px: f32) {
    let blur = blur_px.clamp(0.0, 64.0);
    if blur < 0.05 {
        return;
    }
    let (w, h) = img.dimensions();
    if w < 2 || h < 2 {
        return;
    }
    // Continuous factor: blur 1 ≈ mild, blur 10 ≈ strong soft (frame-smooth).
    let factor = (1.0 + blur * 0.38).clamp(1.02, 16.0);
    let nw = ((w as f32) / factor).max(1.0).round() as u32;
    let nh = ((h as f32) / factor).max(1.0).round() as u32;
    if nw < w || nh < h {
        let small = image::imageops::resize(img, nw, nh, image::imageops::FilterType::Triangle);
        *img = image::imageops::resize(&small, w, h, image::imageops::FilterType::Triangle);
    }
    // Residual fine blur so mid-frame values between integer sizes still change.
    let residual = (blur / factor).clamp(0.0, 6.0);
    if residual >= 0.35 {
        fast_box_blur_rgba(img, residual);
    }
}

/// Fast multi-pass box blur (approximate Gaussian). Supports fractional radii via
/// weighted blend of floor/ceil so animation is not locked to integer levels.
pub fn fast_box_blur_rgba(img: &mut image::RgbaImage, radius_px: f32) {
    let r = radius_px.clamp(0.0, 24.0);
    if r < 0.35 {
        return;
    }
    let r0 = r.floor() as i32;
    let t = r - r0 as f32;
    // Three box passes ≈ Gaussian; smaller radius per pass.
    let apply = |img: &mut image::RgbaImage, rad: i32| {
        if rad < 1 {
            return;
        }
        let br = ((rad as f32) / 3.0_f32.sqrt()).round().max(1.0) as i32;
        for _ in 0..3 {
            box_blur_pass(img, br);
        }
    };
    if t < 0.08 || r0 >= 24 {
        apply(img, r0.max(1));
        return;
    }
    // Blend floor vs ceil radius for continuous mid-frame look.
    let mut a = img.clone();
    let mut b = img.clone();
    apply(&mut a, r0.max(1));
    apply(&mut b, (r0 + 1).min(24));
    for (out, (pa, pb)) in img.pixels_mut().zip(a.pixels().zip(b.pixels())) {
        for c in 0..4 {
            let va = pa.0[c] as f32;
            let vb = pb.0[c] as f32;
            out.0[c] = (va * (1.0 - t) + vb * t).round().clamp(0.0, 255.0) as u8;
        }
    }
}

fn box_blur_pass(img: &mut image::RgbaImage, radius: i32) {
    if radius < 1 {
        return;
    }
    let (w, h) = img.dimensions();
    let w = w as i32;
    let h = h as i32;
    if w < 1 || h < 1 {
        return;
    }
    let src: Vec<[u8; 4]> = img.pixels().map(|p| p.0).collect();
    let mut tmp = vec![[0u8; 4]; src.len()];
    let idx = |x: i32, y: i32| -> usize { (y * w + x) as usize };
    let cx = |x: i32| x.clamp(0, w - 1);
    let cy = |y: i32| y.clamp(0, h - 1);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            for dx in -radius..=radius {
                let p = src[idx(cx(x + dx), y)];
                for c in 0..4 {
                    acc[c] += p[c] as u32;
                }
                n += 1;
            }
            tmp[idx(x, y)] = [
                (acc[0] / n) as u8,
                (acc[1] / n) as u8,
                (acc[2] / n) as u8,
                (acc[3] / n) as u8,
            ];
        }
    }
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            for dy in -radius..=radius {
                let p = tmp[idx(x, cy(y + dy))];
                for c in 0..4 {
                    acc[c] += p[c] as u32;
                }
                n += 1;
            }
            img.put_pixel(
                x as u32,
                y as u32,
                image::Rgba([
                    (acc[0] / n) as u8,
                    (acc[1] / n) as u8,
                    (acc[2] / n) as u8,
                    (acc[3] / n) as u8,
                ]),
            );
        }
    }
}

/// Downscale so the longest side is at most `max_side` (nearest).
pub fn downscale_rgba_max_side(img: &image::RgbaImage, max_side: u32) -> image::RgbaImage {
    let (w, h) = img.dimensions();
    let long = w.max(h);
    if long <= max_side || max_side < 8 {
        return img.clone();
    }
    let scale = max_side as f32 / long as f32;
    let nw = ((w as f32) * scale).round().max(1.0) as u32;
    let nh = ((h as f32) * scale).round().max(1.0) as u32;
    image::imageops::resize(img, nw, nh, image::imageops::FilterType::Triangle)
}

/// Export-only blur: single downscale→upscale, **no** residual box passes.
/// Live preview keeps `continuous_preview_blur_rgba` (looks smoother, costs more).
pub fn export_fast_blur_rgba(img: &mut image::RgbaImage, blur_px: f32) {
    let blur = blur_px.clamp(0.0, 64.0);
    if blur < 0.05 {
        return;
    }
    let (w, h) = img.dimensions();
    if w < 2 || h < 2 {
        return;
    }
    // Stronger downscale than live preview so export stays cheap at video scale.
    let factor = (1.0 + blur * 0.55).clamp(1.05, 24.0);
    let nw = ((w as f32) / factor).max(1.0).round() as u32;
    let nh = ((h as f32) / factor).max(1.0).round() as u32;
    if nw >= w && nh >= h {
        return;
    }
    let small = image::imageops::resize(img, nw, nh, image::imageops::FilterType::Triangle);
    *img = image::imageops::resize(&small, w, h, image::imageops::FilterType::Triangle);
}

/// Decode + downscale + apply FX for Output Object FilePath (export / software path).
///
/// `blur_step` quantizes blur for cache reuse (Draft=2, Normal=1, High=0.5).
/// When `base_cache` / `fx_cache` are provided, reuse decoded base and baked FX across frames.
pub fn bake_graph_output_rgba(
    path: &str,
    eval: &GraphOutputEval,
    max_side: u32,
    blur_step: f32,
    mut base_cache: Option<&mut std::collections::HashMap<String, image::RgbaImage>>,
    mut fx_cache: Option<&mut std::collections::HashMap<String, image::RgbaImage>>,
) -> Option<image::RgbaImage> {
    let mut q = eval.quantized_for_cache(true);
    let step = blur_step.max(0.25) as f64;
    q.blur_px = ((q.blur_px / step).round() * step).clamp(0.0, 64.0);
    // Cap bake size — image is scaled onto the page; High allows 512.
    let max_side = max_side.max(64).min(1024);
    let media_tag = q
        .video_time_sec
        .map(|t| format!("|t{t:.3}"))
        .unwrap_or_default();
    let base_key = format!("{path}{media_tag}|ms{max_side}");
    // Export must bake zoom into pixels — include in cache key.
    let fx_key = format!(
        "{}|ms{}|z{:.3}@{:.3},{:.3}",
        q.fx_cache_key(path),
        max_side,
        q.zoom,
        q.zoom_cx,
        q.zoom_cy
    );

    if let Some(cache) = fx_cache.as_ref() {
        if let Some(img) = cache.get(&fx_key) {
            return Some(img.clone());
        }
    }

    let load_base = || {
        load_graph_media_rgba(path, q.video_time_sec, 30.0).unwrap_or_else(|| {
            eprintln!(
                "Warning: Failed to open media {:?}, using fallback placeholder.",
                path
            );
            let mut fallback = image::RgbaImage::new(64, 64);
            for px in fallback.pixels_mut() {
                *px = image::Rgba([128, 128, 128, 128]);
            }
            fallback
        })
    };

    let base = if let Some(cache) = base_cache.as_mut() {
        if let Some(img) = cache.get(&base_key) {
            img.clone()
        } else {
            let rgba = load_base();
            let down = downscale_rgba_max_side(&rgba, max_side);
            cache.insert(base_key.clone(), down.clone());
            down
        }
    } else {
        let rgba = load_base();
        downscale_rgba_max_side(&rgba, max_side)
    };

    let mut rgba = base;
    // Zoom first (export bake); live preview does this as UV at paint time.
    if q.has_zoom() {
        apply_zoom_crop_export(&mut rgba, &q);
    }

    if !q.needs_pixel_fx() {
        if let Some(cache) = fx_cache.as_mut() {
            cache.insert(fx_key, rgba.clone());
        }
        return Some(rgba);
    }

    let mut color_only = q.clone();
    let br = q.blur_px.clamp(0.0, 64.0) as f32;
    color_only.blur_px = 0.0;
    apply_graph_image_fx(&mut rgba, &color_only);
    if br >= 0.05 {
        export_fast_blur_rgba(&mut rgba, br);
    }
    if let Some(cache) = fx_cache.as_mut() {
        cache.insert(fx_key, rgba.clone());
    }
    Some(rgba)
}

/// Apply color / blur effects (in place).
///
/// **Zoom is not applied here for live preview** — that is free UV sampling at paint time.
/// Export bake paths that need baked zoom should call [`apply_zoom_crop_export`] first.
pub fn apply_graph_image_fx(img: &mut image::RgbaImage, eval: &GraphOutputEval) {
    if !eval.needs_pixel_fx() {
        return;
    }
    let bright = eval.brightness.clamp(0.0, 8.0) as f32;
    let contrast = eval.contrast.clamp(0.0, 8.0) as f32;
    let sat = eval.saturation.clamp(0.0, 8.0) as f32;
    let hue = eval.hue_shift as f32;
    for pixel in img.pixels_mut() {
        let [r, g, b, a] = pixel.0;
        let mut rf = r as f32 / 255.0;
        let mut gf = g as f32 / 255.0;
        let mut bf = b as f32 / 255.0;
        if (contrast - 1.0).abs() > 1e-6 {
            rf = (rf - 0.5) * contrast + 0.5;
            gf = (gf - 0.5) * contrast + 0.5;
            bf = (bf - 0.5) * contrast + 0.5;
        }
        if (bright - 1.0).abs() > 1e-6 {
            rf *= bright;
            gf *= bright;
            bf *= bright;
        }
        if (sat - 1.0).abs() > 1e-6 {
            let lum = 0.2126 * rf + 0.7152 * gf + 0.0722 * bf;
            rf = lum + (rf - lum) * sat;
            gf = lum + (gf - lum) * sat;
            bf = lum + (bf - lum) * sat;
        }
        if hue.abs() > 1e-3 {
            let (h0, s0, l0) = fx_rgb_to_hsl(rf, gf, bf);
            let (nr, ng, nb) = fx_hsl_to_rgb((h0 + hue).rem_euclid(360.0), s0, l0);
            rf = nr;
            gf = ng;
            bf = nb;
        }
        pixel.0 = [
            (rf.clamp(0.0, 1.0) * 255.0).round() as u8,
            (gf.clamp(0.0, 1.0) * 255.0).round() as u8,
            (bf.clamp(0.0, 1.0) * 255.0).round() as u8,
            a,
        ];
    }
    let radius = eval.blur_px.clamp(0.0, 64.0);
    if radius >= 0.05 {
        // Continuous Gaussian — radius maps to σ every frame (not stepped levels).
        gaussian_blur_rgba(img, radius as f32);
    }
}

/// CPU zoom crop for **export** only (live path uses UV — no lag).
/// Uses Nearest when zooming heavily on already-downscaled previews for speed.
pub fn apply_zoom_crop_export(img: &mut image::RgbaImage, eval: &GraphOutputEval) {
    let z = eval.zoom.max(1.0);
    if z <= 1.001 {
        return;
    }
    let (w, h) = img.dimensions();
    if w < 2 || h < 2 {
        return;
    }
    let crop_w = ((w as f64) / z).floor().max(1.0) as u32;
    let crop_h = ((h as f64) / z).floor().max(1.0) as u32;
    let cx = eval.zoom_cx.clamp(0.0, 1.0);
    let cy = eval.zoom_cy.clamp(0.0, 1.0);
    let mut x0 = (cx * w as f64 - crop_w as f64 * 0.5).round() as i64;
    let mut y0 = (cy * h as f64 - crop_h as f64 * 0.5).round() as i64;
    x0 = x0.clamp(0, (w.saturating_sub(crop_w)) as i64);
    y0 = y0.clamp(0, (h.saturating_sub(crop_h)) as i64);
    let cropped = image::imageops::crop_imm(img, x0 as u32, y0 as u32, crop_w, crop_h).to_image();
    // Triangle is expensive; Nearest is fine for large zooms / export intermediates.
    let filter = if z >= 2.0 || w.max(h) <= 512 {
        image::imageops::FilterType::Nearest
    } else {
        image::imageops::FilterType::Triangle
    };
    *img = image::imageops::resize(&cropped, w, h, filter);
}

fn fx_rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) * 0.5;
    if (max - min).abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if (max - r).abs() < 1e-6 {
        ((g - b) / d + if g < b { 6.0 } else { 0.0 }) * 60.0
    } else if (max - g).abs() < 1e-6 {
        ((b - r) / d + 2.0) * 60.0
    } else {
        ((r - g) / d + 4.0) * 60.0
    };
    (h, s, l)
}

fn fx_hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s.abs() < 1e-6 {
        return (l, l, l);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hk = h / 360.0;
    let t = |t: f32| {
        let mut t = t;
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    (t(hk + 1.0 / 3.0), t(hk), t(hk - 1.0 / 3.0))
}

/// Build a normalized 1D Gaussian kernel. `sigma` in pixels; taps cover ~3σ each side.
pub fn gaussian_kernel(sigma: f32) -> Vec<f32> {
    let sigma = sigma.max(0.25);
    let radius = (sigma * 3.0).ceil() as i32;
    let radius = radius.clamp(1, 48);
    let mut k = Vec::with_capacity((radius * 2 + 1) as usize);
    let mut sum = 0.0_f32;
    let inv = 1.0 / (2.0 * sigma * sigma);
    for i in -radius..=radius {
        let w = (-(i as f32) * (i as f32) * inv).exp();
        k.push(w);
        sum += w;
    }
    if sum > 1e-8 {
        for w in &mut k {
            *w /= sum;
        }
    }
    k
}

/// Separable Gaussian blur (horizontal + vertical). Radius ≈ user blur_px (sigma = r/2).
/// For large radii, cascades 2 half-σ passes for smoother GPU-like falloff.
pub fn gaussian_blur_rgba(img: &mut image::RgbaImage, radius_px: f32) {
    if radius_px < 0.05 {
        return;
    }
    // Map UI "radius" to σ so r≈3 looks similar to prior box radius feel but smoother.
    // Continuous in radius_px — each animation frame can differ.
    let sigma = (radius_px * 0.5).clamp(0.12, 32.0);
    if sigma > 8.0 {
        // Cascade: two smaller gaussians ≈ one larger (σ_total² = σ1² + σ2²).
        let half = (sigma * sigma * 0.5).sqrt();
        gaussian_blur_pass(img, half);
        gaussian_blur_pass(img, half);
    } else {
        gaussian_blur_pass(img, sigma);
    }
}

fn gaussian_blur_pass(img: &mut image::RgbaImage, sigma: f32) {
    let kernel = gaussian_kernel(sigma);
    let radius = (kernel.len() as i32 - 1) / 2;
    let (w, h) = img.dimensions();
    let w = w as i32;
    let h = h as i32;
    if w < 1 || h < 1 {
        return;
    }
    let src: Vec<[u8; 4]> = img.pixels().map(|p| p.0).collect();
    let mut tmp = vec![[0u8; 4]; src.len()];
    let idx = |x: i32, y: i32| -> usize { (y * w + x) as usize };
    let clamp_x = |x: i32| x.clamp(0, w - 1);
    let clamp_y = |y: i32| y.clamp(0, h - 1);

    // Horizontal
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0_f32; 4];
            for (ki, &weight) in kernel.iter().enumerate() {
                let dx = ki as i32 - radius;
                let p = src[idx(clamp_x(x + dx), y)];
                for c in 0..4 {
                    acc[c] += p[c] as f32 * weight;
                }
            }
            tmp[idx(x, y)] = [
                acc[0].round().clamp(0.0, 255.0) as u8,
                acc[1].round().clamp(0.0, 255.0) as u8,
                acc[2].round().clamp(0.0, 255.0) as u8,
                acc[3].round().clamp(0.0, 255.0) as u8,
            ];
        }
    }
    // Vertical
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0_f32; 4];
            for (ki, &weight) in kernel.iter().enumerate() {
                let dy = ki as i32 - radius;
                let p = tmp[idx(x, clamp_y(y + dy))];
                for c in 0..4 {
                    acc[c] += p[c] as f32 * weight;
                }
            }
            img.put_pixel(
                x as u32,
                y as u32,
                image::Rgba([
                    acc[0].round().clamp(0.0, 255.0) as u8,
                    acc[1].round().clamp(0.0, 255.0) as u8,
                    acc[2].round().clamp(0.0, 255.0) as u8,
                    acc[3].round().clamp(0.0, 255.0) as u8,
                ]),
            );
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphView {
    #[serde(default)]
    pub pan_x: f32,
    #[serde(default)]
    pub pan_y: f32,
    #[serde(default = "default_zoom")]
    pub zoom: f32,
}

fn default_zoom() -> f32 {
    1.0
}

impl Default for GraphView {
    fn default() -> Self {
        Self {
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeGraph {
    #[serde(default)]
    pub nodes: IndexMap<Uuid, GraphNode>,
    #[serde(default)]
    pub links: Vec<GraphLink>,
    #[serde(default)]
    pub view: GraphView,
    #[serde(default)]
    pub parameters: Vec<GraphParam>,
    /// Primary Output Object node (continuous video sink).
    #[serde(default)]
    pub output_node_id: Option<Uuid>,
    /// Last evaluation error for the root output (propagated from broken links).
    #[serde(default)]
    pub root_error: Option<String>,
    /// Cached Real outputs from last [`Self::eval_reals`] (not persisted).
    #[serde(skip)]
    pub last_real_values: std::collections::HashMap<Uuid, f64>,
    /// Port-scoped Real outputs (Mouse Encoder x/y/shakiness/event, etc.).
    #[serde(skip)]
    pub last_real_port_values: std::collections::HashMap<(Uuid, String), f64>,
}

impl Default for NodeGraph {
    fn default() -> Self {
        Self::new_empty()
    }
}

impl NodeGraph {
    pub fn new_empty() -> Self {
        let mut g = Self {
            nodes: IndexMap::new(),
            links: Vec::new(),
            view: GraphView::default(),
            parameters: Vec::new(),
            output_node_id: None,
            root_error: None,
            last_real_values: std::collections::HashMap::new(),
            last_real_port_values: std::collections::HashMap::new(),
        };
        // Seed with an Output Object so the layer has a clear sink.
        let out = GraphNode::new(GraphNodeKind::OutputObject, 280.0, 120.0);
        g.output_node_id = Some(out.id);
        g.nodes.insert(out.id, out);
        g
    }

    pub fn add_node(&mut self, kind: GraphNodeKind, x: f32, y: f32) -> Uuid {
        let node = GraphNode::new(kind, x, y);
        let id = node.id;
        // Keep the first Output Object as the primary sink — a second one must not
        // steal image/sound resolve (users often spawn extras by accident).
        if matches!(node.kind, GraphNodeKind::OutputObject) && self.output_node_id.is_none() {
            self.output_node_id = Some(id);
        }
        self.nodes.insert(id, node);
        id
    }

    /// Primary Output Object, or any Output Object that has links if primary is unset.
    fn primary_output_id(&self) -> Option<Uuid> {
        if let Some(id) = self.output_node_id {
            if self.nodes.contains_key(&id) {
                return Some(id);
            }
        }
        self.nodes
            .values()
            .find(|n| matches!(n.kind, GraphNodeKind::OutputObject))
            .map(|n| n.id)
    }

    pub fn remove_node(&mut self, id: Uuid) {
        self.nodes.shift_remove(&id);
        self.links
            .retain(|l| l.from_node != id && l.to_node != id);
        if self.output_node_id == Some(id) {
            self.output_node_id = self
                .nodes
                .values()
                .find(|n| matches!(n.kind, GraphNodeKind::OutputObject))
                .map(|n| n.id);
        }
        // Drop Parameter-tab entries that no longer have a Param* node.
        self.sync_parameters_with_nodes();
    }

    /// Keep `parameters` in sync with ParamReal / ParamColor / ParamPosition nodes.
    /// Orphan entries (no node) are removed; tab only shows live node params.
    pub fn sync_parameters_with_nodes(&mut self) {
        use std::collections::HashSet;
        let used: HashSet<Uuid> = self
            .nodes
            .values()
            .filter_map(|n| match n.kind {
                GraphNodeKind::ParamReal { param_id }
                | GraphNodeKind::ParamColor { param_id }
                | GraphNodeKind::ParamPosition { param_id } => Some(param_id),
                _ => None,
            })
            .collect();
        self.parameters.retain(|p| used.contains(&p.id));
    }

    pub fn port_type(&self, node_id: Uuid, port_id: &str) -> Option<PortType> {
        let node = self.nodes.get(&node_id)?;
        node.ports()
            .into_iter()
            .find(|p| p.id == port_id)
            .map(|p| p.ty)
    }

    pub fn try_add_link(
        &mut self,
        from_node: Uuid,
        from_port: &str,
        to_node: Uuid,
        to_port: &str,
    ) -> Result<(), String> {
        if from_node == to_node {
            return Err("Cannot connect a node to itself".into());
        }
        let from_ty = self
            .port_type(from_node, from_port)
            .ok_or("Unknown output port")?;
        let to_ty = self
            .port_type(to_node, to_port)
            .ok_or("Unknown input port")?;
        let from_dir = self
            .nodes
            .get(&from_node)
            .and_then(|n| n.ports().into_iter().find(|p| p.id == from_port))
            .map(|p| p.dir);
        let to_dir = self
            .nodes
            .get(&to_node)
            .and_then(|n| n.ports().into_iter().find(|p| p.id == to_port))
            .map(|p| p.dir);
        if from_dir != Some(PortDir::Output) || to_dir != Some(PortDir::Input) {
            return Err("Wire must go from output to input".into());
        }
        if !PortType::can_connect(from_ty, to_ty) {
            return Err(format!(
                "Type mismatch: {} → {}",
                from_ty.label(),
                to_ty.label()
            ));
        }
        // One link per input port.
        self.links
            .retain(|l| !(l.to_node == to_node && l.to_port == to_port));
        self.links
            .push(GraphLink::new(from_node, from_port, to_node, to_port));
        Ok(())
    }

    /// Drop links that reference missing app objects; set root_error if Output is affected.
    pub fn prune_dead_object_links(&mut self, living_nodes: &std::collections::HashSet<Uuid>) {
        let mut broken = false;
        let mut dead_nodes = Vec::new();
        for (id, node) in &mut self.nodes {
            if let GraphNodeKind::ObjectFromApp { node_ids } = &mut node.kind {
                let before = node_ids.len();
                node_ids.retain(|nid| living_nodes.contains(nid));
                if node_ids.is_empty() && before > 0 {
                    node.error = Some("Source object deleted".into());
                    broken = true;
                    dead_nodes.push(*id);
                } else if !node_ids.is_empty() {
                    node.error = None;
                }
            }
        }
        if broken {
            // Remove outbound links from broken object nodes.
            self.links.retain(|l| !dead_nodes.contains(&l.from_node));
            self.root_error = Some("Graph error: missing source object (check Output Object)".into());
        } else if self.nodes.values().all(|n| n.error.is_none()) {
            self.root_error = None;
        }
    }

    /// Incoming Real link to `(to_node, to_port)` → source node id.
    pub fn real_input_source(&self, to_node: Uuid, to_port: &str) -> Option<Uuid> {
        self.links.iter().find_map(|l| {
            if l.to_node == to_node && l.to_port == to_port {
                let ty = self.port_type(l.from_node, &l.from_port)?;
                if ty == PortType::Real {
                    Some(l.from_node)
                } else {
                    None
                }
            } else {
                None
            }
        })
    }

    /// Incoming link source node for any port.
    pub fn input_source_node(&self, to_node: Uuid, to_port: &str) -> Option<Uuid> {
        self.links.iter().find_map(|l| {
            if l.to_node == to_node && l.to_port == to_port {
                Some(l.from_node)
            } else {
                None
            }
        })
    }

    /// Resolve Output Object image input for canvas compositing (P2).
    /// Walks pass-through effect/geometry nodes; applies Brightness.amount from last Real eval.
    /// Blank when past Output Object `run_till` (seconds, if wired > 0).
    pub fn resolve_output_image(&self) -> GraphOutputEval {
        let run_till = self.output_run_till_secs();
        if run_till > 1e-9 && self.last_time_secs() >= run_till - 1e-9 {
            return GraphOutputEval::default();
        }
        // Prefer primary Output; if its image is empty, try other Output Objects
        // (legacy graphs may have image on one sink and sound on another).
        let mut tried = std::collections::HashSet::new();
        if let Some(out_id) = self.primary_output_id() {
            tried.insert(out_id);
            let ev = self.resolve_image_chain(out_id, "image", 0);
            if !matches!(ev.image, GraphImageSource::Empty) {
                return ev;
            }
        }
        for n in self.nodes.values() {
            if matches!(n.kind, GraphNodeKind::OutputObject) && tried.insert(n.id) {
                let ev = self.resolve_image_chain(n.id, "image", 0);
                if !matches!(ev.image, GraphImageSource::Empty) {
                    return ev;
                }
            }
        }
        GraphOutputEval::default()
    }

    /// Resolve Output Object sound input for playback / export (P5).
    /// Silent when past Output Object `run_till` (seconds, if wired > 0).
    pub fn resolve_output_sound(&self) -> GraphOutputSound {
        let run_till = self.output_run_till_secs();
        if run_till > 1e-9 && self.last_time_secs() >= run_till - 1e-9 {
            return GraphOutputSound::default();
        }
        let mut tried = std::collections::HashSet::new();
        if let Some(out_id) = self.primary_output_id() {
            tried.insert(out_id);
            let s = self.resolve_sound_chain(out_id, "sound", 0);
            if s.path().is_some() {
                return s;
            }
        }
        for n in self.nodes.values() {
            if matches!(n.kind, GraphNodeKind::OutputObject) && tried.insert(n.id) {
                let s = self.resolve_sound_chain(n.id, "sound", 0);
                if s.path().is_some() {
                    return s;
                }
            }
        }
        GraphOutputSound::default()
    }

    fn resolve_sound_chain(&self, to_node: Uuid, to_port: &str, depth: usize) -> GraphOutputSound {
        let mut out = GraphOutputSound::default();
        if depth > 64 {
            return out;
        }
        let Some(src_id) = self.input_source_node(to_node, to_port) else {
            return out;
        };
        let Some(node) = self.nodes.get(&src_id) else {
            return out;
        };
        match &node.kind {
            GraphNodeKind::ObjectAudio { path } => {
                if path.trim().is_empty() {
                    out.sound = GraphSoundSource::Empty;
                } else {
                    out.sound = GraphSoundSource::FilePath(path.clone());
                }
            }
            GraphNodeKind::ObjectVideo { path } => {
                // Sound pin only — video pin is video-only (no mixed A/V).
                if path.trim().is_empty() {
                    out.sound = GraphSoundSource::Empty;
                } else {
                    out.sound = GraphSoundSource::FilePath(path.clone());
                }
            }
            GraphNodeKind::VideoPlayer => {
                // Timed window: media_t = start + time*speed; blank outside [start, start+duration).
                // Prefer optional `audio` input (ObjectVideo.sound / ObjectAudio); else demux video file.
                let (path, t, rate, silent) = self.video_player_media_window(src_id, depth + 1);
                if silent || path.is_empty() {
                    out.sound = GraphSoundSource::Empty;
                } else {
                    out.sound = GraphSoundSource::FilePath(path);
                    out.media_time_sec = Some(t);
                    out.playback_rate = rate;
                }
            }
            GraphNodeKind::SepticPlayer => {
                // Sound from sibling/session video (muxed system audio when capture_audio was on).
                let septic = self.resolve_septic_path(src_id, "septic", depth + 1);
                let t_req = self
                    .real_input_source(src_id, "time")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(0.0);
                if let Some(sp) = septic {
                    if let Ok(session) =
                        super::septic::SepticSession::load_path_cached_arc(std::path::Path::new(&sp))
                    {
                        let t = session.truth_time(t_req);
                        if let Some(vp) = super::septic::resolve_video_path(
                            std::path::Path::new(&sp),
                            &session.meta,
                        ) {
                            out.sound =
                                GraphSoundSource::FilePath(vp.to_string_lossy().into_owned());
                            out.media_time_sec = Some(t);
                            out.playback_rate = 1.0;
                        } else if !session.meta.video_path.is_empty() {
                            out.sound =
                                GraphSoundSource::FilePath(session.meta.video_path.clone());
                            out.media_time_sec = Some(t);
                            out.playback_rate = 1.0;
                        }
                    }
                }
            }
            GraphNodeKind::Equalizer => {
                let bass = self
                    .real_input_source(src_id, "bass")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(0.0);
                let mid = self
                    .real_input_source(src_id, "mid")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(0.0);
                let treble = self
                    .real_input_source(src_id, "treble")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(0.0);
                let vol = self
                    .real_input_source(src_id, "volume")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(1.0);
                let mut inner = self.resolve_sound_chain(src_id, "in", depth + 1);
                inner.eq_bass += bass;
                inner.eq_mid += mid;
                inner.eq_treble += treble;
                inner.volume *= vol.max(0.0);
                // media_time_sec preserved from VideoPlayer upstream.
                return inner;
            }
            // Pass-through anything that only has image (ignore).
            _ => {
                // If a node has a sound "out", try its primary sound input.
                let ports = node.ports();
                if ports
                    .iter()
                    .any(|p| p.dir == PortDir::Output && p.ty == PortType::RawSound)
                {
                    if let Some(inp) = ports
                        .iter()
                        .find(|p| p.dir == PortDir::Input && p.ty == PortType::RawSound)
                    {
                        return self.resolve_sound_chain(src_id, &inp.id, depth + 1);
                    }
                }
            }
        }
        out
    }

    /// Resolve the image produced *at* `node_id` (for preview).
    /// For effect/geometry nodes this is the transformed upstream image.
    pub fn resolve_node_image_out(&self, node_id: Uuid) -> GraphOutputEval {
        let Some(node) = self.nodes.get(&node_id) else {
            return GraphOutputEval::default();
        };
        // Leaf sources.
        match &node.kind {
            GraphNodeKind::ObjectFromApp { node_ids } => {
                return GraphOutputEval {
                    image: GraphImageSource::AppObjects(node_ids.clone()),
                    ..Default::default()
                };
            }
            GraphNodeKind::ObjectImage { path } | GraphNodeKind::ObjectVideo { path } => {
                return GraphOutputEval {
                    image: if path.trim().is_empty() {
                        GraphImageSource::Empty
                    } else {
                        GraphImageSource::FilePath(path.clone())
                    },
                    ..Default::default()
                };
            }
            GraphNodeKind::VideoPlayer => {
                return self.resolve_video_player(node_id, 0);
            }
            GraphNodeKind::SepticPlayer => {
                return self.resolve_septic_player_video(node_id, 0);
            }
            GraphNodeKind::OutputObject => {
                return self.resolve_image_chain(node_id, "image", 0);
            }
            _ => {}
        }
        // Effect / geometry: apply this node on top of its primary image input.
        // Use a synthetic "from this node into a dummy" by reusing resolve on children + self match.
        // Easiest: resolve chain into a virtual port by matching on kind (same as resolve_image_chain arms).
        let ports = node.ports();
        let has_img_out = ports
            .iter()
            .any(|p| p.dir == PortDir::Output && p.ty == PortType::RawImage);
        if !has_img_out {
            return GraphOutputEval::default();
        }
        // Walk as if Output asked for this node's image: inject a fake link from node_id.
        // Implement by calling the same arms with to_port pointing at this node's inputs.
        match &node.kind {
            GraphNodeKind::Brightness
            | GraphNodeKind::ColorChanger
            | GraphNodeKind::LinearBlur
            | GraphNodeKind::Zoom
            | GraphNodeKind::Speed
            | GraphNodeKind::GeoSize
            | GraphNodeKind::GeoPlacement
            | GraphNodeKind::GeoRotate
            | GraphNodeKind::GeoTrapezoid
            | GraphNodeKind::GeoMirror
            | GraphNodeKind::VideoPlayer => {
                // Recurse into self by temporarily using resolve_image_chain from a fake consumer.
                // resolve_image_chain looks at *input* of to_node — so invent call from ports.
                self.resolve_effect_as_root(node_id)
            }
            GraphNodeKind::GeoAdd => {
                let mut inner = self.resolve_image_chain(node_id, "a", 0);
                if matches!(inner.image, GraphImageSource::Empty) {
                    inner = self.resolve_image_chain(node_id, "b", 0);
                }
                inner.effects_on_path = true;
                inner
            }
            _ => GraphOutputEval::default(),
        }
    }

    /// Apply one effect/geometry node as root (input(s) resolved, then this node).
    fn resolve_effect_as_root(&self, node_id: Uuid) -> GraphOutputEval {
        // Mirror resolve_image_chain arms for effect nodes, but start *at* this node.
        let Some(node) = self.nodes.get(&node_id) else {
            return GraphOutputEval::default();
        };
        match &node.kind {
            GraphNodeKind::Brightness => {
                let amount = self
                    .real_input_source(node_id, "amount")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(1.0);
                let mut inner = self.resolve_image_chain(node_id, "in", 0);
                inner.brightness *= amount;
                inner.effects_on_path = true;
                inner
            }
            GraphNodeKind::ColorChanger => {
                let hue = self
                    .real_input_source(node_id, "hue")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(0.0);
                let sat = self
                    .real_input_source(node_id, "sat")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(1.0);
                let mut inner = self.resolve_image_chain(node_id, "in", 0);
                inner.hue_shift += hue;
                inner.saturation *= sat;
                inner.effects_on_path = true;
                inner
            }
            GraphNodeKind::LinearBlur => {
                let amount = self
                    .real_input_source(node_id, "amount")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(0.0);
                let mut inner = self.resolve_image_chain(node_id, "in", 0);
                inner.blur_px += amount.max(0.0).min(128.0);
                inner.effects_on_path = true;
                inner
            }
            GraphNodeKind::Zoom => {
                let (cx, cy) = self.resolve_position_input(node_id, "pos", 0.5, 0.5);
                let z = self
                    .real_input_value(node_id, "zoom")
                    .or_else(|| {
                        self.real_input_source(node_id, "zoom")
                            .and_then(|id| self.last_real_out(id))
                    })
                    .unwrap_or(1.0)
                    .clamp(1.0, 64.0);
                let mut inner = self.resolve_image_chain(node_id, "in", 0);
                let prev = inner.zoom.max(1.0);
                inner.zoom = (prev * z).clamp(1.0, 64.0);
                if z > 1.001 {
                    inner.zoom_cx = cx.clamp(0.0, 1.0);
                    inner.zoom_cy = cy.clamp(0.0, 1.0);
                }
                inner.effects_on_path = true;
                inner
            }
            GraphNodeKind::Speed => {
                let factor = self
                    .real_input_source(node_id, "factor")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(1.0);
                let mut inner = self.resolve_image_chain(node_id, "in", 0);
                inner.speed *= factor.max(0.0);
                inner.effects_on_path = true;
                inner
            }
            GraphNodeKind::VideoPlayer => self.resolve_video_player(node_id, 0),
            GraphNodeKind::GeoSize => {
                let w = self
                    .real_input_source(node_id, "w")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(1.0);
                let h = self
                    .real_input_source(node_id, "h")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(1.0);
                let mut inner = self.resolve_image_chain(node_id, "in", 0);
                inner.geo_scale_w *= w.max(0.01);
                inner.geo_scale_h *= h.max(0.01);
                inner.effects_on_path = true;
                inner
            }
            GraphNodeKind::GeoPlacement => {
                let x = self
                    .real_input_source(node_id, "x")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(0.0);
                let y = self
                    .real_input_source(node_id, "y")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(0.0);
                let mut inner = self.resolve_image_chain(node_id, "in", 0);
                inner.geo_off_x += x;
                inner.geo_off_y += y;
                inner.effects_on_path = true;
                inner
            }
            GraphNodeKind::GeoRotate => {
                let angle = self
                    .real_input_source(node_id, "angle")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(0.0);
                let mut inner = self.resolve_image_chain(node_id, "in", 0);
                inner.geo_rot_deg += angle;
                inner.effects_on_path = true;
                inner
            }
            GraphNodeKind::GeoTrapezoid => {
                let mut inner = self.resolve_image_chain(node_id, "in", 0);
                inner.effects_on_path = true;
                inner
            }
            GraphNodeKind::GeoMirror => {
                let axis = self
                    .real_input_source(node_id, "axis")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(1.0);
                let mut inner = self.resolve_image_chain(node_id, "in", 0);
                let a = axis.round() as i32;
                let prev = inner.geo_mirror.round() as i32;
                inner.geo_mirror = (prev | a) as f64;
                inner.effects_on_path = true;
                inner
            }
            _ => GraphOutputEval::default(),
        }
    }

    /// Whether this node has image/video input and/or output ports (for preview).
    pub fn image_port_dirs(kind: &GraphNodeKind) -> (bool, bool) {
        let ports = kind.ports();
        let is_vis =
            |ty: PortType| matches!(ty, PortType::RawImage | PortType::RawVideo);
        let has_in = ports
            .iter()
            .any(|p| p.dir == PortDir::Input && is_vis(p.ty));
        let has_out = ports
            .iter()
            .any(|p| p.dir == PortDir::Output && is_vis(p.ty));
        (has_in, has_out)
    }

    fn resolve_image_chain(&self, to_node: Uuid, to_port: &str, depth: usize) -> GraphOutputEval {
        let mut out = GraphOutputEval::default();
        if depth > 64 {
            return out;
        }
        let Some(src_id) = self.input_source_node(to_node, to_port) else {
            return out;
        };
        let Some(node) = self.nodes.get(&src_id) else {
            return out;
        };
        match &node.kind {
            GraphNodeKind::ObjectFromApp { node_ids } => {
                out.image = GraphImageSource::AppObjects(node_ids.clone());
            }
            GraphNodeKind::ObjectImage { path } | GraphNodeKind::ObjectVideo { path } => {
                if path.trim().is_empty() {
                    out.image = GraphImageSource::Empty;
                } else {
                    out.image = GraphImageSource::FilePath(path.clone());
                }
            }
            GraphNodeKind::VideoPlayer => {
                return self.resolve_video_player(src_id, depth + 1);
            }
            GraphNodeKind::SepticPlayer => {
                return self.resolve_septic_player_video(src_id, depth + 1);
            }
            GraphNodeKind::Brightness => {
                let amount = self
                    .real_input_source(src_id, "amount")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(1.0);
                let mut inner = self.resolve_image_chain(src_id, "in", depth + 1);
                inner.brightness *= amount;
                inner.effects_on_path = true;
                return inner;
            }
            GraphNodeKind::Zoom => {
                let (cx, cy) = self.resolve_position_input(src_id, "pos", 0.5, 0.5);
                let z = self
                    .real_input_value(src_id, "zoom")
                    .or_else(|| {
                        self.real_input_source(src_id, "zoom")
                            .and_then(|id| self.last_real_out(id))
                    })
                    .unwrap_or(1.0)
                    .clamp(1.0, 64.0);
                let mut inner = self.resolve_image_chain(src_id, "in", depth + 1);
                let prev = inner.zoom.max(1.0);
                inner.zoom = (prev * z).clamp(1.0, 64.0);
                if z > 1.001 {
                    inner.zoom_cx = cx.clamp(0.0, 1.0);
                    inner.zoom_cy = cy.clamp(0.0, 1.0);
                }
                inner.effects_on_path = true;
                return inner;
            }
            GraphNodeKind::ColorChanger => {
                let hue = self
                    .real_input_source(src_id, "hue")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(0.0);
                let sat = self
                    .real_input_source(src_id, "sat")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(1.0);
                let mut inner = self.resolve_image_chain(src_id, "in", depth + 1);
                inner.hue_shift += hue;
                inner.saturation *= sat;
                inner.effects_on_path = true;
                return inner;
            }
            GraphNodeKind::LinearBlur => {
                // Default 0 when amount unconnected (was 2.0 → looked “stuck blurred”).
                let amount = self
                    .real_input_source(src_id, "amount")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(0.0);
                let mut inner = self.resolve_image_chain(src_id, "in", depth + 1);
                // Treat amount as blur radius in "document px"; clamp wild Value nodes.
                inner.blur_px += amount.max(0.0).min(128.0);
                inner.effects_on_path = true;
                return inner;
            }
            GraphNodeKind::Speed => {
                let factor = self
                    .real_input_source(src_id, "factor")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(1.0);
                let mut inner = self.resolve_image_chain(src_id, "in", depth + 1);
                inner.speed *= factor.max(0.0);
                inner.effects_on_path = true;
                return inner;
            }
            GraphNodeKind::GeoSize => {
                let w = self
                    .real_input_source(src_id, "w")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(1.0);
                let h = self
                    .real_input_source(src_id, "h")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(1.0);
                let mut inner = self.resolve_image_chain(src_id, "in", depth + 1);
                inner.geo_scale_w *= w.max(0.01);
                inner.geo_scale_h *= h.max(0.01);
                inner.effects_on_path = true;
                return inner;
            }
            GraphNodeKind::GeoPlacement => {
                let x = self
                    .real_input_source(src_id, "x")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(0.0);
                let y = self
                    .real_input_source(src_id, "y")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(0.0);
                let mut inner = self.resolve_image_chain(src_id, "in", depth + 1);
                inner.geo_off_x += x;
                inner.geo_off_y += y;
                inner.effects_on_path = true;
                return inner;
            }
            GraphNodeKind::GeoRotate => {
                let angle = self
                    .real_input_source(src_id, "angle")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(0.0);
                let mut inner = self.resolve_image_chain(src_id, "in", depth + 1);
                inner.geo_rot_deg += angle;
                inner.effects_on_path = true;
                return inner;
            }
            GraphNodeKind::GeoTrapezoid => {
                // Trapezoid corners reserved for later mesh warp; pass image through.
                let mut inner = self.resolve_image_chain(src_id, "in", depth + 1);
                inner.effects_on_path = true;
                return inner;
            }
            GraphNodeKind::GeoMirror => {
                let axis = self
                    .real_input_source(src_id, "axis")
                    .and_then(|id| self.last_real_out(id))
                    .unwrap_or(1.0);
                let mut inner = self.resolve_image_chain(src_id, "in", depth + 1);
                // OR-combine axis flags (1=H, 2=V).
                let a = axis.round() as i32;
                let prev = inner.geo_mirror.round() as i32;
                inner.geo_mirror = (prev | a) as f64;
                inner.effects_on_path = true;
                return inner;
            }
            GraphNodeKind::GeoAdd => {
                // Prefer first connected image input (A then B).
                let mut inner = self.resolve_image_chain(src_id, "a", depth + 1);
                if matches!(inner.image, GraphImageSource::Empty) {
                    inner = self.resolve_image_chain(src_id, "b", depth + 1);
                }
                inner.effects_on_path = true;
                return inner;
            }
            GraphNodeKind::OutputObject => {
                // Nested / miswired — follow its image input.
                return self.resolve_image_chain(src_id, "image", depth + 1);
            }
            _ => {
                // Non-image producer.
                out.image = GraphImageSource::Empty;
            }
        }
        out
    }

    /// Primary Real output value for a node after [`Self::eval_reals`].
    pub fn last_real_out(&self, node_id: Uuid) -> Option<f64> {
        self.last_real_values.get(&node_id).copied()
    }

    /// Real value on a specific output port (falls back to primary).
    pub fn last_real_port(&self, node_id: Uuid, port: &str) -> Option<f64> {
        self.last_real_port_values
            .get(&(node_id, port.to_string()))
            .copied()
            .or_else(|| self.last_real_out(node_id))
    }

    /// Real input on `to_port`, using the source's **from_port** when multi-out.
    pub fn real_input_value(&self, to_node: Uuid, to_port: &str) -> Option<f64> {
        let link = self.links.iter().find(|l| {
            l.to_node == to_node && l.to_port == to_port
        })?;
        let ty = self.port_type(link.from_node, &link.from_port)?;
        if ty != PortType::Real {
            return None;
        }
        self.last_real_port(link.from_node, &link.from_port)
    }

    /// Resolve a Position input (ParamPosition / Mouse Player x+y / defaults).
    pub fn resolve_position_input(
        &self,
        to_node: Uuid,
        to_port: &str,
        default_x: f64,
        default_y: f64,
    ) -> (f64, f64) {
        let Some(link) = self.links.iter().find(|l| {
            l.to_node == to_node && l.to_port == to_port
        }) else {
            return (default_x, default_y);
        };
        let Some(src) = self.nodes.get(&link.from_node) else {
            return (default_x, default_y);
        };
        match &src.kind {
            GraphNodeKind::ParamPosition { param_id } => {
                let p = self.parameters.iter().find(|p| p.id == *param_id);
                (
                    p.map(|p| p.v0).unwrap_or(default_x),
                    p.map(|p| p.v1).unwrap_or(default_y),
                )
            }
            GraphNodeKind::MouseEncoder { .. } => (
                self.last_real_port(link.from_node, "x")
                    .unwrap_or(default_x),
                self.last_real_port(link.from_node, "y")
                    .unwrap_or(default_y),
            ),
            _ => {
                // Prefer explicit x/y port values when the source emits them.
                let x = self
                    .last_real_port(link.from_node, "x")
                    .or_else(|| self.last_real_out(link.from_node))
                    .unwrap_or(default_x);
                let y = self
                    .last_real_port(link.from_node, "y")
                    .unwrap_or(default_y);
                (x, y)
            }
        }
    }

    /// Output Object `run_till` (seconds). `0` / unconnected = no limit.
    pub fn output_run_till_secs(&self) -> f64 {
        let Some(out_id) = self.primary_output_id() else {
            return 0.0;
        };
        self.real_input_value(out_id, "run_till")
            .or_else(|| {
                self.real_input_source(out_id, "run_till")
                    .and_then(|id| self.last_real_out(id))
            })
            .unwrap_or(0.0)
            .max(0.0)
    }

    /// Current algebra time (seconds) from the Time node, else 0.
    pub fn last_time_secs(&self) -> f64 {
        self.nodes
            .values()
            .find(|n| matches!(n.kind, GraphNodeKind::Time))
            .and_then(|n| self.last_real_out(n.id))
            .unwrap_or(0.0)
            .max(0.0)
    }

    /// Resolve path to a `.sepscrr` from ObjectSeptic / ObjectMouse / SepticPlayer chain.
    pub fn resolve_septic_path(&self, node_id: Uuid, port: &str, depth: usize) -> Option<String> {
        if depth > 32 {
            return None;
        }
        let Some(src) = self.input_source_node(node_id, port) else {
            // Node itself may be the septic object.
            let n = self.nodes.get(&node_id)?;
            return match &n.kind {
                GraphNodeKind::ObjectSeptic { path } | GraphNodeKind::ObjectMouse { path }
                    if !path.trim().is_empty() =>
                {
                    Some(path.clone())
                }
                _ => None,
            };
        };
        let n = self.nodes.get(&src)?;
        match &n.kind {
            GraphNodeKind::ObjectSeptic { path } | GraphNodeKind::ObjectMouse { path }
                if !path.trim().is_empty() =>
            {
                Some(path.clone())
            }
            GraphNodeKind::SepticPlayer => self.resolve_septic_path(src, "septic", depth + 1),
            _ => None,
        }
    }

    /// Evaluate all Real-producing algebra nodes for the current frame.
    /// Results stored in `last_real_values` (node_id → primary `out` value).
    /// Clears Real-related node errors and rewrites them on failure / cycles.
    pub fn eval_reals(&mut self, frame: usize, fps: f32) {
        use std::collections::{HashMap, HashSet, VecDeque};

        self.last_real_values.clear();
        self.last_real_port_values.clear();
        let fps = fps.max(1.0) as f64;
        let frame_f = frame as f64;
        let time_sec = frame_f / fps;

        // Clear prior algebra errors (keep object-deleted errors).
        for node in self.nodes.values_mut() {
            if matches!(
                node.kind,
                GraphNodeKind::Value { .. }
                    | GraphNodeKind::ExprX { .. }
                    | GraphNodeKind::ExprXy { .. }
                    | GraphNodeKind::ExprXyz { .. }
                    | GraphNodeKind::Frame
                    | GraphNodeKind::Time
                    | GraphNodeKind::ParamReal { .. }
            ) {
                if node
                    .error
                    .as_ref()
                    .is_some_and(|e| e.contains("cycle") || e.contains("Expr") || e.contains("expr"))
                {
                    node.error = None;
                }
            }
        }

        // Build adjacency for Real wires only (from → to).
        let mut real_nodes: HashSet<Uuid> = HashSet::new();
        for (id, node) in &self.nodes {
            let produces_real = node.ports().iter().any(|p| {
                p.dir == PortDir::Output && p.ty == PortType::Real
            });
            if produces_real {
                real_nodes.insert(*id);
            }
        }

        let mut indeg: HashMap<Uuid, usize> = real_nodes.iter().map(|id| (*id, 0usize)).collect();
        let mut outs: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
        for link in &self.links {
            let Some(from_ty) = self.port_type(link.from_node, &link.from_port) else {
                continue;
            };
            let Some(to_ty) = self.port_type(link.to_node, &link.to_port) else {
                continue;
            };
            if from_ty != PortType::Real || to_ty != PortType::Real {
                continue;
            }
            if !real_nodes.contains(&link.from_node) || !real_nodes.contains(&link.to_node) {
                continue;
            }
            outs.entry(link.from_node).or_default().push(link.to_node);
            *indeg.entry(link.to_node).or_default() += 1;
            indeg.entry(link.from_node).or_default();
        }

        let mut q: VecDeque<Uuid> = indeg
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(id, _)| *id)
            .collect();
        // Isolated real nodes with no links still need eval.
        for id in &real_nodes {
            if !indeg.contains_key(id) {
                q.push_back(*id);
            }
        }

        let mut order = Vec::new();
        let mut seen = HashSet::new();
        while let Some(id) = q.pop_front() {
            if !seen.insert(id) {
                continue;
            }
            order.push(id);
            if let Some(children) = outs.get(&id) {
                for c in children {
                    if let Some(d) = indeg.get_mut(c) {
                        *d = d.saturating_sub(1);
                        if *d == 0 {
                            q.push_back(*c);
                        }
                    }
                }
            }
        }

        if order.len() < real_nodes.len() {
            // Cycle among remaining.
            for id in &real_nodes {
                if !seen.contains(id) {
                    if let Some(n) = self.nodes.get_mut(id) {
                        n.error = Some("cycle in Real graph".into());
                    }
                }
            }
            self.root_error = Some("Graph error: cycle in algebra nodes".into());
        } else if self
            .root_error
            .as_ref()
            .is_some_and(|e| e.contains("cycle"))
        {
            self.root_error = None;
        }

        // Evaluate in order.
        let mut values: HashMap<Uuid, f64> = HashMap::new();
        let mut port_buf: HashMap<(Uuid, String), f64> = HashMap::new();
        for id in order {
            let Some(node) = self.nodes.get(&id) else {
                continue;
            };
            let kind = node.kind.clone();
            let result = match &kind {
                GraphNodeKind::Value { value } => Ok(*value),
                GraphNodeKind::Frame => Ok(frame_f),
                GraphNodeKind::Time => Ok(time_sec),
                GraphNodeKind::Visualizer { gain } => {
                    // Level 0..1 driven by optional frequency (Hz) and gain.
                    // `frequency` ≤ 0 → broadband envelope; else band-energy proxy at f Hz.
                    let g_in = self
                        .real_input_source(id, "gain")
                        .and_then(|src| values.get(&src).copied())
                        .unwrap_or(*gain)
                        .clamp(0.0, 8.0);
                    let freq = self
                        .real_input_source(id, "frequency")
                        .and_then(|src| values.get(&src).copied())
                        .unwrap_or(0.0);
                    let has_audio = self.input_source_node(id, "audio").is_some();
                    let level = if freq > 1.0 {
                        // Narrow-band energy proxy: fundamental + 2nd harmonic, phase-stable.
                        let w = std::f64::consts::TAU * freq * time_sec;
                        let band = (w.sin().abs() * 0.7 + (w * 2.0).sin().abs() * 0.3)
                            .powf(1.4);
                        // Slight detune shimmer so pure tones don't look flat.
                        let shimmer = ((time_sec * (freq * 0.07 + 3.0)).sin() * 0.5 + 0.5) * 0.15;
                        (band * (0.85 + shimmer)).clamp(0.0, 1.0)
                    } else if has_audio {
                        let env = (time_sec * 6.0).sin().abs();
                        let pulse = ((time_sec * 2.5).sin() * 0.5 + 0.5).powf(2.0);
                        (0.35 * env + 0.65 * pulse).clamp(0.0, 1.0)
                    } else {
                        // No audio / no freq: quiet baseline so graphs aren't stuck at full.
                        0.05
                    };
                    Ok((level * g_in).clamp(0.0, 1.0))
                }
                GraphNodeKind::SepticPlayer => {
                    // Truth time out = clamped request time from session (or raw time).
                    let t_req = self
                        .real_input_source(id, "time")
                        .and_then(|src| values.get(&src).copied())
                        .unwrap_or(time_sec);
                    let path = self.resolve_septic_path(id, "septic", 0);
                    let t_out = if let Some(p) = path {
                        if let Ok(session) = super::septic::SepticSession::load_path_cached_arc(
                            std::path::Path::new(&p),
                        ) {
                            session.truth_time(t_req)
                        } else {
                            t_req.max(0.0)
                        }
                    } else {
                        t_req.max(0.0)
                    };
                    port_buf.insert((id, "time_out".into()), t_out);
                    Ok(t_out)
                }
                GraphNodeKind::VideoPlayer => {
                    // Media metadata outs: total duration (s) + pixel size.
                    let video_inner = self.resolve_image_chain(id, "video", 0);
                    let path = match &video_inner.image {
                        GraphImageSource::FilePath(p) if !p.trim().is_empty() => p.as_str(),
                        _ => "",
                    };
                    let dur = if path.is_empty() {
                        0.0
                    } else {
                        crate::video_decode::probe_media_duration_secs(path)
                            .map(|d| d as f64)
                            .unwrap_or(0.0)
                    };
                    let (w, h) = if path.is_empty() {
                        (0.0, 0.0)
                    } else {
                        crate::video_decode::probe_media_size(path)
                            .map(|(a, b)| (a as f64, b as f64))
                            .unwrap_or((0.0, 0.0))
                    };
                    port_buf.insert((id, "total_duration".into()), dur);
                    port_buf.insert((id, "width".into()), w);
                    port_buf.insert((id, "height".into()), h);
                    Ok(dur)
                }
                GraphNodeKind::MouseEncoder {
                    time_threshold,
                    gain,
                } => {
                    let t_req = self
                        .real_input_source(id, "time")
                        .and_then(|src| values.get(&src).copied())
                        .unwrap_or(time_sec);
                    let thr = self
                        .real_input_source(id, "threshold")
                        .and_then(|src| values.get(&src).copied())
                        .unwrap_or(*time_threshold)
                        .clamp(0.001, 5.0);
                    let g = self
                        .real_input_source(id, "gain")
                        .and_then(|src| values.get(&src).copied())
                        .unwrap_or(*gain)
                        .clamp(0.0, 64.0);
                    // Mouse object path: from mouse pin (ObjectMouse or SepticPlayer chain).
                    let path = self
                        .input_source_node(id, "mouse")
                        .and_then(|src| {
                            let n = self.nodes.get(&src)?;
                            match &n.kind {
                                GraphNodeKind::ObjectMouse { path }
                                | GraphNodeKind::ObjectSeptic { path }
                                    if !path.trim().is_empty() =>
                                {
                                    Some(path.clone())
                                }
                                GraphNodeKind::SepticPlayer => {
                                    self.resolve_septic_path(src, "septic", 0)
                                }
                                _ => self.resolve_septic_path(src, "out", 0),
                            }
                        })
                        .or_else(|| self.resolve_septic_path(id, "mouse", 0));
                    let enc = if let Some(p) = path {
                        match super::septic::SepticSession::load_path_cached_arc(std::path::Path::new(&p)) {
                            Ok(session) => super::septic::encode_mouse(
                                &session,
                                t_req,
                                super::septic::MouseEncoderParams {
                                    time_threshold: thr,
                                    gain: g,
                                },
                            ),
                            Err(_) => super::septic::MouseEncoderOut {
                                x: 0.5,
                                y: 0.5,
                                shakiness: 0.0,
                                event: 0.0,
                                time: t_req.max(0.0),
                            },
                        }
                    } else {
                        super::septic::MouseEncoderOut {
                            x: 0.5,
                            y: 0.5,
                            shakiness: 0.0,
                            event: 0.0,
                            time: t_req.max(0.0),
                        }
                    };
                    port_buf.insert((id, "x".into()), enc.x);
                    port_buf.insert((id, "y".into()), enc.y);
                    port_buf.insert((id, "shakiness".into()), enc.shakiness);
                    port_buf.insert((id, "event".into()), enc.event);
                    // Primary = shakiness (useful default for single-wire demos).
                    Ok(enc.shakiness)
                }
                GraphNodeKind::ParamReal { param_id } => {
                    let v = self
                        .parameters
                        .iter()
                        .find(|p| p.id == *param_id)
                        .map(|p| p.v0)
                        .unwrap_or(0.0);
                    Ok(v)
                }
                GraphNodeKind::ExprX { expr }
                | GraphNodeKind::ExprXy { expr }
                | GraphNodeKind::ExprXyz { expr } => {
                    let x = self
                        .real_input_source(id, "x")
                        .and_then(|src| values.get(&src).copied())
                        .unwrap_or(0.0);
                    let y = self
                        .real_input_source(id, "y")
                        .and_then(|src| values.get(&src).copied())
                        .unwrap_or(0.0);
                    let z = self
                        .real_input_source(id, "z")
                        .and_then(|src| values.get(&src).copied())
                        .unwrap_or(0.0);
                    let t = (time_sec % 1.0_f64.max(1e-9)).clamp(0.0, 1.0);
                    let vars = super::expr::ExprVars {
                        t,
                        f: frame_f,
                        s: x,
                        x,
                        y,
                        z,
                        r: x,
                        g: y,
                        b: z,
                        a: 1.0,
                    };
                    super::expr::eval_expr_vars(expr, vars).map_err(|e| e.0)
                }
                _ => continue, // non-algebra real producers skipped in P1
            };

            match result {
                Ok(v) => {
                    values.insert(id, v);
                    if let Some(n) = self.nodes.get_mut(&id) {
                        if n.error.as_ref().is_some_and(|e| {
                            e.contains("Expr") || e.contains("expr") || e.contains("cycle")
                        }) {
                            n.error = None;
                        }
                    }
                }
                Err(msg) => {
                    if let Some(n) = self.nodes.get_mut(&id) {
                        n.error = Some(format!("Expr: {msg}"));
                    }
                }
            }
        }

        self.last_real_values = values;
        self.last_real_port_values = port_buf;
    }

    /// Kinds that can accept an input of `ty` (for wire-drop add menu).
    pub fn catalog_kinds_accepting(ty: PortType) -> Vec<GraphNodeKind> {
        use GraphNodeKind::*;
        let all = vec![
            Value { value: 0.0 },
            ExprX {
                expr: "x".into(),
            },
            ExprXy {
                expr: "x+y".into(),
            },
            ExprXyz {
                expr: "x+y+z".into(),
            },
            Frame,
            Time,
            Brightness,
            ColorChanger,
            LinearBlur,
            Zoom,
            Equalizer,
            Speed,
            Visualizer { gain: 1.0 },
            VideoPlayer,
            SepticPlayer,
            MouseEncoder {
                time_threshold: 0.12,
                gain: 8.0,
            },
            GeoSize,
            GeoPlacement,
            GeoRotate,
            GeoTrapezoid,
            GeoMirror,
            GeoAdd,
            OutputObject,
        ];
        all.into_iter()
            .filter(|k| {
                k.ports()
                    .iter()
                    .any(|p| p.dir == PortDir::Input && PortType::can_connect(ty, p.ty))
            })
            .collect()
    }

    /// Kinds that produce an output of `ty`.
    pub fn catalog_kinds_producing(ty: PortType) -> Vec<GraphNodeKind> {
        use GraphNodeKind::*;
        let all = vec![
            Value { value: 0.0 },
            ExprX {
                expr: "x".into(),
            },
            ExprXy {
                expr: "x+y".into(),
            },
            ExprXyz {
                expr: "x+y+z".into(),
            },
            Frame,
            Time,
            ObjectImage {
                path: String::new(),
            },
            ObjectVideo {
                path: String::new(),
            },
            ObjectAudio {
                path: String::new(),
            },
            ObjectFromApp {
                node_ids: Vec::new(),
            },
            ObjectSeptic {
                path: String::new(),
            },
            ObjectMouse {
                path: String::new(),
            },
            VideoPlayer,
            SepticPlayer,
            MouseEncoder {
                time_threshold: 0.12,
                gain: 8.0,
            },
            Brightness,
            ColorChanger,
            LinearBlur,
            Zoom,
            Equalizer,
            Speed,
            Visualizer { gain: 1.0 },
            GeoSize,
            GeoPlacement,
            GeoRotate,
            GeoTrapezoid,
            GeoMirror,
            GeoAdd,
        ];
        all.into_iter()
            .filter(|k| {
                k.ports()
                    .iter()
                    .any(|p| p.dir == PortDir::Output && p.ty == ty)
            })
            .collect()
    }

    /// Septic Player → video file path + media time from session.
    fn resolve_septic_player_video(&self, node_id: Uuid, _depth: usize) -> GraphOutputEval {
        let mut out = GraphOutputEval::default();
        let path = self.resolve_septic_path(node_id, "septic", 0);
        let t_req = self
            .real_input_source(node_id, "time")
            .and_then(|id| self.last_real_out(id))
            .unwrap_or(0.0);
        let Some(septic_path) = path else {
            return out;
        };
        let Ok(session) =
            super::septic::SepticSession::load_path_cached_arc(std::path::Path::new(&septic_path))
        else {
            return out;
        };
        let t = session.truth_time(t_req);
        out.video_time_sec = Some(t);
        if let Some(vp) =
            super::septic::resolve_video_path(std::path::Path::new(&septic_path), &session.meta)
        {
            out.image = GraphImageSource::FilePath(vp.to_string_lossy().into_owned());
        } else if !session.meta.video_path.is_empty() {
            out.image = GraphImageSource::FilePath(session.meta.video_path.clone());
        }
        out
    }

    /// Resolve VideoPlayer: video + time + start/duration → frame (Empty outside window).
    fn resolve_video_player(&self, node_id: Uuid, depth: usize) -> GraphOutputEval {
        let mut inner = self.resolve_image_chain(node_id, "video", depth);
        let path = match &inner.image {
            GraphImageSource::FilePath(p) if !p.trim().is_empty() => p.clone(),
            _ => return GraphOutputEval::default(),
        };
        let (t, _rate, silent) = self.video_player_time_window(node_id, &path, inner.speed);
        if silent {
            return GraphOutputEval {
                image: GraphImageSource::Empty,
                ..Default::default()
            };
        }
        inner.video_time_sec = Some(t);
        inner.effects_on_path = true;
        inner
    }

    /// Shared timing for VideoPlayer image + sound.
    /// Returns `(media_time_sec, playback_rate, silent)`.
    fn video_player_time_window(
        &self,
        node_id: Uuid,
        path: &str,
        speed: f64,
    ) -> (f64, f64, bool) {
        let time = self
            .real_input_source(node_id, "time")
            .and_then(|id| self.last_real_out(id))
            .unwrap_or(0.0);
        let start = self
            .real_input_source(node_id, "start")
            .and_then(|id| self.last_real_out(id))
            .unwrap_or(0.0)
            .max(0.0);
        let duration = self
            .real_input_source(node_id, "duration")
            .and_then(|id| self.last_real_out(id))
            .unwrap_or(0.0); // 0 = until media end
        let speed = speed.max(0.0);
        let t = start + (time * speed).max(0.0);

        // Probe can fail or return junk — never treat that as “always silent”.
        let media_end = crate::video_decode::probe_media_duration_secs(path)
            .map(|d| d as f64)
            .filter(|d| d.is_finite() && *d > 0.05)
            .unwrap_or(f64::INFINITY);
        let win_end = if duration > 1e-9 {
            (start + duration).min(media_end)
        } else {
            media_end
        };

        // Blank only when clearly past a finite window (or before start).
        let silent = t < start - 1e-9
            || (win_end.is_finite() && t >= win_end - 1e-9)
            || t < 0.0;

        let timeline = self
            .nodes
            .values()
            .find(|n| matches!(n.kind, GraphNodeKind::Time))
            .and_then(|n| self.last_real_out(n.id))
            .unwrap_or(time);
        let mut rate = if timeline.abs() > 1e-3 {
            (time / timeline) * speed
        } else if time.abs() > 1e-6 {
            speed.max(1e-3)
        } else {
            speed.max(1.0)
        };
        if !rate.is_finite() || rate <= 0.0 {
            rate = 1.0;
        }
        rate = rate.clamp(0.05, 16.0);
        (t, rate, silent)
    }

    /// Path + timing for VideoPlayer sound out.
    /// Prefer `audio` input path; else demux from video file.
    fn video_player_media_window(
        &self,
        node_id: Uuid,
        depth: usize,
    ) -> (String, f64, f64, bool) {
        let video_inner = self.resolve_image_chain(node_id, "video", depth);
        let video_path = match &video_inner.image {
            GraphImageSource::FilePath(p) if !p.trim().is_empty() => p.clone(),
            _ => String::new(),
        };
        // Optional separate soundtrack (ObjectVideo.sound / ObjectAudio).
        let audio_path = {
            let snd = self.resolve_sound_chain(node_id, "audio", depth);
            match snd.sound {
                GraphSoundSource::FilePath(p) if !p.trim().is_empty() => p,
                _ => String::new(),
            }
        };
        let path = if !audio_path.is_empty() {
            audio_path
        } else {
            video_path.clone()
        };
        if path.is_empty() {
            return (String::new(), 0.0, 1.0, true);
        }
        // Window is defined in video/media time; use video path for duration probe when possible.
        let probe = if !video_path.is_empty() {
            video_path.as_str()
        } else {
            path.as_str()
        };
        let (t, rate, silent) =
            self.video_player_time_window(node_id, probe, video_inner.speed);
        (path, t, rate, silent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_player_sound_with_audio_in_and_time() {
        let mut g = NodeGraph::new_empty();
        let out = g.output_node_id.expect("seeded Output");
        let vid = g.add_node(
            GraphNodeKind::ObjectVideo {
                path: "/tmp/fake.mp4".into(),
            },
            0.0,
            0.0,
        );
        let player = g.add_node(GraphNodeKind::VideoPlayer, 200.0, 0.0);
        let time = g.add_node(GraphNodeKind::Time, 0.0, 100.0);
        g.try_add_link(vid, "out", player, "video").unwrap();
        g.try_add_link(vid, "sound", player, "audio").unwrap();
        g.try_add_link(time, "out", player, "time").unwrap();
        g.try_add_link(player, "out", out, "image").unwrap();
        g.try_add_link(player, "sound", out, "sound").unwrap();
        g.eval_reals(60, 30.0); // t = 2.0s
        let snd = g.resolve_output_sound();
        assert_eq!(snd.path(), Some("/tmp/fake.mp4"));
        // Without a real probe, media_end is ∞ so not silent; media time ≈ 2s.
        assert!(snd.media_time_sec.is_some());
        let t = snd.media_time_sec.unwrap();
        assert!((t - 2.0).abs() < 0.01, "media_time={t}");
    }

    #[test]
    fn export_fast_blur_changes_pixels() {
        let mut img = image::RgbaImage::from_fn(64, 64, |x, y| {
            if x < 32 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            }
        });
        let before = img.clone();
        export_fast_blur_rgba(&mut img, 8.0);
        assert_ne!(img.as_raw(), before.as_raw());
        // Near-zero blur is a no-op.
        let mid = img.clone();
        export_fast_blur_rgba(&mut img, 0.0);
        assert_eq!(img.as_raw(), mid.as_raw());
    }

    #[test]
    fn bake_graph_output_fallback_missing_file() {
        let eval = GraphOutputEval::default();
        let img = bake_graph_output_rgba(
            "/nonexistent/path/that/does/not/exist.png",
            &eval,
            128,
            1.0,
            None,
            None,
        )
        .expect("fallback placeholder");
        assert!(img.width() <= 128 && img.height() <= 128);
        assert!(!img.as_raw().is_empty());
    }

    #[test]
    fn bake_graph_output_fx_cache_hit() {
        let path = "/nonexistent/bake_cache_test.png";
        let mut eval = GraphOutputEval::default();
        eval.blur_px = 4.0;
        let mut base = std::collections::HashMap::new();
        let mut fx = std::collections::HashMap::new();
        let a = bake_graph_output_rgba(path, &eval, 128, 1.0, Some(&mut base), Some(&mut fx))
            .unwrap();
        assert_eq!(base.len(), 1);
        assert_eq!(fx.len(), 1);
        let b = bake_graph_output_rgba(path, &eval, 128, 1.0, Some(&mut base), Some(&mut fx))
            .unwrap();
        assert_eq!(a.as_raw(), b.as_raw());
        // Same quantized blur step → same cache key (4.4 → 4 with step 1).
        eval.blur_px = 4.4;
        let c = bake_graph_output_rgba(path, &eval, 128, 1.0, Some(&mut base), Some(&mut fx))
            .unwrap();
        assert_eq!(fx.len(), 1);
        assert_eq!(a.as_raw(), c.as_raw());
        // Different step bucket → new FX entry.
        eval.blur_px = 6.0;
        let _d = bake_graph_output_rgba(path, &eval, 128, 1.0, Some(&mut base), Some(&mut fx))
            .unwrap();
        assert_eq!(fx.len(), 2);
    }

    #[test]
    fn eval_value_frame_time() {
        let mut g = NodeGraph {
            nodes: IndexMap::new(),
            links: Vec::new(),
            view: GraphView::default(),
            parameters: Vec::new(),
            output_node_id: None,
            root_error: None,
            last_real_values: Default::default(),
            last_real_port_values: Default::default(),
        };
        let v = g.add_node(GraphNodeKind::Value { value: 3.5 }, 0.0, 0.0);
        let f = g.add_node(GraphNodeKind::Frame, 0.0, 0.0);
        let t = g.add_node(GraphNodeKind::Time, 0.0, 0.0);
        g.eval_reals(60, 30.0);
        assert!((g.last_real_out(v).unwrap() - 3.5).abs() < 1e-9);
        assert!((g.last_real_out(f).unwrap() - 60.0).abs() < 1e-9);
        assert!((g.last_real_out(t).unwrap() - 2.0).abs() < 1e-9); // 60/30
    }

    #[test]
    fn eval_expr_uses_linked_x() {
        let mut g = NodeGraph {
            nodes: IndexMap::new(),
            links: Vec::new(),
            view: GraphView::default(),
            parameters: Vec::new(),
            output_node_id: None,
            root_error: None,
            last_real_values: Default::default(),
            last_real_port_values: Default::default(),
        };
        let v = g.add_node(GraphNodeKind::Value { value: 10.0 }, 0.0, 0.0);
        let e = g.add_node(
            GraphNodeKind::ExprX {
                expr: "x*2+1".into(),
            },
            100.0,
            0.0,
        );
        g.try_add_link(v, "out", e, "x").unwrap();
        g.eval_reals(0, 30.0);
        assert!((g.last_real_out(e).unwrap() - 21.0).abs() < 1e-9);
    }

    #[test]
    fn type_mismatch_rejected() {
        let mut g = NodeGraph::new_empty();
        let v = g.add_node(GraphNodeKind::Value { value: 1.0 }, 0.0, 0.0);
        let img = g.add_node(
            GraphNodeKind::ObjectImage {
                path: String::new(),
            },
            0.0,
            0.0,
        );
        // Value Real out → Image node has only Image out, no Real in — try brightness amount
        let b = g.add_node(GraphNodeKind::Brightness, 0.0, 0.0);
        // Real → RawImage rejected
        assert!(g.try_add_link(v, "out", b, "in").is_err());
        // Real → amount OK
        assert!(g.try_add_link(v, "out", b, "amount").is_ok());
        // Image → amount rejected
        assert!(g.try_add_link(img, "out", b, "amount").is_err());
    }

    #[test]
    fn cycle_marks_error() {
        let mut g = NodeGraph {
            nodes: IndexMap::new(),
            links: Vec::new(),
            view: GraphView::default(),
            parameters: Vec::new(),
            output_node_id: None,
            root_error: None,
            last_real_values: Default::default(),
            last_real_port_values: Default::default(),
        };
        let e1 = g.add_node(
            GraphNodeKind::ExprX {
                expr: "x".into(),
            },
            0.0,
            0.0,
        );
        let e2 = g.add_node(
            GraphNodeKind::ExprX {
                expr: "x".into(),
            },
            0.0,
            0.0,
        );
        g.try_add_link(e1, "out", e2, "x").unwrap();
        g.try_add_link(e2, "out", e1, "x").unwrap();
        g.eval_reals(0, 30.0);
        assert!(g.root_error.as_ref().is_some_and(|e| e.contains("cycle")));
    }

    #[test]
    fn catalog_accepts_real() {
        let kinds = NodeGraph::catalog_kinds_accepting(PortType::Real);
        assert!(kinds.iter().any(|k| matches!(k, GraphNodeKind::ExprX { .. } | GraphNodeKind::ExprXy { .. } | GraphNodeKind::ExprXyz { .. })));
        assert!(kinds
            .iter()
            .any(|k| matches!(k, GraphNodeKind::Brightness)));
        assert!(!kinds
            .iter()
            .any(|k| matches!(k, GraphNodeKind::ObjectImage { .. })));
    }

    #[test]
    fn param_real_eval() {
        let mut g = NodeGraph {
            nodes: IndexMap::new(),
            links: Vec::new(),
            view: GraphView::default(),
            parameters: vec![GraphParam::new_real("r", 7.25)],
            output_node_id: None,
            root_error: None,
            last_real_values: Default::default(),
            last_real_port_values: Default::default(),
        };
        let pid = g.parameters[0].id;
        let n = g.add_node(GraphNodeKind::ParamReal { param_id: pid }, 0.0, 0.0);
        g.eval_reals(0, 24.0);
        assert!((g.last_real_out(n).unwrap() - 7.25).abs() < 1e-9);
    }

    #[test]
    fn eval_chain_frame_into_expr() {
        let mut g = NodeGraph {
            nodes: IndexMap::new(),
            links: Vec::new(),
            view: GraphView::default(),
            parameters: Vec::new(),
            output_node_id: None,
            root_error: None,
            last_real_values: Default::default(),
            last_real_port_values: Default::default(),
        };
        let f = g.add_node(GraphNodeKind::Frame, 0.0, 0.0);
        let e = g.add_node(
            GraphNodeKind::ExprX {
                expr: "x/2".into(),
            },
            100.0,
            0.0,
        );
        g.try_add_link(f, "out", e, "x").unwrap();
        g.eval_reals(10, 30.0);
        assert!((g.last_real_out(e).unwrap() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn catalog_producing_real_includes_frame() {
        let kinds = NodeGraph::catalog_kinds_producing(PortType::Real);
        assert!(kinds.iter().any(|k| matches!(k, GraphNodeKind::Frame)));
        assert!(kinds.iter().any(|k| matches!(k, GraphNodeKind::Value { .. })));
        assert!(!kinds
            .iter()
            .any(|k| matches!(k, GraphNodeKind::ObjectImage { .. })));
    }

    #[test]
    fn expr_error_sets_node_error() {
        let mut g = NodeGraph {
            nodes: IndexMap::new(),
            links: Vec::new(),
            view: GraphView::default(),
            parameters: Vec::new(),
            output_node_id: None,
            root_error: None,
            last_real_values: Default::default(),
            last_real_port_values: Default::default(),
        };
        let e = g.add_node(
            GraphNodeKind::ExprX {
                expr: "@@@".into(),
            },
            0.0,
            0.0,
        );
        g.eval_reals(0, 30.0);
        assert!(g.nodes.get(&e).unwrap().error.is_some());
        assert!(g.last_real_out(e).is_none());
    }

    #[test]
    fn resolve_output_from_app_object_via_brightness() {
        let mut g = NodeGraph::new_empty();
        let out_id = g.output_node_id.expect("seeded output");
        let app_id = Uuid::new_v4();
        let src = g.add_node(
            GraphNodeKind::ObjectFromApp {
                node_ids: vec![app_id],
            },
            0.0,
            0.0,
        );
        let bright = g.add_node(GraphNodeKind::Brightness, 100.0, 0.0);
        let amount = g.add_node(GraphNodeKind::Value { value: 0.5 }, 50.0, 80.0);
        g.try_add_link(src, "out", bright, "in").unwrap();
        g.try_add_link(amount, "out", bright, "amount").unwrap();
        g.try_add_link(bright, "out", out_id, "image").unwrap();
        g.eval_reals(0, 30.0);
        let ev = g.resolve_output_image();
        assert!(matches!(
            ev.image,
            GraphImageSource::AppObjects(ref ids) if ids == &[app_id]
        ));
        assert!((ev.brightness - 0.5).abs() < 1e-9);
        assert!(ev.effects_on_path);
    }

    #[test]
    fn resolve_color_blur_speed_stack() {
        let mut g = NodeGraph::new_empty();
        let out_id = g.output_node_id.expect("seeded output");
        let img = g.add_node(
            GraphNodeKind::ObjectImage {
                path: "/tmp/x.png".into(),
            },
            0.0,
            0.0,
        );
        let blur = g.add_node(GraphNodeKind::LinearBlur, 40.0, 0.0);
        let color = g.add_node(GraphNodeKind::ColorChanger, 80.0, 0.0);
        let speed = g.add_node(GraphNodeKind::Speed, 120.0, 0.0);
        let hue = g.add_node(GraphNodeKind::Value { value: 90.0 }, 0.0, 40.0);
        let sat = g.add_node(GraphNodeKind::Value { value: 1.5 }, 0.0, 60.0);
        let rad = g.add_node(GraphNodeKind::Value { value: 3.0 }, 0.0, 80.0);
        let fac = g.add_node(GraphNodeKind::Value { value: 2.0 }, 0.0, 100.0);
        g.try_add_link(img, "out", blur, "in").unwrap();
        g.try_add_link(rad, "out", blur, "amount").unwrap();
        g.try_add_link(blur, "out", color, "in").unwrap();
        g.try_add_link(hue, "out", color, "hue").unwrap();
        g.try_add_link(sat, "out", color, "sat").unwrap();
        g.try_add_link(color, "out", speed, "in").unwrap();
        g.try_add_link(fac, "out", speed, "factor").unwrap();
        g.try_add_link(speed, "out", out_id, "image").unwrap();
        g.eval_reals(0, 30.0);
        let ev = g.resolve_output_image();
        assert_eq!(ev.image, GraphImageSource::FilePath("/tmp/x.png".into()));
        assert!((ev.blur_px - 3.0).abs() < 1e-9);
        assert!((ev.hue_shift - 90.0).abs() < 1e-9);
        assert!((ev.saturation - 1.5).abs() < 1e-9);
        assert!((ev.speed - 2.0).abs() < 1e-9);
        assert!(ev.needs_pixel_fx());
    }

    #[test]
    fn resolve_output_sound_via_equalizer() {
        let mut g = NodeGraph {
            nodes: IndexMap::new(),
            links: Vec::new(),
            view: GraphView::default(),
            parameters: Vec::new(),
            output_node_id: None,
            root_error: None,
            last_real_values: Default::default(),
            last_real_port_values: Default::default(),
        };
        let audio = g.add_node(
            GraphNodeKind::ObjectAudio {
                path: "/tmp/a.mp3".into(),
            },
            0.0,
            0.0,
        );
        let eq = g.add_node(GraphNodeKind::Equalizer, 100.0, 0.0);
        let bass = g.add_node(GraphNodeKind::Value { value: 3.0 }, 50.0, 80.0);
        let vol = g.add_node(GraphNodeKind::Value { value: 0.5 }, 50.0, 120.0);
        let out = g.add_node(GraphNodeKind::OutputObject, 200.0, 0.0);
        g.output_node_id = Some(out);
        g.try_add_link(audio, "out", eq, "in").unwrap();
        g.try_add_link(bass, "out", eq, "bass").unwrap();
        g.try_add_link(vol, "out", eq, "volume").unwrap();
        g.try_add_link(eq, "out", out, "sound").unwrap();
        g.eval_reals(0, 30.0);
        let s = g.resolve_output_sound();
        assert_eq!(s.sound, GraphSoundSource::FilePath("/tmp/a.mp3".into()));
        assert!((s.eq_bass - 3.0).abs() < 1e-9);
        assert!((s.volume - 0.5).abs() < 1e-9);
        assert_eq!(s.path(), Some("/tmp/a.mp3"));
    }

    #[test]
    fn apply_graph_image_fx_brightness_darkens() {
        let mut img = image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 200, 200, 255]));
        let mut eval = GraphOutputEval::default();
        eval.brightness = 0.5;
        apply_graph_image_fx(&mut img, &eval);
        let p = img.get_pixel(0, 0).0;
        assert!(p[0] < 120, "expected darker pixel, got {}", p[0]);
    }

    #[test]
    fn apply_graph_image_fx_blur_smooths() {
        let mut img = image::RgbaImage::new(5, 5);
        for y in 0..5 {
            for x in 0..5 {
                let v = if x == 2 && y == 2 { 255 } else { 0 };
                img.put_pixel(x, y, image::Rgba([v, v, v, 255]));
            }
        }
        let mut eval = GraphOutputEval::default();
        eval.blur_px = 2.0;
        apply_graph_image_fx(&mut img, &eval);
        let center = img.get_pixel(2, 2).0[0];
        let neighbor = img.get_pixel(2, 1).0[0];
        assert!(center < 255, "blur should reduce peak, got {center}");
        assert!(neighbor > 0, "blur should spread light to neighbors, got {neighbor}");
    }

    #[test]
    fn gaussian_kernel_normalized() {
        let k = gaussian_kernel(2.0);
        let sum: f32 = k.iter().sum();
        assert!((sum - 1.0).abs() < 1e-4, "kernel sum {sum}");
        assert!(k.len() >= 3);
        // Center weight is largest.
        let mid = k.len() / 2;
        assert!(k[mid] >= k[0]);
    }

    #[test]
    fn resolve_output_file_path() {
        let mut g = NodeGraph::new_empty();
        let out_id = g.output_node_id.expect("seeded output");
        let img = g.add_node(
            GraphNodeKind::ObjectImage {
                path: "/tmp/foo.png".into(),
            },
            0.0,
            0.0,
        );
        g.try_add_link(img, "out", out_id, "image").unwrap();
        let ev = g.resolve_output_image();
        assert_eq!(
            ev.image,
            GraphImageSource::FilePath("/tmp/foo.png".into())
        );
    }

    #[test]
    fn param_anim_track_labels() {
        let r = GraphParam::new_real("a", 1.0);
        assert_eq!(r.anim_track_labels().len(), 1);
        assert!(r.anim_track_labels()[0].starts_with("param:"));
        let c = GraphParam::new_color("c", 1.0, 0.0, 0.0);
        assert_eq!(c.anim_track_labels().len(), 4);
        let p = GraphParam::new_position("p", 0.0, 1.0);
        assert_eq!(p.anim_track_labels().len(), 2);
    }

    #[test]
    fn geometry_nodes_have_control_ports() {
        let size = GraphNodeKind::GeoSize.ports();
        assert!(size.iter().any(|p| p.id == "w" && p.ty == PortType::Real));
        assert!(size.iter().any(|p| p.id == "h" && p.ty == PortType::Real));
        assert!(size.iter().any(|p| p.id == "in" && p.ty == PortType::RawImage));
        assert!(size.iter().any(|p| p.id == "out" && p.ty == PortType::RawImage));

        let place = GraphNodeKind::GeoPlacement.ports();
        assert!(place.iter().any(|p| p.id == "x"));
        assert!(place.iter().any(|p| p.id == "y"));

        let rot = GraphNodeKind::GeoRotate.ports();
        assert!(rot.iter().any(|p| p.id == "angle" && p.ty == PortType::Real));

        let add = GraphNodeKind::GeoAdd.ports();
        assert!(add.iter().any(|p| p.id == "a"));
        assert!(add.iter().any(|p| p.id == "b"));
        assert!(!add.iter().any(|p| p.id == "in"));
    }

    #[test]
    fn sync_parameters_drops_orphans() {
        let mut g = NodeGraph::new_empty();
        let p = GraphParam::new_real("orphan", 1.0);
        let pid = p.id;
        g.parameters.push(p);
        // No ParamReal node → orphan removed.
        g.sync_parameters_with_nodes();
        assert!(g.parameters.is_empty());

        let p2 = GraphParam::new_real("live", 2.0);
        let pid2 = p2.id;
        g.parameters.push(p2);
        g.add_node(GraphNodeKind::ParamReal { param_id: pid2 }, 0.0, 0.0);
        g.sync_parameters_with_nodes();
        assert_eq!(g.parameters.len(), 1);
        assert_eq!(g.parameters[0].id, pid2);

        // Remove node → param goes away.
        let nid = *g.nodes.keys().find(|id| {
            matches!(g.nodes.get(*id).map(|n| &n.kind), Some(GraphNodeKind::ParamReal { .. }))
        }).unwrap();
        g.remove_node(nid);
        assert!(g.parameters.iter().all(|p| p.id != pid2));
        let _ = pid;
    }

    #[test]
    fn resolve_geo_placement_and_size() {
        let mut g = NodeGraph::new_empty();
        let out_id = g.output_node_id.expect("seeded");
        let img = g.add_node(
            GraphNodeKind::ObjectImage {
                path: "/tmp/g.png".into(),
            },
            0.0,
            0.0,
        );
        let size = g.add_node(GraphNodeKind::GeoSize, 40.0, 0.0);
        let place = g.add_node(GraphNodeKind::GeoPlacement, 80.0, 0.0);
        let sw = g.add_node(GraphNodeKind::Value { value: 2.0 }, 0.0, 40.0);
        let sh = g.add_node(GraphNodeKind::Value { value: 0.5 }, 0.0, 60.0);
        let px = g.add_node(GraphNodeKind::Value { value: 10.0 }, 0.0, 80.0);
        let py = g.add_node(GraphNodeKind::Value { value: 20.0 }, 0.0, 100.0);
        g.try_add_link(img, "out", size, "in").unwrap();
        g.try_add_link(sw, "out", size, "w").unwrap();
        g.try_add_link(sh, "out", size, "h").unwrap();
        g.try_add_link(size, "out", place, "in").unwrap();
        g.try_add_link(px, "out", place, "x").unwrap();
        g.try_add_link(py, "out", place, "y").unwrap();
        g.try_add_link(place, "out", out_id, "image").unwrap();
        g.eval_reals(0, 30.0);
        let ev = g.resolve_output_image();
        assert!((ev.geo_scale_w - 2.0).abs() < 1e-9);
        assert!((ev.geo_scale_h - 0.5).abs() < 1e-9);
        assert!((ev.geo_off_x - 10.0).abs() < 1e-9);
        assert!((ev.geo_off_y - 20.0).abs() < 1e-9);
    }
}
