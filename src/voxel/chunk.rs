//! Chunk storage: a flat, compact voxel array for one world column.
//!
//! A chunk is a contiguous `Vec<BlockId>` in the index order defined by
//! [`LocalBlockPosition::index`] (x fastest, then z, then y) - one heap
//! allocation per chunk, one `u16` per block, no per-block objects.
//!
//! Dimensions are engine constants (see `coordinates`); keeping them fixed
//! lets index math stay branch-free while remaining trivially editable in
//! one place.

use crate::voxel::block::{BlockId, AIR_ID};
use crate::voxel::coordinates::{CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z, LocalBlockPosition};

/// One 16 x 384 x 16 world column of blocks.
#[derive(Debug, Clone)]
pub struct Chunk {
    blocks: Vec<BlockId>,
    /// Number of non-air blocks, maintained incrementally so `is_empty` and
    /// (later) mesh/save decisions are O(1).
    non_air: usize,
}

impl Chunk {
    /// A chunk filled entirely with air.
    pub fn air() -> Self {
        Self {
            blocks: vec![BlockId(AIR_ID); Self::voxel_count()],
            non_air: 0,
        }
    }

    /// Total number of block slots in a chunk.
    pub const fn voxel_count() -> usize {
        (CHUNK_SIZE_X * CHUNK_SIZE_Y * CHUNK_SIZE_Z) as usize
    }

    /// Reads the block at `pos`. Out-of-bounds reads are an invariant
    /// violation and panic (callers check bounds when the position is
    /// external input).
    pub fn get_block(&self, pos: LocalBlockPosition) -> BlockId {
        assert!(pos.is_in_bounds(), "get_block out of bounds: {pos:?}");
        self.blocks[pos.index()]
    }

    /// Writes the block at `pos`. Returns the previous block id.
    pub fn set_block(&mut self, pos: LocalBlockPosition, id: BlockId) -> BlockId {
        assert!(pos.is_in_bounds(), "set_block out of bounds: {pos:?}");
        let idx = pos.index();
        let previous = self.blocks[idx];
        if previous != id {
            if previous.0 == AIR_ID {
                self.non_air += 1;
            } else if id.0 == AIR_ID {
                self.non_air -= 1;
            }
            self.blocks[idx] = id;
        }
        previous
    }

    /// Fills the entire chunk with `id`. Returns the number of slots written
    /// (always the full chunk volume).
    pub fn fill(&mut self, id: BlockId) -> usize {
        self.non_air = if id.0 == AIR_ID { 0 } else { Self::voxel_count() };
        let written = self.blocks.len();
        self.blocks.fill(id);
        written
    }

