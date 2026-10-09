//! VBLua image resources (milestone 1: analysis + transforms, CPU-always).
//!
//! ```text
//! Lua handle (opaque id) → VbImage { w, h, rgba } (runtime-owned)
//!   → analysis: luminance / average / color / histogram / sample / region
//!   → transforms: grayscale / brightness / contrast  (new handles)
//! ```
//!
//! Coordinates: normalized `0,0 = top-left`, `1,1 = bottom-right` (region API).
//! Luminance: Rec.709 on stored bytes, normalized 0..1 (black..white).
//! GPU execution (luminance map + reduction) lives in [`super::gpu`]; every
//! op below is the CPU fallback AND the correctness oracle. New handles mean
//! Lua can never alias a live frame buffer.

/// CPU image owned by the runtime (tight RGBA8, row-major).
#[derive(Debug, Clone)]
pub struct VbImage {
    pub width: u32,
    pub height: u32,
    /// `width*height*4` bytes.
    pub rgba: Vec<u8>,
}

impl VbImage {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, String> {
        if width == 0 || height == 0 || width > 16384 || height > 16384 {
            return Err("image dimensions out of range".to_string());
        }
        if rgba.len() != width as usize * height as usize * 4 {
            return Err("rgba length mismatch".to_string());
        }
        Ok(Self { width, height, rgba })
    }

    pub fn solid(width: u32, height: u32, color: [f32; 4]) -> Result<Self, String> {
        let px = [
            (color[0].clamp(0.0, 1.0) * 255.0) as u8,
            (color[1].clamp(0.0, 1.0) * 255.0) as u8,
            (color[2].clamp(0.0, 1.0) * 255.0) as u8,
            (color[3].clamp(0.0, 1.0) * 255.0) as u8,
        ];
        let n = width as usize * height as usize;
        if width == 0 || height == 0 || n > 16384 * 16384 {
            return Err("image dimensions out of range".to_string());
        }
        Ok(Self {
            width,
            height,
            rgba: [px[0], px[1], px[2], px[3]].repeat(n),
        })
    }

    /// Rec.709 luminance of one sRGB-stored pixel, 0..1.
    #[inline]
    pub fn pixel_luminance(px: [u8; 4]) -> f32 {
        let (r, g, b) = (px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0);
        super::gpu::LUMA_R * r + super::gpu::LUMA_G * g + super::gpu::LUMA_B * b
    }

    /// Mean luminance over the whole image (CPU oracle for the GPU path).
    pub fn average_brightness_cpu(&self) -> f32 {
        if self.rgba.is_empty() {
            return 0.0;
        }
        let mut sum = 0.0f64;
        for px in self.rgba.chunks_exact(4) {
            sum += Self::pixel_luminance([px[0], px[1], px[2], px[3]]) as f64;
        }
        (sum / (self.width as f64 * self.height as f64)) as f32
    }

    /// Mean linear-RGB color (alpha averaged separately).
    pub fn average_color_cpu(&self) -> [f32; 4] {
        if self.rgba.is_empty() {
            return [0.0; 4];
        }
        let mut acc = [0.0f64; 4];
        for px in self.rgba.chunks_exact(4) {
            for c in 0..4 {
                acc[c] += px[c] as f64 / 255.0;
            }
        }
        let n = self.width as f64 * self.height as f64;
        [acc[0] as f32 / n as f32, acc[1] as f32 / n as f32, acc[2] as f32 / n as f32, acc[3] as f32 / n as f32]
    }

    /// 256-bin luminance histogram (counts sum to pixel count).
    pub fn histogram_cpu(&self) -> Vec<u32> {
        let mut hist = vec![0u32; 256];
        for px in self.rgba.chunks_exact(4) {
            let bin = (Self::pixel_luminance([px[0], px[1], px[2], px[3]]) * 255.0).round() as usize;
            hist[bin.min(255)] += 1;
        }
        hist
    }

    /// Nearest pixel at normalized `(u, v)` (clamped to the frame).
    pub fn sample_cpu(&self, u: f32, v: f32) -> [f32; 4] {
        if self.rgba.is_empty() {
            return [0.0; 4];
        }
        let x = (u.clamp(0.0, 1.0) * self.width as f32) as u32;
        let y = (v.clamp(0.0, 1.0) * self.height as f32) as u32;
        let x = x.min(self.width - 1) as usize;
        let y = y.min(self.height - 1) as usize;
        let i = (y * self.width as usize + x) * 4;
        [
            self.rgba[i] as f32 / 255.0,
            self.rgba[i + 1] as f32 / 255.0,
            self.rgba[i + 2] as f32 / 255.0,
            self.rgba[i + 3] as f32 / 255.0,
        ]
    }

