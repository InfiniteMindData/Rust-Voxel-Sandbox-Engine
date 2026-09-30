//! Mesh representation shared by the renderer backends.

use glam::Vec2;
use glam::Vec3;

/// A CPU-side mesh vertex: position, shading normal and texture coordinates.
#[derive(Debug, Clone, Copy)]
pub struct MeshVertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub uv: Vec2,
}

/// An indexed triangle mesh in model space.
#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
}

impl Mesh {
    /// A centered unit cube with per-face normals and `0..1` UVs per face.
    /// Face order: +X, -X, +Y, -Y, +Z, -Z (six quads = 24 vertices).
    pub fn unit_cube() -> Mesh {
        // (normal, tangent u, tangent v) per face.
        let faces = [
            (Vec3::X, -Vec3::Z, Vec3::Y),   // +X
            (-Vec3::X, Vec3::Z, Vec3::Y),   // -X
            (Vec3::Y, Vec3::X, -Vec3::Z),   // +Y
            (-Vec3::Y, Vec3::X, Vec3::Z),   // -Y
            (Vec3::Z, Vec3::X, Vec3::Y),    // +Z
            (-Vec3::Z, -Vec3::X, Vec3::Y),  // -Z
        ];

        let mut mesh = Mesh::default();
        for (normal, u_axis, v_axis) in faces {
            let base = mesh.vertices.len() as u32;
            let center = normal * 0.5;
            // Corners of the face quad, CCW when seen from outside.
            for (du, dv) in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
                mesh.vertices.push(MeshVertex {
                    position: center + u_axis * du + v_axis * dv,
                    normal,
                    uv: Vec2::new(du + 0.5, dv + 0.5),
                });
            }
            mesh.indices
                .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        mesh
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_cube_has_six_quads() {
        let cube = Mesh::unit_cube();
        assert_eq!(cube.vertices.len(), 24);
        assert_eq!(cube.indices.len(), 36);
        // All indices in range.
        assert!(cube.indices.iter().all(|&i| (i as usize) < cube.vertices.len()));
    }

    #[test]
    fn cube_vertices_are_unit_sized() {
        let cube = Mesh::unit_cube();
        for v in &cube.vertices {
            assert!(v.position.max_element() <= 0.5 + 1e-6);
            assert!(v.position.min_element() >= -0.5 - 1e-6);
        }
    }

    #[test]
    fn cube_normals_point_outward() {
        let cube = Mesh::unit_cube();
        for v in &cube.vertices {
            // A face vertex must lie on the outward side of its own normal.
            assert!(v.normal.dot(v.position) > 0.0, "normal {:?} vs position {:?}", v.normal, v.position);
        }
    }
}
