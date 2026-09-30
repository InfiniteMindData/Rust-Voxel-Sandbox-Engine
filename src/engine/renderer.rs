//! Renderer abstraction plus the software rasterizer implementation.
//!
//! [`Renderer`] is the seam a future GPU backend (`wgpu`) will implement:
//! meshes are uploaded once and drawn with a model matrix per frame, exactly
//! the data flow a GPU pipeline needs. The software backend simply keeps them
//! on the CPU and rasterizes on the spot.

use glam::{Mat3, Mat4, Vec2, Vec3, Vec4};

use crate::render::framebuffer::Framebuffer;
use crate::render::mesh::Mesh;
use crate::render::rasterizer::{draw_triangles, RasterOpts, RasterVertex};
use crate::render::texture::Texture;

/// Opaque handle to an uploaded mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshHandle(u32);

/// Per-frame global rendering parameters.
#[derive(Clone)]
pub struct FrameParams {
    /// Combined camera matrix.
    pub view_proj: Mat4,
    /// Direction *towards* the sun in world space (normalized).
    pub sun_dir: Vec3,
    /// Sky gradient color at the top of the screen (sRGB 0..1).
    pub sky_top: Vec3,
    /// Sky gradient color at the bottom of the screen.
    pub sky_bottom: Vec3,
}

impl Default for FrameParams {
    fn default() -> Self {
        Self {
            view_proj: Mat4::IDENTITY,
            sun_dir: Vec3::new(0.5, 1.0, 0.3).normalize(),
            sky_top: Vec3::new(0.32, 0.53, 0.83),
            sky_bottom: Vec3::new(0.72, 0.83, 0.94),
        }
    }
}

/// Per-frame counters for the debug HUD.
#[derive(Debug, Default, Clone, Copy)]
pub struct RenderStats {
    pub triangles: u64,
    pub draw_calls: u64,
}

/// Backend-agnostic renderer interface.
pub trait Renderer {
    /// Uploads a mesh (and its texture, if any) for repeated drawing.
    fn alloc_mesh(&mut self, mesh: &Mesh, texture: Option<Texture>) -> MeshHandle;
    /// Releases an uploaded mesh. (Consumed by chunk unloading, milestone 10.)
    #[allow(dead_code)]
    fn free_mesh(&mut self, handle: MeshHandle);
    /// Resizes the render target. (Wired to dynamic resolution changes later.)
    #[allow(dead_code)]
    fn resize(&mut self, width: u32, height: u32);
    /// Starts a frame: clears and draws the sky backdrop.
    fn begin_frame(&mut self, params: &FrameParams);
    /// Draws one mesh instance with the given model matrix.
    fn draw_mesh(&mut self, handle: MeshHandle, model: Mat4);
    /// Finishes the frame and returns the rendered target.
    fn end_frame(&mut self) -> &Framebuffer;
    /// Access to the render target (for stats, screenshots). (Read outside
    /// `end_frame` from later systems, e.g. entity hitbox debug views.)
    #[allow(dead_code)]
    fn framebuffer(&self) -> &Framebuffer;
    /// Counters from the most recent frame.
    fn stats(&self) -> RenderStats;
}

/// Software (CPU) renderer implementation.
pub struct SoftwareRenderer {
    fb: Framebuffer,
    meshes: Vec<Option<(Mesh, Option<Texture>)>>,
    free_slots: Vec<usize>,
    white: Texture,
    params: FrameParams,
    stats: RenderStats,
}

// Ambient term keeps faces facing away from the sun readable.
const AMBIENT: f32 = 0.32;
// Diffuse strength at full sun exposure.
const DIFFUSE: f32 = 0.68;

impl SoftwareRenderer {
    /// Creates a software renderer with the given target size.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            fb: Framebuffer::new(width, height),
            meshes: Vec::new(),
            free_slots: Vec::new(),
            white: Texture::solid(1, 1, [255, 255, 255, 255]),
            params: FrameParams::default(),
            stats: RenderStats::default(),
        }
    }

    /// Transforms and shades one mesh instance into rasterizer vertices.
    fn build_raster_vertices(&self, mesh: &Mesh, texture: Option<&Texture>, model: Mat4) -> Vec<RasterVertex> {
        let normal_mat = Mat4::from_mat3(Mat3::from_mat4(model));
        mesh.vertices
            .iter()
            .map(|v| {
                let world = model.transform_point3(v.position);
                let world_normal = normal_mat.transform_vector3(v.normal).normalize_or_zero();
                let lambert = world_normal.dot(self.params.sun_dir).max(0.0);
                let light = (AMBIENT + DIFFUSE * lambert).min(1.0);
                let texel = texture
                    .map(|t| t.sample_repeat(v.uv.x, v.uv.y))
                    .unwrap_or([255, 255, 255, 255]);
                let color = Vec3::new(
                    (texel[0] as f32 / 255.0) * light,
                    (texel[1] as f32 / 255.0) * light,
                    (texel[2] as f32 / 255.0) * light,
                );
                RasterVertex {
                    clip: self.params.view_proj * world.extend(1.0),
                    uv: v.uv,
                    color,
                }
            })
            .collect()
    }

    /// Draws the sky as a fullscreen gradient: two flat triangles that are
    /// drawn first and never write depth, so everything draws over them.
    fn draw_sky(&mut self) {
        let top = self.params.sky_top;
        let bottom = self.params.sky_bottom;
        let v = [
            RasterVertex {
                clip: Vec4::new(-1.0, -1.0, 1.0, 1.0),
                uv: Vec2::ZERO,
                color: bottom,
            },
            RasterVertex {
                clip: Vec4::new(-1.0, 1.0, 1.0, 1.0),
                uv: Vec2::ZERO,
                color: top,
            },
            RasterVertex {
                clip: Vec4::new(1.0, 1.0, 1.0, 1.0),
                uv: Vec2::ZERO,
                color: top,
            },
            RasterVertex {
                clip: Vec4::new(1.0, -1.0, 1.0, 1.0),
                uv: Vec2::ZERO,
                color: bottom,
            },
        ];
        let indices = [0u32, 1, 2, 0, 2, 3];
        self.fb.clear(0, 0, 0);
        draw_triangles(&mut self.fb, &v, &indices, &RasterOpts::backdrop(None));
        self.stats.draw_calls += 1;
        self.stats.triangles += 2;
    }
}