    /// Fills an axis-aligned local box (inclusive bounds, clamped to the
    /// chunk). Returns the number of slots written. (Consumed by structure
    /// generation, milestone 9.)
    #[allow(dead_code)]
    pub fn fill_box(
        &mut self,
        min: LocalBlockPosition,
        max: LocalBlockPosition,
        id: BlockId,
    ) -> usize {
        let clamp = |v: u16, limit: i32| (v as i32).min(limit - 1).max(0) as u16;
        let min = LocalBlockPosition::new(
            clamp(min.x, CHUNK_SIZE_X),
            clamp(min.y, CHUNK_SIZE_Y),
            clamp(min.z, CHUNK_SIZE_Z),
        );
        let max = LocalBlockPosition::new(
            clamp(max.x, CHUNK_SIZE_X),
            clamp(max.y, CHUNK_SIZE_Y),
            clamp(max.z, CHUNK_SIZE_Z),
        );
        let mut written = 0usize;
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                for x in min.x..=max.x {
                    self.set_block(LocalBlockPosition::new(x, y, z), id);
                    written += 1;
                }
            }
        }
        written
    }

    /// Whether the chunk contains only air.
    pub fn is_empty(&self) -> bool {
        self.non_air == 0
    }

    /// Number of non-air blocks (O(1)). (Used by meshing decisions, milestone 4.)
    #[allow(dead_code)]
    pub fn non_air_count(&self) -> usize {
        self.non_air
    }

    /// Direct read access to the voxel array (meshers, savers; milestone 4+).
    #[allow(dead_code)]
    pub fn blocks(&self) -> &[BlockId] {
        &self.blocks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voxel::block::BlockDefinition;
    use crate::voxel::registry::BlockRegistry;

    fn ids() -> (BlockRegistry, BlockId, BlockId) {
        let r = BlockRegistry::with_builtins();
        let stone = r.id_of("stone").unwrap();
        let glass = r.id_of("glass").unwrap();
        (r, stone, glass)
    }

    #[test]
    fn new_chunk_is_all_air() {
        let c = Chunk::air();
        assert!(c.is_empty());
        assert_eq!(c.non_air_count(), 0);
        let center = LocalBlockPosition::new(8, 100, 8);
        assert_eq!(c.get_block(center).0, AIR_ID);
        // Spot-check the corners including the y extremes.
        for p in [
            LocalBlockPosition::new(0, 0, 0),
            LocalBlockPosition::new(15, 0, 15),
            LocalBlockPosition::new(15, (CHUNK_SIZE_Y - 1) as u16, 15),
        ] {
            assert_eq!(c.get_block(p).0, AIR_ID);
        }
    }

    #[test]
    fn set_then_get_roundtrip() {
        let (r, stone, _glass) = ids();
        let mut c = Chunk::air();
        let p = LocalBlockPosition::new(3, 64, 9);
        assert_eq!(c.set_block(p, stone), BlockId(AIR_ID), "returns previous");
        assert_eq!(c.get_block(p), stone);
        assert!(!c.is_empty());
        assert_eq!(c.non_air_count(), 1);
        // Overwrite: same id -> counter unchanged.
        c.set_block(p, stone);
        assert_eq!(c.non_air_count(), 1);
        // Overwrite with air -> back to empty.
        c.set_block(p, r.air());
        assert!(c.is_empty());
    }

    #[test]
    fn edge_blocks_are_addressable() {
        let (_r, stone, _g) = ids();
        let mut c = Chunk::air();
        let corners = [
            LocalBlockPosition::new(0, 0, 0),
            LocalBlockPosition::new(15, 0, 0),
            LocalBlockPosition::new(0, 0, 15),
            LocalBlockPosition::new(15, 0, 15),
            LocalBlockPosition::new(0, (CHUNK_SIZE_Y - 1) as u16, 0),
            LocalBlockPosition::new(15, (CHUNK_SIZE_Y - 1) as u16, 15),
        ];
        for p in corners {
            c.set_block(p, stone);
            assert_eq!(c.get_block(p), stone);
        }
        assert_eq!(c.non_air_count(), corners.len());
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn get_out_of_bounds_panics() {
        let c = Chunk::air();
        let _ = c.get_block(LocalBlockPosition::new(16, 0, 0));
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn set_out_of_bounds_panics() {
        let mut c = Chunk::air();
        let _ = c.set_block(LocalBlockPosition::new(0, CHUNK_SIZE_Y as u16, 0), BlockId(1));
    }

    #[test]
    fn fill_makes_full_and_revertable() {
        let (r, stone, _g) = ids();
        let mut c = Chunk::air();
        let written = c.fill(stone);
        assert_eq!(written, Chunk::voxel_count());
        assert!(!c.is_empty());
        assert_eq!(c.non_air_count(), Chunk::voxel_count());
        assert_eq!(c.get_block(LocalBlockPosition::new(7, 200, 7)), stone);
        let written = c.fill(r.air());
        assert_eq!(written, Chunk::voxel_count());
        assert!(c.is_empty());
    }

    #[test]
    fn fill_box_is_clamped_and_counted() {
        let (_r, stone, glass) = ids();
        let mut c = Chunk::air();
        // A 2x2x2 box at the corner.
        let min = LocalBlockPosition::new(0, 0, 0);
        let max = LocalBlockPosition::new(1, 1, 1);
        assert_eq!(c.fill_box(min, max, stone), 8);
        assert_eq!(c.non_air_count(), 8);
        // A box hanging off the +X/+Z edge gets clamped to the chunk.
        let min = LocalBlockPosition::new(14, 5, 14);
        let max = LocalBlockPosition::new(20, 6, 20);
        assert_eq!(c.fill_box(min, max, glass), 8); // 2x2x2 after clamp
        assert_eq!(c.non_air_count(), 16);
        assert_eq!(c.get_block(LocalBlockPosition::new(15, 6, 15)), glass);
        assert_eq!(c.get_block(LocalBlockPosition::new(14, 5, 14)), glass);
    }

    #[test]
    fn index_roundtrip_via_coordinates() {
        // Chunk storage must agree with the coordinate module's layout.
        use crate::voxel::coordinates::{BlockPosition, ChunkPosition};
        let (_r, stone, _g) = ids();
        let mut c = Chunk::air();
        let chunk = ChunkPosition::new(-1, 2);
        let world = BlockPosition::new(-2, 17, 33); // inside chunk (-1, 2)
        let local = world.local();
        c.set_block(local, stone);
        assert_eq!(c.get_block(local), stone);
        assert!(chunk.to_world(local) == world);
    }
}
