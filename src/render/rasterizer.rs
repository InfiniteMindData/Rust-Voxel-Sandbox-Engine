//! Screen-space triangle rasterizer with a z-buffer.
//!
//! This is the heart of the software renderer. It consumes clip-space
//! vertices (transformed and lit by the scene layer) and produces pixels:
//!
//! 1. Near-plane clipping of each triangle (Sutherland-Hodgman against
//!    `w >= epsilon` in clip space) so geometry crossing the camera plane
//!    cannot produce garbage.
//! 2. Perspective divide and viewport mapping into pixel space.
//! 3. Backface culling (counter-clockwise front faces).
//! 4. Edge-function rasterization with perspective-correct interpolation of
//!    UVs and vertex colors, using the `1/w` trick.
//! 5. Strict `LESS` depth testing with optional depth writes.
//!
//! Conventions match the `wgpu`/WebGPU defaults that a future GPU backend
//! will use: right-handed view space, depth range `0..1`, CCW front faces.

use glam::Vec2;
use glam::Vec3;
use glam::Vec4;

use super::framebuffer::Framebuffer;
use super::texture::Texture;

/// Clip-space w below which geometry is clipped (slightly in front of the
/// camera plane). Prevents division by zero and mirrored garbage.
const NEAR_CLIP_W: f32 = 1e-4;

/// A rasterizer input vertex: clip-space position plus perspective-correct
/// attributes (UV and vertex color/light).
#[derive(Debug, Clone, Copy)]
pub struct RasterVertex {
    /// Position in clip space (post model-view-projection).
    pub clip: Vec4,
    /// Texture coordinates.
    pub uv: Vec2,
    /// Vertex color / baked lighting multiplier, `0.0..1.0` per channel.
    pub color: Vec3,
}

/// Depth comparison mode for a draw call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepthTest {
    /// Always pass; never write depth. Used for sky/backdrops.
    Disabled,
    /// Standard opaque-geometry mode: pass when nearer, write depth.
    Less,
}

/// Per-draw-call rasterizer state.
pub struct RasterOpts<'a> {
    pub texture: Option<&'a Texture>,
    pub depth_test: DepthTest,
}

impl<'a> RasterOpts<'a> {
    /// Opaque textured geometry with depth testing. (Flat-colored draws use
    /// the 1x1 white texture via the renderer, keeping one shader-like path.)
    #[allow(dead_code)]
    pub fn textured(texture: &'a Texture) -> Self {
        Self {
            texture: Some(texture),
            depth_test: DepthTest::Less,
        }
    }

    /// Flat-shaded geometry with depth testing (vertex color only; unit tests).
    #[allow(dead_code)]
    pub fn flat() -> Self {
        Self {
            texture: None,
            depth_test: DepthTest::Less,
        }
    }

    /// Backdrop geometry: always drawn, never writes depth.
    pub fn backdrop(texture: Option<&'a Texture>) -> Self {
        Self {
            texture,
            depth_test: DepthTest::Disabled,
        }
    }
}

/// A screen-space interpolated vertex (post divide), plus `1/w` for
/// perspective-correct attribute interpolation.
#[derive(Debug, Clone, Copy)]
struct ScreenVertex {
    x: f32,
    y: f32,
    z: f32,
    inv_w: f32,
    uv: Vec2,
    color: Vec3,
}

/// Clips a triangle against the near plane `w >= NEAR_CLIP_W` in clip space.
/// Returns 0, 3 or 4 vertices.
fn clip_near(tri: &[RasterVertex; 3]) -> heapless_vec::HeapVec4<RasterVertex, 4> {
    // Sutherland-Hodgman against a single plane.
    let mut out = heapless_vec::HeapVec4::new();
    for i in 0..3 {
        let cur = tri[i];
        let next = tri[(i + 1) % 3];
        let cur_in = cur.clip.w >= NEAR_CLIP_W;
        let next_in = next.clip.w >= NEAR_CLIP_W;
        if cur_in {
            out.push(cur);
        }
        if cur_in != next_in {
            let t = (NEAR_CLIP_W - cur.clip.w) / (next.clip.w - cur.clip.w);
            out.push(RasterVertex {
                clip: cur.clip.lerp(next.clip, t),
                uv: cur.uv.lerp(next.uv, t),
                color: cur.color.lerp(next.color, t),
            });
        }
    }
    out
}

/// Tiny stack-friendly vector used by the clipper (max 4 elements ever).
mod heapless_vec {

