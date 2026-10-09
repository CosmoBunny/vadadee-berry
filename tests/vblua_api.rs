//! VBLua integration suite (Phase 33): black-box flows through the public API.
//!
//! Unit tests live next to each module; these cover cross-subsystem runs:
//! document + graph + animation + batch + transaction in one script.

use vadadee_berry::document::{Fill, Layer, Node, NodeStore, ProjectFile};
use vadadee_berry::vblua::{SandboxPolicy, VbRuntime, VbluaError};

fn project_with_ne_layer() -> ProjectFile {
    let doc = vadadee_berry::document::Document {
        title: "integration".into(),
        width: 1920.0,
        height: 1080.0,
        layers: vec![Layer::new_node_editor_layer(
            uuid::Uuid::new_v4(),
            "NE".into(),
        )],
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
    project.document.layers[0].ensure_node_graph();
    let node = Node::rect(0.0, 0.0, 10.0, 10.0, Fill::None);
    project.nodes.insert(node);
    project
}

fn permissive() -> VbRuntime {
    let mut policy = SandboxPolicy::permissive_for_tests();
    policy.grant(vadadee_berry::vblua::Capability::FilesystemRead);
    VbRuntime::new(policy).unwrap()
}

#[test]
fn full_procedural_pipeline_in_one_script() {
    let mut rt = permissive();
    let mut project = project_with_ne_layer();
    let cnid = project.nodes.map.keys().next().unwrap().to_string();
    let out = rt
        .execute_with_project(
            "pipeline.lua",
            &format!(
                r#"
            assert(vblua.maturity() == "experimental")
            assert(vblua.api_version() == "1")
            -- procedural graph
            local pts = vblua.batch.grid({{ x = 0, y = 0 }}, 5, 80, 80, 10)
            local specs = {{}}
            for i, p in ipairs(pts) do specs[i] = {{ kind = "Value", x = p.x, y = p.y }} end
            local ids = vblua.batch.create_nodes(specs)
            -- template instance wired into the batch
            vblua.node.define("T", {{ nodes = {{ {{ kind = "Blur" }}, {{ kind = "Brightness" }} }},
              links = {{ {{ from = 1, from_port = "out", to = 2, to_port = "in" }} }} }})
            local t = vblua.node.spawn("T")
            -- animate the canvas node + a graph param
            vblua.batch.keyframes("{cnid}", "rotation", {{{{ 0, 0 }}, {{ 30, 360, "bezier" }}}})
            vblua.graph.set_param(ids[1], "value", 7)
            return #ids + #t
            "#
            ),
            Some(&mut project),
        )
        .unwrap();
    assert_eq!(out, "12");
    let g = project.document.layers[0].node_graph.as_ref().unwrap();
    assert_eq!(g.nodes.len(), 13); // seed + 10 + 2
    assert_eq!(g.links.len(), 1);
}

#[test]
fn transaction_rolls_back_across_subsystems() {
    let mut rt = permissive();
    let mut project = project_with_ne_layer();
    let before = project.document.layers[0]
        .node_graph
        .as_ref()
        .unwrap()
        .nodes
        .len();
    let err = rt
        .execute_with_project(
            "rollback.lua",
            r#"
            vblua.editor.transaction(function()
              vblua.graph.create("Blur")
              vblua.document.rename("Gone")
              error("boom")
            end)"#,
            Some(&mut project),
        )
        .unwrap_err();
    assert!(matches!(err, VbluaError::Runtime { .. }));
    let g = project.document.layers[0].node_graph.as_ref().unwrap();
    assert_eq!(g.nodes.len(), before);
    assert_eq!(project.document.title, "integration");
}

#[test]
fn default_policy_is_read_only_everywhere() {
    let mut rt = VbRuntime::new(SandboxPolicy::default()).unwrap();
    let mut project = project_with_ne_layer();
    for code in [
        r#"vblua.graph.create("Blur")"#,
        r#"vblua.document.rename("X")"#,
        r#"vblua.ui.panel("p", "P")"#,
        r#"vblua.node.define("T", { nodes = { { kind = "Blur" } } })"#,
    ] {
        let err = rt
            .execute_with_project("deny.lua", &format!("return {code}"), Some(&mut project))
            .unwrap_err();
        assert!(matches!(err, VbluaError::Runtime { .. }), "{code}: {err:?}");
    }
    // Reads still work.
    let out = rt
        .execute_with_project(
            "read.lua",
            "return vblua.document.current().name .. #vblua.assets.list()",
            Some(&mut project),
        )
        .unwrap();
    assert!(out.starts_with("integration"));
}

#[test]
fn kinematics_chain_drives_keyframes() {
    let mut rt = permissive();
    let mut project = project_with_ne_layer();
    let cnid = project.nodes.map.keys().next().unwrap().to_string();
    rt.execute_with_project(
        "arm.lua",
        &format!(
            r#"
        local arm = vblua.kinematics.chain({{ 100, 80 }})
        local a = arm.solve({{ x = 150, y = 40 }})
        -- elbow angle (degrees) becomes rotation keyframes
        local deg = a[2] * 180 / math.pi
        vblua.animation.set_keyframe("{cnid}", "rotation", 0, 0)
        vblua.animation.set_keyframe("{cnid}", "rotation", 30, deg)"#
        ),
        Some(&mut project),
    )
    .unwrap();
    let tl: Vec<_> = project
        .anim_timeline
        .nodes
        .values()
        .flat_map(|a| a.rotation.keyframes.clone())
        .collect();
    assert_eq!(tl.len(), 2);
}
