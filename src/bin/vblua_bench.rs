//! VBLua benchmarks (Phase 24): measure before optimizing.
//!
//! Run: `cargo run --no-default-features --bin vblua-bench`
//! Prints wall times for startup, script exec, bulk ops, solvers, templates.
//! No assertions on absolute speed (CI machines vary) — deltas matter.

use std::time::Instant;

use vadadee_berry::document::{Fill, Layer, Node, NodeStore, ProjectFile};
use vadadee_berry::vblua::{SandboxPolicy, VbRuntime};

fn ms<F: FnOnce()>(f: F) -> f64 {
    let t = Instant::now();
    f();
    t.elapsed().as_secs_f64() * 1000.0
}

fn test_project() -> ProjectFile {
    let doc = vadadee_berry::document::Document {
        title: "bench".into(),
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

fn main() {
    let startup = ms(|| {
        let _ = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
    });
    println!("runtime startup:        {startup:8.2} ms");

    let mut rt = VbRuntime::new(SandboxPolicy::permissive_for_tests()).unwrap();
    let hello = ms(|| {
        rt.execute("b.lua", r#"vblua.log("hi"); return vblua.version()"#)
            .unwrap();
    });
    println!("hello execute:          {hello:8.2} ms");

    let mut project = test_project();
    let batch100 = ms(|| {
        rt.execute_with_project(
            "b.lua",
            r#"
            local pts = vblua.batch.grid({ x = 0, y = 0 }, 10, 60, 60, 100)
            local specs = {}
            for i, p in ipairs(pts) do specs[i] = { kind = "Value", x = p.x, y = p.y } end
            return #vblua.batch.create_nodes(specs)"#,
            Some(&mut project),
        )
        .unwrap();
    });
    println!("100-node batch:         {batch100:8.2} ms");

    let cnid = project.nodes.map.keys().next().unwrap().to_string();
    let keys200 = ms(|| {
        let code = format!(
            "local pts = {{}} for f = 0, 200, 2 do pts[#pts+1] = {{ f, f * 1.5 }} end return vblua.batch.keyframes(\"{cnid}\", \"rotation\", pts)"
        );
        rt.execute_with_project("b.lua", &code, Some(&mut project))
            .unwrap();
    });
    println!("101-keyframe batch:     {keys200:8.2} ms");

    let ik = ms(|| {
        rt.execute("b.lua", "return vblua.kinematics.ik({100,80,60}, {x=150,y=40})")
            .unwrap();
    });
    println!("CCD IK solve:           {ik:8.2} ms");

    let tpl = ms(|| {
        rt.execute_with_project(
            "b.lua",
            r#"
            vblua.node.define("B", { nodes = { { kind = "Blur" }, { kind = "Brightness" } },
              links = { { from = 1, from_port = "out", to = 2, to_port = "in" } } })
            return #vblua.node.spawn("B")"#,
            Some(&mut project),
        )
        .unwrap();
    });
    println!("template define+spawn:  {tpl:8.2} ms");

    // GPU timings (skipped without a device; first call includes warmup).
    if vadadee_berry::vblua::gpu::gpu_available() {
        let gbright = ms(|| {
            rt.execute("b.lua", "local i = vblua.image.solid(512, 512, {0.5,0.5,0.5,1}) return vblua.image.average_brightness(i)")
                .unwrap();
        });
        println!("512px GPU brightness:   {gbright:8.2} ms (incl. warmup)");
        let gsteady = ms(|| {
            rt.execute("b.lua", "local i = vblua.image.solid(512, 512, {0.5,0.5,0.5,1}) return vblua.image.average_brightness(i)")
                .unwrap();
        });
        println!("512px GPU steady-state: {gsteady:8.2} ms");
        let kapply = ms(|| {
            rt.execute_with_project(
                "b.lua",
                r#"
                local k = vblua.kernel.create({ name = "id", language = "wgsl",
                  source = "@group(0) @binding(0) var t: texture_2d<f32>; @group(0) @binding(1) var o: texture_storage_2d<rgba8unorm, write>; @compute @workgroup_size(8,8) fn main(@builtin(global_invocation_id) g: vec3<u32>) { textureStore(o, g.xy, textureLoad(t, g.xy, 0)); }" })
                local i = vblua.image.solid(256, 256, {0.5,0.5,0.5,1})
                return vblua.image.apply(i, k, {})"#,
                Some(&mut project),
            )
            .unwrap();
        });
        println!("256px kernel apply:     {kapply:8.2} ms");
    } else {
        println!("GPU timings:            skipped (no device; CPU fallback active)");
    }

    let manifest = ms(|| {
        vadadee_berry::vblua::addons::parse_manifest(
            "bench",
            r#"return { id = "a.b", name = "N", version = "1", vblua = "1" }"#,
        )
        .unwrap();
    });
    println!("manifest parse:         {manifest:8.2} ms");
}