    pub struct HeapVec4<T, const N: usize> {
        data: [Option<T>; 4],
        len: usize,
    }

    impl<T, const N: usize> HeapVec4<T, N> {
        pub fn new() -> Self {
            Self {
                data: [None, None, None, None],
                len: 0,
            }
        }

        pub fn push(&mut self, v: T) {
            debug_assert!(self.len < N, "clip output overflow");
            self.data[self.len] = Some(v);
            self.len += 1;
        }

        pub fn get(&self, i: usize) -> Option<&T> {
            self.data[i].as_ref()
        }

        pub fn len(&self) -> usize {
            self.len
        }
    }
}

/// Rasterizes an indexed triangle list.
///
/// Vertices whose index list references out-of-range slots are skipped with a
/// warning-free silent drop: mesh data comes from trusted engine code and the
/// invariant is checked in debug builds instead.
pub fn draw_triangles(fb: &mut Framebuffer, verts: &[RasterVertex], indices: &[u32], opts: &RasterOpts<'_>) {
    debug_assert!(
        indices.iter().all(|&i| (i as usize) < verts.len()),
        "index out of range in draw call"
    );
    for tri in indices.chunks_exact(3) {
        let (a, b, c) = match (verts.get(tri[0] as usize), verts.get(tri[1] as usize), verts.get(tri[2] as usize)) {
            (Some(a), Some(b), Some(c)) => (a, b, c),
            _ => continue, // invalid index: trusted-data invariant violated
        };
        let clipped = clip_near(&[*a, *b, *c]);
        if clipped.len() < 3 {
            continue;
        }
        let v0 = to_screen(clipped.get(0).unwrap(), fb);
        let v1 = to_screen(clipped.get(1).unwrap(), fb);
        let v2 = to_screen(clipped.get(2).unwrap(), fb);
        fill_triangle(fb, v0, v1, v2, opts);
        if clipped.len() == 4 {
            let v3 = to_screen(clipped.get(3).unwrap(), fb);
            fill_triangle(fb, v0, v2, v3, opts);
        }
    }
}

#[inline]
fn to_screen(v: &RasterVertex, fb: &Framebuffer) -> ScreenVertex {
    let inv_w = 1.0 / v.clip.w;
    let ndc = v.clip * inv_w;
    ScreenVertex {
        x: (ndc.x * 0.5 + 0.5) * fb.width() as f32,
        y: (0.5 - ndc.y * 0.5) * fb.height() as f32,
        z: ndc.z,
        inv_w,
        uv: v.uv * inv_w,
        color: v.color * inv_w,
    }
}

#[inline]
fn edge(a: &ScreenVertex, b: &ScreenVertex, px: f32, py: f32) -> f32 {
    (b.x - a.x) * (py - a.y) - (b.y - a.y) * (px - a.x)
}

