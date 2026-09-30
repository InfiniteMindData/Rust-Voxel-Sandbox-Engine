//! First-person perspective camera.
//!
//! Conventions (matching `glam` and the future `wgpu` backend):
//! * Right-handed coordinates: `+X` right, `+Y` up, `-Z` forward.
//! * Yaw rotates around `+Y`; pitch is positive looking up.
//! * Depth range after projection is `0..1`.

use glam::{Mat4, Vec3};

/// Sensible pitch clamp just short of the gimbal lock point.
const MAX_PITCH: f32 = 89.0_f32.to_radians();

#[derive(Debug, Clone)]
pub struct Camera {
    /// World-space eye position.
    pub position: Vec3,
    /// Horizontal rotation in radians. `0.0` looks along `-Z`;
    /// increasing yaw turns to the left (towards `-X`... see [`Camera::forward`]).
    pub yaw: f32,
    /// Vertical rotation in radians, clamped to `[-MAX_PITCH, MAX_PITCH]`.
    pub pitch: f32,
    /// Vertical field of view in radians.
    pub fov_y: f32,
    /// Width / height of the viewport.
    pub aspect: f32,
    /// Near plane distance.
    pub near: f32,
    /// Far plane distance.
    pub far: f32,
}

impl Camera {
    pub fn new(position: Vec3, fov_y_degrees: f32, aspect: f32) -> Self {
        Self {
            position,
            yaw: 0.0,
            pitch: 0.0,
            fov_y: fov_y_degrees.to_radians(),
            aspect,
            near: 0.1,
            far: 1000.0,
        }
    }

    /// Unit forward direction (where the camera looks).
    pub fn forward(&self) -> Vec3 {
        Vec3::new(
            -self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            -self.yaw.cos() * self.pitch.cos(),
        )
    }

    /// Unit right direction.
    pub fn right(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin())
    }

    /// Clamps pitch into the valid range. Call after modifying `pitch`.
    pub fn clamp_pitch(&mut self) {
        self.pitch = self.pitch.clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// View matrix (world -> view space).
    pub fn view_matrix(&self) -> Mat4 {
        glam::camera::rh::view::look_to_mat4(self.position, self.forward(), Vec3::Y)
    }

    /// Projection matrix (view -> clip space, depth `0..1`).
    pub fn projection_matrix(&self) -> Mat4 {
        glam::camera::rh::proj::directx::perspective(self.fov_y, self.aspect, self.near, self.far)
    }

    /// Combined view-projection matrix.
    pub fn view_projection(&self) -> Mat4 {
        self.projection_matrix() * self.view_matrix()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec4;

    /// Projects a world point and returns NDC coordinates.
    fn project(cam: &Camera, p: Vec3) -> Vec3 {
        let clip = cam.view_projection() * Vec4::new(p.x, p.y, p.z, 1.0);
        (clip / clip.w).truncate()
    }

    #[test]
    fn looking_down_minus_z_centers_origin_ahead() {
        let cam = Camera::new(Vec3::new(0.0, 0.0, 5.0), 90.0, 1.0);
        // The point 5 units ahead of the eye must be exactly screen center.
        let ndc = project(&cam, Vec3::new(0.0, 0.0, 0.0));
        assert!(ndc.x.abs() < 1e-5);
        assert!(ndc.y.abs() < 1e-5);
        assert!((0.0..=1.0).contains(&ndc.z));
    }

    #[test]
    fn forward_basis_rotates_with_yaw() {
        let mut cam = Camera::new(Vec3::ZERO, 90.0, 1.0);
        assert!((cam.forward() - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-6);
        cam.yaw = std::f32::consts::FRAC_PI_2;
        assert!(
            (cam.forward() - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-6,
            "quarter turn left must face -X, got {:?}",
            cam.forward()
        );
        cam.yaw = -std::f32::consts::FRAC_PI_2;
        assert!((cam.forward() - Vec3::new(1.0, 0.0, 0.0)).length() < 1e-6);
    }

    #[test]
    fn pitch_clamps() {
        let mut cam = Camera::new(Vec3::ZERO, 90.0, 1.0);
        cam.pitch = 5.0;
        cam.clamp_pitch();
        assert!(cam.pitch <= MAX_PITCH);
        cam.pitch = -5.0;
        cam.clamp_pitch();
        assert!(cam.pitch >= -MAX_PITCH);
    }

    #[test]
    fn point_right_of_center_projects_right() {
        let cam = Camera::new(Vec3::ZERO, 90.0, 1.0);
        let ndc = project(&cam, Vec3::new(1.0, 0.0, -5.0));
        assert!(ndc.x > 0.0, "a point to the camera's right must project to positive NDC x");
    }

    #[test]
    fn aspect_splits_horizontal_fov() {
        let wide = Camera::new(Vec3::ZERO, 90.0, 2.0);
        // fov_y 90 deg => tan(45 deg) = 1; with aspect 2 the horizontal half
        // extent is 2, so a point 4 ahead and 8 right sits on the right edge.
        let ndc = project(&wide, Vec3::new(8.0, 0.0, -4.0));
        assert!((ndc.x - 1.0).abs() < 1e-4, "edge point must hit NDC x=1, got {}", ndc.x);
        let half = project(&wide, Vec3::new(4.0, 0.0, -4.0));
        assert!((half.x - 0.5).abs() < 1e-4, "half-way point must hit NDC x=0.5, got {}", half.x);
    }
}