    /// Mean luminance over a normalized rect (clipped to the frame).
    pub fn region_average_cpu(&self, x: f32, y: f32, w: f32, h: f32) -> f32 {
        if self.rgba.is_empty() || w <= 0.0 || h <= 0.0 {
            return 0.0;
        }
        let x0 = (x.clamp(0.0, 1.0) * self.width as f32) as u32;
        let y0 = (y.clamp(0.0, 1.0) * self.height as f32) as u32;
        let x1 = ((x + w).clamp(0.0, 1.0) * self.width as f32) as u32;
        let y1 = ((y + h).clamp(0.0, 1.0) * self.height as f32) as u32;
        if x1 <= x0 || y1 <= y0 {
            return 0.0;
        }
        let mut sum = 0.0f64;
        let mut n = 0u64;
        for yy in y0..y1.min(self.height) {
            for xx in x0..x1.min(self.width) {
                let i = (yy as usize * self.width as usize + xx as usize) * 4;
                sum += Self::pixel_luminance([
                    self.rgba[i],
                    self.rgba[i + 1],
                    self.rgba[i + 2],
                    self.rgba[i + 3],
                ]) as f64;
                n += 1;
            }
        }
        if n == 0 {
            0.0
        } else {
            (sum / n as f64) as f32
        }
    }

    /// Grayscale copy (luminance broadcast, alpha kept).
    pub fn grayscale_cpu(&self) -> Self {
        let mut out = Vec::with_capacity(self.rgba.len());
        for px in self.rgba.chunks_exact(4) {
            let l = (Self::pixel_luminance([px[0], px[1], px[2], px[3]]) * 255.0).round() as u8;
            out.extend_from_slice(&[l, l, l, px[3]]);
        }
        Self { width: self.width, height: self.height, rgba: out }
    }

    /// Brightness scale copy (`value` multiplies RGB, alpha kept).
    pub fn brightness_cpu(&self, value: f32) -> Self {
        let mut out = Vec::with_capacity(self.rgba.len());
        for px in self.rgba.chunks_exact(4) {
            for c in 0..3 {
                out.push(((px[c] as f32 * value).round().clamp(0.0, 255.0)) as u8);
            }
            out.push(px[3]);
        }
        Self { width: self.width, height: self.height, rgba: out }
    }

    /// Contrast copy around middle gray (`v=1` identity).
    pub fn contrast_cpu(&self, value: f32) -> Self {
        let mut out = Vec::with_capacity(self.rgba.len());
        for px in self.rgba.chunks_exact(4) {
            for c in 0..3 {
                let v = ((px[c] as f32 - 128.0) * value + 128.0).round().clamp(0.0, 255.0) as u8;
                out.push(v);
            }
            out.push(px[3]);
        }
        Self { width: self.width, height: self.height, rgba: out }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn red() -> VbImage {
        VbImage::solid(4, 4, [1.0, 0.0, 0.0, 1.0]).unwrap()
    }

    #[test]
    fn luminance_values_match_rec709() {
        assert!((VbImage::pixel_luminance([255, 0, 0, 255]) - 0.2126).abs() < 1e-4);
        assert!((VbImage::pixel_luminance([0, 255, 0, 255]) - 0.7152).abs() < 1e-4);
        assert!((VbImage::pixel_luminance([0, 0, 255, 255]) - 0.0722).abs() < 1e-4);
        assert!((red().average_brightness_cpu() - 0.2126).abs() < 1e-3);
    }

    #[test]
    fn analysis_helpers() {
        let img = red();
        let c = img.average_color_cpu();
        assert!((c[0] - 1.0).abs() < 1e-6 && c[1].abs() < 1e-6);
        let hist = img.histogram_cpu();
        assert_eq!(hist.iter().sum::<u32>(), 16);
        assert_eq!(hist[(0.2126_f64 * 255.0).round() as usize], 16);
        let s = img.sample_cpu(0.0, 0.0);
        assert!((s[0] - 1.0).abs() < 1e-6);
        assert!((img.region_average_cpu(0.0, 0.0, 1.0, 1.0) - 0.2126).abs() < 1e-3);
        assert_eq!(img.region_average_cpu(0.0, 0.0, 0.0, 1.0), 0.0);
    }

    #[test]
    fn transforms() {
        let img = red();
        let g = img.grayscale_cpu();
        assert_eq!(&g.rgba[0..3], &[(0.2126_f64 * 255.0).round() as u8; 3]);
        assert_eq!(g.rgba[3], 255);
        let b = img.brightness_cpu(0.5);
        assert_eq!(b.rgba[0], 128);
        let c = img.contrast_cpu(1.0);
        assert_eq!(c.rgba, img.rgba);
    }
}
