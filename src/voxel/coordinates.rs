//! Coordinate types and conversions for the voxel world.
//!
//! Three coordinate spaces exist and must never be mixed implicitly:
//!
//! * [`BlockPosition`] - a block in world space (signed, unbounded).
//! * [`ChunkPosition`] - which chunk (signed; chunks tile the world in X/Z;
//!   the world is one chunk tall in Y, spanning `[0, CHUNK_SIZE_Y)`).
//! * [`LocalBlockPosition`] - a block inside a chunk (`0..SIZE` per axis).
//!
//! All conversions live here and are exhaustively unit tested, especially
//! for negative coordinates and chunk boundaries.

/// Chunk edge length on X (blocks). Configurable engine-wide via this constant.
pub const CHUNK_SIZE_X: i32 = 16;
/// Chunk edge length on Z (blocks).
pub const CHUNK_SIZE_Z: i32 = 16;
/// World height in blocks (a chunk spans the full world column).
pub const CHUNK_SIZE_Y: i32 = 384;

/// A block position in world space. Signed: the world extends infinitely in
/// every direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockPosition {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPosition {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// Which chunk contains this block.
    pub fn chunk(&self) -> ChunkPosition {
        ChunkPosition::new(div_floor(self.x, CHUNK_SIZE_X), div_floor(self.z, CHUNK_SIZE_Z))
    }

    /// Position of this block inside its chunk.
    ///
    /// Panics if the block is outside the vertical world bounds - use
    /// [`BlockPosition::try_local`] at trust boundaries.
    pub fn local(&self) -> LocalBlockPosition {
        self.try_local()
            .unwrap_or_else(|| panic!("block y={} outside world height {CHUNK_SIZE_Y}", self.y))
    }

    /// Like [`BlockPosition::local`] but returns `None` instead of panicking.
    pub fn try_local(&self) -> Option<LocalBlockPosition> {
        if self.y < 0 || self.y >= CHUNK_SIZE_Y {
            return None;
        }
        Some(LocalBlockPosition::new(
            mod_floor_e(self.x, CHUNK_SIZE_X) as u16,
            self.y as u16,
            mod_floor_e(self.z, CHUNK_SIZE_Z) as u16,
        ))
    }
}

/// A chunk grid position (X/Z). The world is one chunk tall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkPosition {
    pub x: i32,
    pub z: i32,
}

impl ChunkPosition {
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    /// World-space block position of this chunk's minimum corner.
    pub fn origin(&self) -> BlockPosition {
        BlockPosition::new(self.x * CHUNK_SIZE_X, 0, self.z * CHUNK_SIZE_Z)
    }

    /// World-space position of a local block inside this chunk.
    ///
    /// Panics if `local` is out of bounds - internal engine code treats that
    /// as an invariant violation.
    pub fn to_world(&self, local: LocalBlockPosition) -> BlockPosition {
        assert!(
            local.is_in_bounds(),
            "local position {local:?} outside chunk bounds"
        );
        let o = self.origin();
        BlockPosition::new(o.x + local.x as i32, local.y as i32, o.z + local.z as i32)
    }
}

/// A block position local to a chunk (`0..SIZE` per axis).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalBlockPosition {
    pub x: u16,
    pub y: u16,
    pub z: u16,
}

impl LocalBlockPosition {
    pub const fn new(x: u16, y: u16, z: u16) -> Self {
        Self { x, y, z }
    }

    /// Whether this position is inside chunk bounds.
    pub fn is_in_bounds(&self) -> bool {
        (self.x as i32) < CHUNK_SIZE_X
            && (self.y as i32) < CHUNK_SIZE_Y
            && (self.z as i32) < CHUNK_SIZE_Z
    }

    /// Flat array index into a chunk's voxel storage: `x + z*SX + y*SX*SZ`.
    pub fn index(&self) -> usize {
        debug_assert!(self.is_in_bounds(), "index of out-of-bounds local position");
        (self.x as usize)
            + (self.z as usize) * CHUNK_SIZE_X as usize
            + (self.y as usize) * (CHUNK_SIZE_X as usize) * (CHUNK_SIZE_Z as usize)
    }
}

/// Euclidean-style floor division (result of Python's `//`).
///
/// Rust's `/` truncates toward zero, which is wrong for chunk math on
/// negative coordinates: `-1 / 16 == 0` but block -1 belongs to chunk -1.
#[inline]
pub fn div_floor(a: i32, b: i32) -> i32 {
    debug_assert!(b > 0);
    let q = a / b;
    if a % b != 0 && a < 0 {
        q - 1
    } else {
        q
    }
}

