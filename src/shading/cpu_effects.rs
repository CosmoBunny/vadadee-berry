//! Shading overlays: GPU (runtime WGSL) with CPU fallback.

use egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, Stroke};

use crate::document::ShadingPass;
use crate::shading::procedural_blackhole::{BlackholeParams, sample};

pub fn draw_shading_passes(
    painter: &Painter,
    page_rect: Rect,
    passes: &[ShadingPass],
    time_secs: f32,
    gpu: Option<&egui_wgpu::RenderState>,
) {
    if let Some(rs) = gpu {
        if crate::shading::wgpu_pass::try_draw_shading_passes_gpu(
            painter, page_rect, passes, time_secs, rs,
        ) {
            return;
        }
    }
    draw_shading_passes_cpu(painter, page_rect, passes, time_secs);
}

fn draw_shading_passes_cpu(
    painter: &Painter,
    page_rect: Rect,
    passes: &[ShadingPass],
    time_secs: f32,
) {
    if let Some(pass) = passes.first().filter(|p| p.enabled) {
        let name = pass.name.to_ascii_lowercase();
        let wgsl = pass.compiled_wgsl.as_ref().unwrap_or(&pass.wgsl);
        let is_blackhole = name.contains("blackhole") || wgsl.contains("blackhole") || wgsl.contains("black hole");
        let is_starfield = name == "starfield" || wgsl.contains("// Starfield — rendered via CPU starfield path.");
        
        if is_blackhole {
            draw_blackhole_shader(painter, page_rect, time_secs, pass);
        } else if is_starfield {
            draw_starfield_shader(painter, page_rect, time_secs, pass);
        } else if name.contains("crt") || wgsl.contains("scan") {
            draw_crt(painter, page_rect);
        } else if name.contains("vignette") {
            draw_vignette(painter, page_rect, 0.65);
        }
    }
}

fn draw_starfield_shader(painter: &Painter, page: Rect, time_secs: f32, pass: &ShadingPass) {
    let t = if pass.uniforms.len() >= 1 {
        pass.uniforms[0] + time_secs
    } else {
        time_secs
    };
    let aspect = (page.width() / page.height().max(1.0)).max(0.25);

    let cols: usize = 160;
    let rows: usize = ((cols as f32 * page.height() / page.width()).ceil() as usize).clamp(90, 200);
    let cw = page.width() / cols as f32;
    let ch = page.height() / rows as f32;

    let mut mesh = Mesh::default();
    for row in 0..rows {
        for col in 0..cols {
            let x0 = page.left() + col as f32 * cw;
            let y0 = page.top() + row as f32 * ch;
            let x1 = x0 + cw;
            let y1 = y0 + ch;
            let u0 = col as f32 / cols as f32;
            let v0 = row as f32 / rows as f32;
            let u1 = (col + 1) as f32 / cols as f32;
            let v1 = (row + 1) as f32 / rows as f32;
            let rgb = crate::shading::procedural_blackhole::sample_starfield(
                ((u0 + u1) * 0.5, (v0 + v1) * 0.5),
                t,
                aspect,
            );
            let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
            append_quad(&mut mesh, x0, y0, x1, y1, color);
        }
    }
    painter.add(Shape::mesh(mesh));
}


fn draw_blackhole_shader(painter: &Painter, page: Rect, time_secs: f32, pass: &ShadingPass) {
    let mut u = BlackholeParams::default();
    if pass.uniforms.len() >= 3 {
        u.time = pass.uniforms[0] + time_secs;
        u.strength = pass.uniforms[1];
        u.disk_radius = pass.uniforms[2];
    } else {
        u.time = time_secs;
    }
    u.aspect = (page.width() / page.height().max(1.0)).max(0.25);

    let cols: usize = 160;
    let rows: usize = ((cols as f32 * page.height() / page.width()).ceil() as usize).clamp(90, 200);
    let cw = page.width() / cols as f32;
    let ch = page.height() / rows as f32;

    let mut mesh = Mesh::default();
    for row in 0..rows {
        for col in 0..cols {
            let x0 = page.left() + col as f32 * cw;
            let y0 = page.top() + row as f32 * ch;
            let x1 = x0 + cw;
            let y1 = y0 + ch;
            let u0 = col as f32 / cols as f32;
            let v0 = row as f32 / rows as f32;
            let u1 = (col + 1) as f32 / cols as f32;
            let v1 = (row + 1) as f32 / rows as f32;
            let rgb = sample(
                ((u0 + u1) * 0.5, (v0 + v1) * 0.5),
                &u,
            );
            let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
            append_quad(&mut mesh, x0, y0, x1, y1, color);
        }
    }
    painter.add(Shape::mesh(mesh));
}

fn append_quad(mesh: &mut Mesh, x0: f32, y0: f32, x1: f32, y1: f32, color: Color32) {
    let base = mesh.vertices.len() as u32;
    let uv = egui::epaint::WHITE_UV;
    mesh.vertices.push(egui::epaint::Vertex {
        pos: Pos2::new(x0, y0),
        uv,
        color,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: Pos2::new(x1, y0),
        uv,
        color,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: Pos2::new(x1, y1),
        uv,
        color,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: Pos2::new(x0, y1),
        uv,
        color,
    });
    mesh.add_triangle(base, base + 1, base + 2);
    mesh.add_triangle(base, base + 2, base + 3);
}

fn draw_vignette(painter: &Painter, page: Rect, strength: f32) {
    let center = page.center();
    let radius = page.width().max(page.height()) * 0.55;
    let steps = 24;
    for i in 0..steps {
        let t0 = i as f32 / steps as f32;
        let t1 = (i + 1) as f32 / steps as f32;
        let alpha = ((1.0 - t0) * strength * 0.55).clamp(0.0, 0.85);
        let r0 = radius * t0;
        let r1 = radius * t1;
        let mut mesh = Mesh::default();
        append_ring(&mut mesh, center, r0, r1, Color32::from_black_alpha((alpha * 255.0) as u8));
        painter.add(Shape::mesh(mesh));
    }
}

fn draw_crt(painter: &Painter, page: Rect) {
    let mut y = page.top();
    while y < page.bottom() {
        painter.line_segment(
            [
                Pos2::new(page.left(), y),
                Pos2::new(page.right(), y),
            ],
            Stroke::new(1.0, Color32::from_black_alpha(28)),
        );
        y += 3.0;
    }
    draw_vignette(painter, page, 0.35);
}

fn append_ring(mesh: &mut Mesh, center: Pos2, r0: f32, r1: f32, color: Color32) {
    let segs = 48;
    let base = mesh.vertices.len() as u32;
    for i in 0..=segs {
        let a = (i as f32 / segs as f32) * std::f32::consts::TAU;
        let (c, s) = (a.cos(), a.sin());
        mesh.vertices.push(egui::epaint::Vertex {
            pos: Pos2::new(center.x + c * r0, center.y + s * r0),
            uv: egui::epaint::WHITE_UV,
            color,
        });
        mesh.vertices.push(egui::epaint::Vertex {
            pos: Pos2::new(center.x + c * r1, center.y + s * r1),
            uv: egui::epaint::WHITE_UV,
            color,
        });
    }
    for i in 0..segs {
        let i0 = base + i * 2;
        mesh.add_triangle(i0, i0 + 1, i0 + 2);
        mesh.add_triangle(i0 + 1, i0 + 3, i0 + 2);
    }
}