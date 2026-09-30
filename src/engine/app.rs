//! The application: owns the main loop, wires input -> simulation -> render
//! -> display.

use std::time::{Duration, Instant};

use glam::{Mat4, Quat, Vec3};

use crate::engine::camera::Camera;
use crate::engine::input::{InputState, Key};
use crate::engine::renderer::{FrameParams, Renderer, SoftwareRenderer};
use crate::engine::timing::{FixedTimestep, FpsCounter};
use crate::engine::window::{DisplayConfig, EngineStats, HttpDisplay};
use crate::engine::Result;
use crate::log_info;
use crate::render::mesh::Mesh;
use crate::render::texture::Texture;

const LOG_TARGET: &str = "app";

/// Runtime configuration, overridable from the command line.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub port: u16,
    pub width: u32,
    pub height: u32,
    /// Simulation ticks per second (fixed timestep).
    pub target_ups: u32,
    /// Rendered frames per second cap.
    pub target_render_fps: u32,
    /// Vertical field of view in degrees.
    pub fov_degrees: f32,
    /// Radians of rotation per pixel of mouse movement.
    pub mouse_sensitivity: f64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            width: 480,
            height: 270,
            target_ups: 60,
            target_render_fps: 30,
            fov_degrees: 70.0,
            mouse_sensitivity: 0.0025,
        }
    }
}

// Fly-camera movement speed in blocks per second.
const WALK_SPEED: f32 = 3.5;
const SPRINT_FACTOR: f32 = 2.5;

/// Game application instance.
pub struct App {
    config: AppConfig,
    display: HttpDisplay,
    renderer: SoftwareRenderer,
    camera: Camera,
    cube_mesh: crate::engine::renderer::MeshHandle,
    sim_time: f32,
    started: Instant,
    timestep: FixedTimestep,
    fps: FpsCounter,
}

impl App {
    /// Creates the app: display server, renderer, initial scene.
    pub fn new(config: AppConfig) -> Result<Self> {
        let display = HttpDisplay::start(DisplayConfig {
            bind: std::net::SocketAddr::from(([0, 0, 0, 0], config.port)),
        })?;
        log_info!(LOG_TARGET, "display server listening on {}", display.addr());

        // The voxel layer is wired up here for the data-model milestones;
        // rendering of voxels arrives with the meshing milestone.
        let registry = crate::voxel::registry::BlockRegistry::with_builtins();
        log_info!(
            LOG_TARGET,
            "block registry initialized with {} blocks",
            registry.len()
        );

        let mut renderer = SoftwareRenderer::new(config.width, config.height);

        // Milestone 1 test scene: a single textured, slowly rotating cube.
        let cube = Mesh::unit_cube();
        let checker_texture = Self::checker_texture(16);
        let cube_mesh = renderer.alloc_mesh(&cube, Some(checker_texture.clone()));

        let aspect = config.width as f32 / config.height as f32;
        let mut camera = Camera::new(Vec3::new(1.4, 0.9, 2.6), config.fov_degrees, aspect);
        // Look slightly down-left towards the origin.
        camera.yaw = 0.44;
        camera.pitch = -0.12;

        let timestep = FixedTimestep::new(
            Duration::from_secs_f64(1.0 / config.target_ups as f64),
            5,
        );

        Ok(Self {
            config,
            display,
            renderer,
            camera,
            cube_mesh,
            sim_time: 0.0,
            started: Instant::now(),
            timestep,
            fps: FpsCounter::new(20.0),
        })
    }

    /// A small two-tone checker texture used until the real texture atlas
    /// (milestone 5) replaces it.
    fn checker_texture(cells: u32) -> Texture {
        // Original palette: warm sand and deep teal.
        Texture::checker(
            cells,
            cells,
            4,
            [214, 177, 124, 255],
            [42, 111, 119, 255],
        )
    }

