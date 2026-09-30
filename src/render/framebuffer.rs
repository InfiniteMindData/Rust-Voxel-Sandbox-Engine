//! 2D color + depth framebuffer used by the software renderer.

/// A color plus depth render target.
///
/// Color is stored as 8-bit RGB, row-major, top-left origin - exactly the
/// layout the PNG encoder consumes. Depth is normalized device depth in
/// `0.0..1.0` (1.0 = far plane).
#[derive(Clone)]
pub struct Framebuffer {
    width: u32,
    height: u32,
    color: Vec<u8>,
    depth: Vec<f32>,
}

impl Framebuffer {
    /// Allocates a framebuffer filled with black at maximum depth.
    pub fn new(width: u32, height: u32) -> Self {
        assert!(width > 0 && height > 0, "framebuffer dimensions must be non-zero");
        let pixels = (width * height) as usize;
        Self {
            width,
            height,
            color: vec![0; pixels * 3],
            depth: vec![1.0; pixels],
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Clears color and depth.
    pub fn clear(&mut self, r: u8, g: u8, b: u8) {
        for px in self.color.chunks_exact_mut(3) {
            px[0] = r;
            px[1] = g;
            px[2] = b;
        }
        self.depth.fill(1.0);
    }

    /// Resizes the framebuffer if needed; contents become undefined.
    #[allow(dead_code)] // used via Renderer::resize
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == self.width && height == self.height {
            return;
        }
        assert!(width > 0 && height > 0, "framebuffer dimensions must be non-zero");
        let pixels = (width * height) as usize;
        self.width = width;
        self.height = height;
        self.color = vec![0; pixels * 3];
        self.depth = vec![1.0; pixels];
    }

    /// Raw RGB bytes, ready for the PNG encoder.
    pub fn color_bytes(&self) -> &[u8] {
        &self.color
    }

    /// The raw depth buffer (read by tests and debug tooling).
    #[allow(dead_code)]
    pub fn depth_buffer(&self) -> &[f32] {
        &self.depth
    }

    /// Mutable access to the raw depth buffer (rasterizer fast path).
    pub fn depth_buffer_mut(&mut self) -> &mut [f32] {
        &mut self.depth
    }

    /// Writes one pixel, bypassing depth. Mostly for tests and HUD use.
    pub fn set_pixel(&mut self, x: u32, y: u32, r: u8, g: u8, b: u8) {
        let idx = (y * self.width + x) as usize * 3;
        self.color[idx] = r;
        self.color[idx + 1] = g;
        self.color[idx + 2] = b;
    }

    /// Reads one pixel's RGB (tests and debug tooling).
    #[allow(dead_code)]
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        let idx = (y * self.width + x) as usize * 3;
        [self.color[idx], self.color[idx + 1], self.color[idx + 2]]
    }

    /// Reads one pixel's depth value (tests and debug tooling).
    #[allow(dead_code)]
    pub fn pixel_depth(&self, x: u32, y: u32) -> f32 {
        self.depth[(y * self.width + x) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_framebuffer_is_black_and_far() {
        let fb = Framebuffer::new(4, 3);
        assert_eq!(fb.color_bytes().len(), 4 * 3 * 3);
        assert!(fb.depth_buffer().iter().all(|&d| d == 1.0));
        assert_eq!(fb.pixel(2, 1), [0, 0, 0]);
    }

    #[test]
    fn clear_and_resize() {
        let mut fb = Framebuffer::new(2, 2);
        fb.clear(10, 20, 30);
        assert_eq!(fb.pixel(1, 1), [10, 20, 30]);
        fb.resize(3, 2);
        assert_eq!(fb.width(), 3);
        assert_eq!(fb.depth_buffer().len(), 6);
        assert_eq!(fb.pixel(2, 1), [0, 0, 0], "resize must reinitialize");
    }

    #[test]
    #[should_panic(expected = "non-zero")]
    fn zero_size_is_rejected() {
        let _ = Framebuffer::new(0, 8);
    }
}
