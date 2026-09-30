//! Voxel layer: blocks, chunks, coordinates and world storage.
//!
//! The data model (registry, chunks, coordinate types) is completed and
//! unit-tested ahead of its consumers: the mesher (milestone 4), terrain
//! (6) and streaming (10) wire it into the live game. Until then most items
//! are exercised only by tests, so dead-code analysis is silenced at module
//! level. **Remove this allow as consumers land** - it should be gone by
//! milestone 6.
#![allow(dead_code)]

pub mod block;
pub mod chunk;
pub mod coordinates;
pub mod registry;