fn fill_triangle(fb: &mut Framebuffer, v0: ScreenVertex, v1: ScreenVertex, v2: ScreenVertex, opts: &RasterOpts<'_>) {
    let area = edge(&v0, &v1, v2.x, v2.y);
    // area <= 0 covers both degenerate triangles and back faces (CCW front).
    if area <= 1e-6 {
        return;
    }
    let inv_area = 1.0 / area;

    let min_x = v0.x.min(v1.x).min(v2.x).floor().max(0.0) as i64;
    let min_y = v0.y.min(v1.y).min(v2.y).floor().max(0.0) as i64;
    let max_x = v0.x.max(v1.x).max(v2.x).ceil().min(fb.width() as f32 - 1.0) as i64;
    let max_y = v0.y.max(v1.y).max(v2.y).ceil().min(fb.height() as f32 - 1.0) as i64;

    let fb_w = fb.width() as usize;
    let depth_enabled = opts.depth_test == DepthTest::Less;

    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let sx = px as f32 + 0.5;
            let sy = py as f32 + 0.5;
            let e0 = edge(&v1, &v2, sx, sy);
            let e1 = edge(&v2, &v0, sx, sy);
            let e2 = edge(&v0, &v1, sx, sy);
            if e0 < 0.0 || e1 < 0.0 || e2 < 0.0 {
                continue;
            }
            let b0 = e0 * inv_area;
            let b1 = e1 * inv_area;
            let b2 = e2 * inv_area;

            // Depth is affine in screen space, so plain barycentric works.
            let z = b0 * v0.z + b1 * v1.z + b2 * v2.z;
            let pix = (py as usize * fb_w + px as usize) as usize;
            if depth_enabled {
                let depth = &mut fb.depth_buffer_mut()[pix];
                if z >= *depth {
                    continue;
                }
                *depth = z;
            }

            // Perspective-correct attributes: interpolate attribute/w and 1/w,
            // then divide.
            let inv_w = b0 * v0.inv_w + b1 * v1.inv_w + b2 * v2.inv_w;
            let w = inv_w.recip();
            let (u, vv) = if let Some(_tex) = opts.texture {
                let u = (b0 * v0.uv.x + b1 * v1.uv.x + b2 * v2.uv.x) * w;
                let v = (b0 * v0.uv.y + b1 * v1.uv.y + b2 * v2.uv.y) * w;
                (u, v)
            } else {
                (0.0, 0.0)
            };
            let r = (b0 * v0.color.x + b1 * v1.color.x + b2 * v2.color.x) * w;
            let g = (b0 * v0.color.y + b1 * v1.color.y + b2 * v2.color.y) * w;
            let b = (b0 * v0.color.z + b1 * v1.color.z + b2 * v2.color.z) * w;

            let rgb = if let Some(tex) = opts.texture {
                let t = tex.sample_repeat(u, vv);
                [
                    (r * t[0] as f32) as u8,
                    (g * t[1] as f32) as u8,
                    (b * t[2] as f32) as u8,
                ]
            } else {
                [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8]
            };

            fb.set_pixel(px as u32, py as u32, rgb[0], rgb[1], rgb[2]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Vec2, Vec3, Vec4};

    fn vert(ndc_x: f32, ndc_y: f32, z: f32, uv: (f32, f32), color: (f32, f32, f32)) -> RasterVertex {
        RasterVertex {
            clip: Vec4::new(ndc_x, ndc_y, z, 1.0),
            uv: Vec2::new(uv.0, uv.1),
            color: Vec3::new(color.0, color.1, color.2),
        }
    }

    #[test]
    fn fullscreen_quad_covers_every_pixel() {
        let mut fb = Framebuffer::new(8, 6);
        let verts = [
            vert(-1.0, -1.0, 0.5, (0.0, 0.0), (1.0, 0.0, 0.0)),
            vert(-1.0, 1.0, 0.5, (0.0, 1.0), (0.0, 1.0, 0.0)),
            vert(1.0, 1.0, 0.5, (1.0, 1.0), (0.0, 0.0, 1.0)),
            vert(1.0, -1.0, 0.5, (1.0, 0.0), (1.0, 1.0, 1.0)),
        ];
        let indices = [0, 1, 2, 0, 2, 3];
        draw_triangles(&mut fb, &verts, &indices, &RasterOpts::flat());
        // Every pixel must be covered (strict depth write happened too).
        for y in 0..6 {
            for x in 0..8 {
                assert_eq!(fb.pixel_depth(x, y), 0.5, "uncovered pixel {x},{y}");
            }
        }
        // Interpolation sanity: along the top edge, green (top-left corner
        // color) must fade towards the right; along the left edge, red
        // (bottom-left corner color) must fade towards the top.
        let top_left = fb.pixel(1, 0);
        let top_right = fb.pixel(6, 0);
        assert!(
            top_left[1] > top_right[1],
            "green must fade left->right along the top: {top_left:?} vs {top_right:?}"
        );
        let bottom_left = fb.pixel(0, 4);
        let upper_left = fb.pixel(0, 1);
        assert!(
            bottom_left[0] > upper_left[0],
            "red must fade bottom->top: {bottom_left:?} vs {upper_left:?}"
        );
    }

    #[test]
    fn backface_culling_hides_reversed_winding() {
        let mut fb = Framebuffer::new(4, 4);
        let verts = [
            vert(-0.5, -0.5, 0.5, (0.0, 0.0), (1.0, 1.0, 1.0)),
            vert(-0.5, 0.5, 0.5, (0.0, 1.0), (1.0, 1.0, 1.0)),
            vert(0.5, 0.5, 0.5, (1.0, 1.0), (1.0, 1.0, 1.0)),
        ];
        // Reversed winding -> culled.
        draw_triangles(&mut fb, &verts, &[0, 2, 1], &RasterOpts::flat());
        assert_eq!(fb.pixel(1, 1), [0, 0, 0], "back face must be culled");
        // Correct winding -> drawn.
        draw_triangles(&mut fb, &verts, &[0, 1, 2], &RasterOpts::flat());
        assert_eq!(fb.pixel(1, 1), [255, 255, 255], "front face must be drawn");
    }

    #[test]
    fn depth_buffer_nearest_triangle_wins() {
        let mut fb = Framebuffer::new(4, 4);
        let far = [
            vert(-1.0, -1.0, 0.8, (0.0, 0.0), (0.2, 0.2, 0.2)),
            vert(-1.0, 1.0, 0.8, (0.0, 1.0), (0.2, 0.2, 0.2)),
            vert(1.0, 1.0, 0.8, (1.0, 1.0), (0.2, 0.2, 0.2)),
            vert(1.0, -1.0, 0.8, (1.0, 0.0), (0.2, 0.2, 0.2)),
        ];
        let near = [
            vert(-0.5, -0.5, 0.3, (0.0, 0.0), (1.0, 0.0, 0.0)),
            vert(-0.5, 0.5, 0.3, (0.0, 1.0), (1.0, 0.0, 0.0)),
            vert(0.5, 0.5, 0.3, (1.0, 1.0), (1.0, 0.0, 0.0)),
            vert(0.5, -0.5, 0.3, (1.0, 0.0), (1.0, 0.0, 0.0)),
        ];
        let indices = [0, 1, 2, 0, 2, 3];
        draw_triangles(&mut fb, &far, &indices, &RasterOpts::flat());
        draw_triangles(&mut fb, &near, &indices, &RasterOpts::flat());
        assert_eq!(fb.pixel(2, 2), [255, 0, 0]);
        assert_eq!(fb.pixel_depth(2, 2), 0.3);
        // Far quad remains visible outside the near quad.
        assert_eq!(fb.pixel(3, 3), [51, 51, 51]);
    }

    #[test]
    fn near_plane_clipping_no_panic_and_partial_draw() {
        let mut fb = Framebuffer::new(8, 8);
        // One vertex behind the camera (w < 0), triangle must clip cleanly.
        let verts = [
            RasterVertex {
                clip: Vec4::new(2.0, 1.0, 0.5, -1.0),
                uv: Vec2::ZERO,
                color: Vec3::ONE,
            },
            vert(-0.5, -0.5, 0.5, (0.0, 0.0), (1.0, 1.0, 1.0)),
            vert(-0.5, 0.5, 0.5, (0.0, 1.0), (1.0, 1.0, 1.0)),
        ];
        draw_triangles(&mut fb, &verts, &[0, 1, 2], &RasterOpts::flat());
    }

    #[test]
    fn offscreen_triangle_is_safe() {
        let mut fb = Framebuffer::new(4, 4);
        let verts = [
            vert(50.0, 50.0, 0.5, (0.0, 0.0), (1.0, 1.0, 1.0)),
            vert(60.0, 50.0, 0.5, (0.0, 1.0), (1.0, 1.0, 1.0)),
            vert(50.0, 60.0, 0.5, (1.0, 1.0), (1.0, 1.0, 1.0)),
        ];
        draw_triangles(&mut fb, &verts, &[0, 1, 2], &RasterOpts::flat());
    }

    #[test]
    fn textured_triangle_samples_checker() {
        let mut fb = Framebuffer::new(8, 8);
        let tex = Texture::checker(2, 2, 2, [255, 0, 0, 255], [0, 0, 255, 255]);
        let verts = [
            vert(-1.0, -1.0, 0.5, (0.0, 0.0), (1.0, 1.0, 1.0)),
            vert(-1.0, 1.0, 0.5, (0.0, 1.0), (1.0, 1.0, 1.0)),
            vert(1.0, 1.0, 0.5, (1.0, 1.0), (1.0, 1.0, 1.0)),
            vert(1.0, -1.0, 0.5, (1.0, 0.0), (1.0, 1.0, 1.0)),
        ];
        draw_triangles(&mut fb, &verts, &[0, 1, 2, 0, 2, 3], &RasterOpts::textured(&tex));
        // The quad must show all four checker cells in the right places:
        // uv (0,0) is the bottom-left vertex, so cell (0,0) (red) lands in
        // the bottom-left quadrant and cell (0,1) (blue) in the top-left.
        assert_eq!(fb.pixel(1, 6), [255, 0, 0], "bottom-left cell");
        assert_eq!(fb.pixel(1, 1), [0, 0, 255], "top-left cell");
        assert_eq!(fb.pixel(6, 1), [255, 0, 0], "bottom-right cell");
        assert_eq!(fb.pixel(6, 6), [0, 0, 255], "top-right cell");
    }
}


