//! Block definitions and the data-driven block model.
//!
//! A [`BlockId`] is a compact numeric handle (what chunks store); a
//! [`BlockDefinition`] is the immutable description of what that handle
//! means (how it renders, collides, emits light, ...). Rendering and gameplay
//! code never hardcode block behavior - they query the [`crate::voxel::registry::BlockRegistry`].

/// Compact numeric block identifier stored in chunk voxel arrays.
///
/// `0` is always [`AIR_ID`] (air), so a freshly allocated chunk is empty air.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u16);

/// The block id guaranteed to be air in every registry.
pub const AIR_ID: u16 = 0;

/// Identifies which texture of a block definition applies to a face.
/// (Consumed by the texture atlas milestone, milestone 5.)
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockFace {
    Top,
    Bottom,
    Side,
}

/// Hardness of a block, governing how long it takes to break.
///
/// `Hardness::Instant` breaks immediately (e.g. flowers, later torches).
/// Values are in arbitrary engine units; scale is tuned per material.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hardness {
    Instant,
    Value(f32),
    /// Cannot be broken by the player. (Consumed by block interaction,
    /// milestone 13; exercised by unit tests.)
    #[allow(dead_code)]
    Unbreakable,
}

impl Hardness {
    /// Break time in seconds with a nominal tool, or `None` if unbreakable /
    /// instant. (Consumed by block interaction, milestone 13.)
    #[allow(dead_code)]
    pub fn break_seconds(&self) -> Option<f32> {
        match self {
            Hardness::Instant => Some(0.0),
            Hardness::Value(v) => Some(*v),
            Hardness::Unbreakable => None,
        }
    }
}

/// Per-face texture names as referenced by the texture atlas (milestone 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceTextures {
    pub top: String,
    pub bottom: String,
    pub side: String,
}

impl FaceTextures {
    /// The same texture on every face.
    pub fn uniform(name: &str) -> Self {
        Self {
            top: name.to_string(),
            bottom: name.to_string(),
            side: name.to_string(),
        }
    }

    /// Distinct top/bottom/side textures (the classic log/Grass pattern).
    pub fn tbs(top: &str, bottom: &str, side: &str) -> Self {
        Self {
            top: top.to_string(),
            bottom: bottom.to_string(),
            side: side.to_string(),
        }
    }

    /// Texture for a given face. (Consumed by the mesher + atlas, milestone 4-5.)
    #[allow(dead_code)]
    pub fn face(&self, face: BlockFace) -> &str {
        match face {
            BlockFace::Top => &self.top,
            BlockFace::Bottom => &self.bottom,
            BlockFace::Side => &self.side,
        }
    }
}

/// The immutable description of one block type.
///
/// Conceptually:
///
/// ```text
/// BlockDefinition {
///     id, name, solid, transparent, hardness, textures, light_emission,
/// }
/// ```
#[derive(Debug, Clone)]
pub struct BlockDefinition {
    /// Numeric handle stored in chunks. Always equal to the registry slot.
    pub id: BlockId,
    /// Unique machine name ("grass", "stone", ...).
    pub name: String,
    /// Display name for UI.
    pub display_name: String,
    /// Participates in collision (players/mobs cannot pass through).
    pub solid: bool,
    /// Renders neighbor faces (air, glass, leaves, water are transparent).
    pub transparent: bool,
    /// Fully blocks light propagation (used by the lighting milestone).
    #[allow(dead_code)]
    pub opaque: bool,
    /// How long the block takes to break.
    pub hardness: Hardness,
    /// Texture names per face (resolved to atlas UVs by the renderer).
    pub textures: FaceTextures,
    /// Light emitted (0 for non-emissive blocks; torches/lava in milestone
    /// 16; exercised by unit tests).
    #[allow(dead_code)]
    pub light_emission: u8,
}

impl BlockDefinition {
    /// Starts a builder-style definition for a basic solid, opaque cube.
    pub fn new(id: BlockId, name: &str, display_name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            display_name: display_name.to_string(),
            solid: true,
            transparent: false,
            opaque: true,
            hardness: Hardness::Value(1.0),
            textures: FaceTextures::uniform(name),
            light_emission: 0,
        }
    }

    /// The standard air block: not solid, transparent, no light.
    pub fn air() -> Self {
        let mut def = Self::new(BlockId(AIR_ID), "air", "Air");
        def.solid = false;
        def.transparent = true;
        def.opaque = false;
        def.hardness = Hardness::Instant;
        def
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardness_semantics() {
        assert_eq!(Hardness::Instant.break_seconds(), Some(0.0));
        assert_eq!(Hardness::Value(2.5).break_seconds(), Some(2.5));
        assert_eq!(Hardness::Unbreakable.break_seconds(), None);
    }

    #[test]
    fn face_textures_lookup() {
        let t = FaceTextures::tbs("log_top", "log_top", "log_side");
        assert_eq!(t.face(BlockFace::Top), "log_top");
        assert_eq!(t.face(BlockFace::Bottom), "log_top");
        assert_eq!(t.face(BlockFace::Side), "log_side");
        let u = FaceTextures::uniform("stone");
        assert_eq!(u.face(BlockFace::Top), "stone");
    }

    #[test]
    fn air_defaults() {
        let air = BlockDefinition::air();
        assert_eq!(air.id, BlockId(AIR_ID));
        assert!(!air.solid);
        assert!(air.transparent);
        assert!(!air.opaque);
    }
}
