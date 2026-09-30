//! Small CPU-side textures with nearest-neighbor sampling.
//!
//! Textures are stored as 8-bit RGBA and sampled with nearest filtering to
//! preserve the crisp pixel-art look. A full texture atlas (milestone 5) will
//! live on top of these primitives.

/// An RGBA texture on the CPU.
#[derive(Clone)]
pub struct Texture {
    width: u32,
    height: u32,
    pixels: Vec<u8>, // rgba
}

impl Texture {
    /// Creates a texture from raw RGBA bytes. (Consumed by the texture atlas
    /// and world-generation textures, milestones 5-6.)
    #[allow(dead_code)]
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        assert_eq!(rgba.len(), (width * height * 4) as usize, "texture size mismatch");
        Self {
            width,
            height,
            pixels: rgba,
        }
    }

    /// A uniform solid-color texture.
    pub fn solid(width: u32, height: u32, color: [u8; 4]) -> Self {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..width * height {
            pixels.extend_from_slice(&color);
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    /// A black/white checkerboard, mainly for testing texture coordinate math.
    pub fn checker(width: u32, height: u32, cells: u32, a: [u8; 4], b: [u8; 4]) -> Self {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let cell = ((x * cells) / width) + ((y * cells) / height);
                pixels.extend_from_slice(&[if cell % 2 == 0 { a } else { b }[0], if cell % 2 == 0 { a } else { b }[1], if cell % 2 == 0 { a } else { b }[2], if cell % 2 == 0 { a } else { b }[3]]);
            }
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    #[allow(dead_code)] // consumed by the texture atlas (milestone 5)
    pub fn width(&self) -> u32 {
        self.width
    }

    #[allow(dead_code)]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Nearest-neighbor sample with coordinates in `0.0..1.0`, clamped at the
    /// edges. UVs outside the range clamp to the border pixel. (The atlas
    /// mesher in milestone 5 relies on clamped sampling at tile borders.)
    #[allow(dead_code)]
    pub fn sample_clamp(&self, u: f32, v: f32) -> [u8; 4] {
        let x = (u.clamp(0.0, 1.0) * (self.width as f32 - 0.001)) as u32;
        let y = (v.clamp(0.0, 1.0) * (self.height as f32 - 0.001)) as u32;
        self.texel(x, y)
    }

    /// Nearest-neighbor sample with wrapping repeat. Coordinates are taken
    /// modulo 1.0, so any finite value is valid.
    pub fn sample_repeat(&self, u: f32, v: f32) -> [u8; 4] {
        let u = u.rem_euclid(1.0);
        let v = v.rem_euclid(1.0);
        let x = (u * self.width as f32) as u32 % self.width;
        let y = (v * self.height as f32) as u32 % self.height;
        self.texel(x, y)
    }

    fn texel(&self, x: u32, y: u32) -> [u8; 4] {
        let idx = ((y * self.width + x) * 4) as usize;
        [
            self.pixels[idx],
            self.pixels[idx + 1],
            self.pixels[idx + 2],
            self.pixels[idx + 3],
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_texture_samples_everywhere() {
        let t = Texture::solid(4, 4, [1, 2, 3, 255]);
        assert_eq!(t.sample_clamp(-3.0, 99.0), [1, 2, 3, 255]);
        assert_eq!(t.sample_repeat(7.25, -0.5), [1, 2, 3, 255]);
    }

    #[test]
    fn checker_quadrants() {
        // 2x2 checker in a 2x2 texture.
        let t = Texture::checker(2, 2, 2, [255, 0, 0, 255], [0, 0, 255, 255]);
        assert_eq!(t.sample_clamp(0.1, 0.1), [255, 0, 0, 255]);
        assert_eq!(t.sample_clamp(0.9, 0.1), [0, 0, 255, 255]);
        assert_eq!(t.sample_clamp(0.9, 0.9), [255, 0, 0, 255]);
    }

    #[test]
    fn repeat_wraps() {
        let t = Texture::checker(2, 2, 2, [255, 0, 0, 255], [0, 0, 255, 255]);
        assert_eq!(t.sample_repeat(1.1, 0.1), t.sample_repeat(0.1, 0.1));
        assert_eq!(t.sample_repeat(-0.1, 0.0), t.sample_repeat(0.9, 0.0));
    }
}
