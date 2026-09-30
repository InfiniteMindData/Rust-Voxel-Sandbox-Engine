//! Engine layer: application loop, display backend, input, camera, timing and logging.
//!
//! The engine is deliberately decoupled from voxel/gameplay concerns so that a
//! different display backend (for example a native `winit` + `wgpu` window once
//! those crates are fetchable in the build environment) can be added behind the
//! same seams.

pub mod app;
pub mod camera;
pub mod error;
pub mod input;
pub mod logging;
pub mod renderer;
pub mod timing;
pub mod window;

pub use error::{EngineError, Result};