/// Euclidean-style modulo, always in `0..b` (Rust's `%` keeps the sign).
#[inline]
pub fn mod_floor_e(a: i32, b: i32) -> i32 {
    debug_assert!(b > 0);
    let m = a % b;
    if m < 0 {
        m + b
    } else {
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn div_floor_matches_chunk_semantics() {
        assert_eq!(div_floor(0, 16), 0);
        assert_eq!(div_floor(15, 16), 0);
        assert_eq!(div_floor(16, 16), 1);
        assert_eq!(div_floor(-1, 16), -1);
        assert_eq!(div_floor(-16, 16), -1);
        assert_eq!(div_floor(-17, 16), -2);
        // Extreme values stay representable: floor(-2147483647 / 16) = -134217728.
        assert_eq!(div_floor(i32::MIN + 1, 16), -134_217_728);
    }

    #[test]
    fn mod_floor_is_always_positive() {
        assert_eq!(mod_floor_e(0, 16), 0);
        assert_eq!(mod_floor_e(15, 16), 15);
        assert_eq!(mod_floor_e(16, 16), 0);
        assert_eq!(mod_floor_e(-1, 16), 15);
        assert_eq!(mod_floor_e(-16, 16), 0);
        assert_eq!(mod_floor_e(-17, 16), 15);
    }

    #[test]
    fn roundtrip_world_chunk_world() {
        for &bx in &[0i32, 1, 15, 16, -1, -15, -16, -17, 1234, -1234] {
            for &bz in &[0i32, 1, 15, 16, -1, -16, -31] {
                let p = BlockPosition::new(bx, 5, bz);
                let chunk = p.chunk();
                let local = p.local();
                let back = chunk.to_world(local);
                assert_eq!(back, p, "roundtrip failed for {p:?}");
            }
        }
    }

    #[test]
    fn negative_coordinates_belong_to_negative_chunks() {
        assert_eq!(BlockPosition::new(-1, 0, -1).chunk(), ChunkPosition::new(-1, -1));
        assert_eq!(BlockPosition::new(-16, 0, -16).chunk(), ChunkPosition::new(-1, -1));
        assert_eq!(BlockPosition::new(-17, 0, 0).chunk(), ChunkPosition::new(-2, 0));
        let l = BlockPosition::new(-1, 0, -1).local();
        assert_eq!((l.x, l.z), (15, 15), "block -1 must be the last block of chunk -1");
    }

    #[test]
    fn chunk_edges_are_consistent() {
        // x = 15 is inside chunk 0; x = 16 is the first block of chunk 1.
        assert_eq!(BlockPosition::new(15, 0, 0).chunk(), ChunkPosition::new(0, 0));
        assert_eq!(BlockPosition::new(16, 0, 0).chunk(), ChunkPosition::new(1, 0));
        assert_eq!(BlockPosition::new(15, 0, 0).local().x, 15);
        assert_eq!(BlockPosition::new(16, 0, 0).local().x, 0);
    }

    #[test]
    fn large_coordinates_are_exact() {
        let big = 1_000_000_000i32; // ~1e9 blocks
        let p = BlockPosition::new(big, 0, -big);
        let chunk = p.chunk();
        let back = chunk.to_world(p.local());
        assert_eq!(back, p);
        // The chunk index is exactly big / 16 (1e9 divides evenly here).
        assert_eq!(chunk.x, 62_500_000);
        assert_eq!(chunk.z, -62_500_000);
    }

    #[test]
    fn vertical_bounds_enforced() {
        assert!(BlockPosition::new(0, 0, 0).try_local().is_some());
        assert!(BlockPosition::new(0, CHUNK_SIZE_Y - 1, 0).try_local().is_some());
        assert_eq!(BlockPosition::new(0, CHUNK_SIZE_Y, 0).try_local(), None);
        assert_eq!(BlockPosition::new(0, -1, 0).try_local(), None);
    }

    #[test]
    fn local_index_layout_matches_expected_order() {
        // Index ordering: x fastest, then z, then y.
        assert_eq!(LocalBlockPosition::new(0, 0, 0).index(), 0);
        assert_eq!(LocalBlockPosition::new(1, 0, 0).index(), 1);
        assert_eq!(
            LocalBlockPosition::new(0, 0, 1).index(),
            CHUNK_SIZE_X as usize
        );
        assert_eq!(
            LocalBlockPosition::new(0, 1, 0).index(),
            (CHUNK_SIZE_X * CHUNK_SIZE_Z) as usize
        );
    }

    #[test]
    fn origin_maps_to_local_zero() {
        let c = ChunkPosition::new(3, -4);
        let o = c.origin();
        assert_eq!(o, BlockPosition::new(48, 0, -64));
        assert_eq!(o.chunk(), c);
        assert_eq!(o.local(), LocalBlockPosition::new(0, 0, 0));
    }
}
