//! VBLua example verification (Phase 32): every shipped example runs
//! against the real API. No documentation-only scripts allowed.

use std::sync::Arc;

use vadadee_berry::document::{Fill, Layer, Node, NodeStore, ProjectFile};
use vadadee_berry::vblua::{SandboxPolicy, VbRuntime};

fn fixture() -> ProjectFile {
    let doc = vadadee_berry::document::Document {
        title: "examples".into(),
        width: 1920.0,
        height: 1080.0,
        layers: vec![
            Layer::new_image(uuid::Uuid::new_v4(), "Art".into(), true, false, vec![]),
            Layer::new_node_editor_layer(uuid::Uuid::new_v4(), "FX".into()),
            Layer::new_av_layer(uuid::Uuid::new_v4(), "Cut".into(), String::new()),
        ],
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
    };
    let mut project = ProjectFile::new(doc, NodeStore::default());
    project.document.layers[1].ensure_node_graph();
    let node = Node::rect(10.0, 10.0, 100.0, 60.0, Fill::None);
    project.document.layers[0].nodes.push(node.id);
    project.nodes.insert(node);
    project
}

fn harness() -> VbRuntime {
    let mut policy = SandboxPolicy::permissive_for_tests();
    policy.grant(vadadee_berry::vblua::Capability::FilesystemRead);
    let rt = VbRuntime::new(policy).unwrap();
    // Fake platform picker (2x2 PNG, no filesystem).
    let png = {
        use image::ImageEncoder;
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([9, 9, 9, 255]));
        let mut buf = Vec::new();
        let enc = image::codecs::png::PngEncoder::new(&mut buf);
        enc.write_image(img.as_raw(), 2, 2, image::ExtendedColorType::Rgba8)
            .unwrap();
        buf
    };
    rt.set_picker(Some(Arc::new(move |_| {
        Ok(vadadee_berry::vblua::PickedFile {
            name: "harness.png".into(),
            bytes: png.clone(),
        })
    })));
    rt
}

fn run_file(name: &str) {
    let path = format!("examples/vblua/{name}");
    let code =
        std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing example {path}"));
    let mut rt = harness();
    let mut project = fixture();
    rt.execute_with_project(name, &code, Some(&mut project))
        .unwrap_or_else(|e| panic!("example {name} failed: {}", e.render()));
}

#[test]
fn all_examples_run() {
    for name in [
        "hello.lua",
        "document.lua",
        "graph.lua",
        "params.lua",
        "animate.lua",
        "kinematics.lua",
        "shader.lua",
        "video.lua",
        "asset-import.lua",
        "picker.lua",
        "procedural.lua",
        "custom-tool.lua",
        "analyze.lua",
        "addon.lua",
    ] {
        run_file(name);
    }
}