impl Renderer for SoftwareRenderer {
    fn alloc_mesh(&mut self, mesh: &Mesh, texture: Option<Texture>) -> MeshHandle {
        let slot = if let Some(slot) = self.free_slots.pop() {
            self.meshes[slot] = Some((mesh.clone(), texture));
            slot
        } else {
            self.meshes.push(Some((mesh.clone(), texture)));
            self.meshes.len() - 1
        };
        MeshHandle(slot as u32)
    }

    #[allow(dead_code)]
    fn free_mesh(&mut self, handle: MeshHandle) {
        let slot = handle.0 as usize;
        if slot < self.meshes.len() {
            self.meshes[slot] = None;
            self.free_slots.push(slot);
        }
    }

    #[allow(dead_code)]
    fn resize(&mut self, width: u32, height: u32) {
        self.fb.resize(width, height);
    }

    fn begin_frame(&mut self, params: &FrameParams) {
        self.params = params.clone();
        self.stats = RenderStats::default();
        self.draw_sky();
    }

    fn draw_mesh(&mut self, handle: MeshHandle, model: Mat4) {
        let Some((mesh, texture)) = self.meshes.get(handle.0 as usize).and_then(|m| m.as_ref()) else {
            return; // freed or invalid handle: nothing to draw
        };
        let raster_verts = self.build_raster_vertices(mesh, texture.as_ref(), model);
        // Untextured meshes sample a 1x1 white texture so the rasterizer and
        // lighting math stay identical for both paths.
        let tex_ref: &Texture = match texture {
            Some(t) => t,
            None => &self.white,
        };
        let opts = RasterOpts::textured(tex_ref);
        draw_triangles(&mut self.fb, &raster_verts, &mesh.indices, &opts);
        self.stats.draw_calls += 1;
        self.stats.triangles += (mesh.indices.len() / 3) as u64;
    }

    fn end_frame(&mut self) -> &Framebuffer {
        &self.fb
    }

    #[allow(dead_code)]
    fn framebuffer(&self) -> &Framebuffer {
        &self.fb
    }

    fn stats(&self) -> RenderStats {
        self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::camera::Camera;

    #[test]
    fn cube_renders_some_pixels() {
        let mut r = SoftwareRenderer::new(64, 48);
        let cube = Mesh::unit_cube();
        let handle = r.alloc_mesh(&cube, None);
        let cam = Camera::new(Vec3::new(0.0, 0.0, 2.5), 70.0, 64.0 / 48.0);
        let params = FrameParams {
            view_proj: cam.view_projection(),
            ..FrameParams::default()
        };
        r.begin_frame(&params);
        r.draw_mesh(handle, Mat4::IDENTITY);
        r.end_frame();

        // The cube must cover a solid block of the screen and the sky must be
        // visible at the edges.
        let center = r.framebuffer().pixel(32, 24);
        let corner = r.framebuffer().pixel(2, 2);
        assert_ne!(center, [0, 0, 0], "cube missing at center");
        assert_ne!(corner, [0, 0, 0], "sky gradient missing at corner");
        assert_ne!(center, corner, "cube and sky must differ");
        let stats = r.stats();
        assert_eq!(stats.draw_calls, 2); // sky + cube
        assert_eq!(stats.triangles, 14); // 2 sky + 12 cube
    }

    #[test]
    fn freed_mesh_draws_nothing() {
        let mut r = SoftwareRenderer::new(32, 32);
        let cube = Mesh::unit_cube();
        let handle = r.alloc_mesh(&cube, None);
        r.free_mesh(handle);
        let cam = Camera::new(Vec3::new(0.0, 0.0, 2.5), 70.0, 1.0);
        let params = FrameParams {
            view_proj: cam.view_projection(),
            ..FrameParams::default()
        };
        r.begin_frame(&params);
        r.draw_mesh(handle, Mat4::IDENTITY);
        r.end_frame();
        assert_eq!(r.stats().draw_calls, 1, "only the sky should draw");
    }
}