    /// Runs until the process is terminated.
    pub fn run(mut self) -> Result<()> {
        log_info!(
            LOG_TARGET,
            "game running: {}x{} @ {} sim Hz / {} render fps cap",
            self.config.width,
            self.config.height,
            self.config.target_ups,
            self.config.target_render_fps
        );

        let sim_step = self.timestep.step();
        let render_interval = Duration::from_secs_f64(1.0 / self.config.target_render_fps as f64);
        let mut last_frame = Instant::now();
        let mut last_render_start: Option<Instant> = None;
        let mut next_render_at = last_frame + render_interval;

        loop {
            let now = Instant::now();
            let elapsed = now - last_frame;
            last_frame = now;

            let steps = self.timestep.tick(elapsed);
            for _ in 0..steps {
                self.simulate(sim_step.as_secs_f32());
            }

            if now >= next_render_at {
                // FPS measures the true wall-clock cadence between frame
                // starts, not how long a render call takes.
                if let Some(prev) = last_render_start {
                    self.fps.record(now - prev);
                }
                last_render_start = Some(now);
                self.render_and_publish();
                // Schedule the next frame; if we are behind, render next time
                // immediately instead of stacking up.
                next_render_at = (next_render_at + render_interval).max(Instant::now());
            }

            // Yield briefly; the loop is paced by render deadlines.
            let sleep_until = next_render_at.min(last_frame + Duration::from_millis(2));
            let sleep_for = sleep_until.saturating_duration_since(Instant::now());
            if sleep_for > Duration::ZERO {
                std::thread::sleep(sleep_for.min(Duration::from_millis(4)));
            }
        }
    }

    /// One fixed simulation step.
    fn simulate(&mut self, dt: f32) {
        self.sim_time += dt;

        // Clone the Arc so no borrow of `self` is held while the lock is.
        let input = std::sync::Arc::clone(self.display.input());
        let mut input = match input.lock() {
            Ok(i) => i,
            Err(poisoned) => poisoned.into_inner(),
        };
        self.apply_camera_input(&mut input, dt);
        // Button events are consumed but unused until block interaction
        // (milestone 13) arrives.
        let _ = input.take_button_events();
    }

    /// Reads accumulated input and moves the fly camera.
    fn apply_camera_input(&mut self, input: &mut InputState, dt: f32) {
        let (dx, dy) = input.take_mouse_delta();
        let sensitivity = self.config.mouse_sensitivity;
        self.camera.yaw -= (dx * sensitivity) as f32;
        self.camera.pitch -= (dy * sensitivity) as f32;
        self.camera.clamp_pitch();

        let mut speed = WALK_SPEED;
        if input.is_down(Key::ShiftLeft) || input.is_down(Key::ShiftRight) {
            speed *= SPRINT_FACTOR;
        }

        let forward = self.camera.forward();
        let right = self.camera.right();
        let mut move_dir = Vec3::ZERO;
        if input.is_down(Key::W) {
            move_dir += forward;
        }
        if input.is_down(Key::S) {
            move_dir -= forward;
        }
        if input.is_down(Key::D) {
            move_dir += right;
        }
        if input.is_down(Key::A) {
            move_dir -= right;
        }
        if input.is_down(Key::Space) {
            move_dir += Vec3::Y;
        }
        if input.is_down(Key::ControlLeft) {
            move_dir -= Vec3::Y;
        }
        if move_dir.length_squared() > 0.0 {
            self.camera.position += move_dir.normalize() * speed * dt;
        }
    }

    /// Renders one frame and hands it to the display.
    fn render_and_publish(&mut self) {
        let spin = Quat::from_rotation_y(self.sim_time * 0.8)
            * Quat::from_rotation_x(self.sim_time * 0.53);
        let model = Mat4::from_rotation_translation(spin, Vec3::ZERO);

        let params = FrameParams {
            view_proj: self.camera.view_projection(),
            sun_dir: Vec3::new(0.55, 0.85, 0.35).normalize(),
            ..FrameParams::default()
        };

        self.renderer.begin_frame(&params);
        self.renderer.draw_mesh(self.cube_mesh, model);
        self.renderer.end_frame();

        let stats = self.renderer.stats();
        self.display.publish_frame(self.renderer.end_frame());

        let render_stats = stats;
        self.display.update_stats(EngineStats {
            fps: self.fps.fps(),
            frame_ms: self.fps.frame_ms(),
            width: self.config.width,
            height: self.config.height,
            uptime_s: self.started.elapsed().as_secs_f32(),
            pos: [
                self.camera.position.x,
                self.camera.position.y,
                self.camera.position.z,
            ],
            yaw_deg: self.camera.yaw.to_degrees(),
            pitch_deg: self.camera.pitch.to_degrees(),
            triangles: render_stats.triangles,
            draw_calls: render_stats.draw_calls,
            simulation_hz: self.config.target_ups as f32,
        });
    }
}
