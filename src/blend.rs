//! CPU RGBA compositing for document blend modes (premultiplied alpha).

use crate::document::BlendMode;

#[inline]
fn unpremultiply(c: [f32; 4]) -> [f32; 4] {
    if c[3] <= 1e-6 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let inv = 1.0 / c[3];
    [c[0] * inv, c[1] * inv, c[2] * inv, c[3]]
}

#[inline]
fn premultiply(rgb: [f32; 3], a: f32) -> [f32; 4] {
    [rgb[0] * a, rgb[1] * a, rgb[2] * a, a]
}

fn blend_channel(base: f32, src: f32, mode: BlendMode) -> f32 {
    match mode {
        BlendMode::Normal => src,
        BlendMode::Multiply => base * src,
        BlendMode::Screen => 1.0 - (1.0 - base) * (1.0 - src),
        BlendMode::Overlay => {
            if base < 0.5 {
                2.0 * base * src
            } else {
                1.0 - 2.0 * (1.0 - base) * (1.0 - src)
            }
        }
        BlendMode::Darken => base.min(src),
        BlendMode::Lighten => base.max(src),
        BlendMode::ColorDodge => {
            if src >= 1.0 {
                1.0
            } else {
                (base / (1.0 - src)).min(1.0)
            }
        }
        BlendMode::ColorBurn => {
            if src <= 0.0 {
                0.0
            } else {
                (1.0 - (1.0 - base) / src).max(0.0)
            }
        }
        BlendMode::HardLight => {
            if src < 0.5 {
                2.0 * base * src
            } else {
                1.0 - 2.0 * (1.0 - base) * (1.0 - src)
            }
        }
        BlendMode::SoftLight => {
            if src < 0.5 {
                base - (1.0 - 2.0 * src) * base * (1.0 - base)
            } else {
                let d = if base < 0.25 {
                    ((16.0 * base - 12.0) * base + 4.0) * base
                } else {
                    base.sqrt()
                };
                base + (2.0 * src - 1.0) * (d - base)
            }
        }
        BlendMode::Difference => (base - src).abs(),
        BlendMode::Exclusion => base + src - 2.0 * base * src,
        BlendMode::Addition => (base + src).min(1.0),
        BlendMode::Subtract => (base - src).max(0.0),
        BlendMode::Hue | BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity => {
            1.0 - (1.0 - base) * (1.0 - src)
        }
    }
}

fn composite_pixel(dst: [f32; 4], src: [f32; 4], mode: BlendMode) -> [f32; 4] {
    let src_a = src[3];
    if src_a <= 1e-6 {
        return dst;
    }
    let dst_rgb = unpremultiply(dst);
    let src_rgb = unpremultiply(src);
    let out_rgb = [
        blend_channel(dst_rgb[0], src_rgb[0], mode),
        blend_channel(dst_rgb[1], src_rgb[1], mode),
        blend_channel(dst_rgb[2], src_rgb[2], mode),
    ];
    let out_premul = premultiply(out_rgb, src_a);
    let inv = 1.0 - src_a;
    [
        out_premul[0] + dst[0] * inv,
        out_premul[1] + dst[1] * inv,
        out_premul[2] + dst[2] * inv,
        (src_a + dst[3] * inv).min(1.0),
    ]
}

/// Stamp `src` RGBA (straight alpha in bytes) onto `dst` at pixel offset `(ox, oy)`.
pub fn composite_stamp(
    dst: &mut [u8],
    dst_w: u32,
    dst_h: u32,
    src: &[u8],
    src_w: u32,
    src_h: u32,
    ox: i32,
    oy: i32,
    mode: BlendMode,
    opacity: f32,
) {
    let op = opacity.clamp(0.0, 1.0);
    for sy in 0..src_h as i32 {
        let dy = oy + sy;
        if dy < 0 || dy >= dst_h as i32 {
            continue;
        }
        for sx in 0..src_w as i32 {
            let dx = ox + sx;
            if dx < 0 || dx >= dst_w as i32 {
                continue;
            }
            let si = ((sy as u32 * src_w + sx as u32) * 4) as usize;
            if si + 3 >= src.len() {
                continue;
            }
            let a = (src[si + 3] as f32 / 255.0) * op;
            if a <= 1e-6 {
                continue;
            }
            let src_px = premultiply(
                [
                    src[si] as f32 / 255.0,
                    src[si + 1] as f32 / 255.0,
                    src[si + 2] as f32 / 255.0,
                ],
                a,
            );
            let di = ((dy as u32 * dst_w + dx as u32) * 4) as usize;
            if di + 3 >= dst.len() {
                continue;
            }
            let dst_px = [
                dst[di] as f32 / 255.0,
                dst[di + 1] as f32 / 255.0,
                dst[di + 2] as f32 / 255.0,
                dst[di + 3] as f32 / 255.0,
            ];
            let out = if mode == BlendMode::Normal {
                let inv = 1.0 - src_px[3];
                [
                    src_px[0] + dst_px[0] * inv,
                    src_px[1] + dst_px[1] * inv,
                    src_px[2] + dst_px[2] * inv,
                    (src_px[3] + dst_px[3] * inv).min(1.0),
                ]
            } else {
                composite_pixel(dst_px, src_px, mode)
            };
            dst[di] = (out[0].clamp(0.0, 1.0) * 255.0).round() as u8;
            dst[di + 1] = (out[1].clamp(0.0, 1.0) * 255.0).round() as u8;
            dst[di + 2] = (out[2].clamp(0.0, 1.0) * 255.0).round() as u8;
            dst[di + 3] = (out[3].clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}

pub fn document_needs_blend_composite(
    nodes: &crate::document::NodeStore,
    order: &[crate::document::NodeId],
    hidden: &std::collections::HashSet<crate::document::NodeId>,
) -> bool {
    order.iter().any(|id| {
        if hidden.contains(id) {
            return false;
        }
        nodes
            .get(*id)
            .is_some_and(|n| n.style.blend_mode != BlendMode::Normal)
    })
}