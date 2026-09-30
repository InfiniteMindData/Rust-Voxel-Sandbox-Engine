//! The block registry: maps [`BlockId`] <-> [`BlockDefinition`] and names.
//!
//! This is one of the extension points for modding (milestone 25): content
//! registers itself here, and everything else (renderer, mesher, gameplay)
//! consults the registry instead of hardcoding behavior.

use std::collections::HashMap;

use crate::voxel::block::{BlockDefinition, BlockId, AIR_ID};

/// Typed error for registry operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// A block with this name is already registered.
    DuplicateName(String),
    /// A block with this id is already registered.
    DuplicateId(BlockId),
    /// No block with this name is registered.
    UnknownName(String),
    /// No block with this id is registered.
    UnknownId(BlockId),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::DuplicateName(n) => write!(f, "duplicate block name '{n}'"),
            RegistryError::DuplicateId(id) => write!(f, "duplicate block id {id:?}"),
            RegistryError::UnknownName(n) => write!(f, "unknown block name '{n}'"),
            RegistryError::UnknownId(id) => write!(f, "unknown block id {id:?}"),
        }
    }
}

impl std::error::Error for RegistryError {}

/// The set of all known block definitions.
///
/// Ids are assigned sequentially on registration; slot `i` of `blocks` is
/// always the definition with `id == BlockId(i)`, making id -> definition a
/// bounds-checked array access.
#[derive(Debug, Clone)]
pub struct BlockRegistry {
    blocks: Vec<BlockDefinition>,
    by_name: HashMap<String, BlockId>,
}

impl BlockRegistry {
    /// Creates an empty registry.
    pub fn empty() -> Self {
        Self {
            blocks: Vec::new(),
            by_name: HashMap::new(),
        }
    }

    /// Creates a registry pre-populated with the built-in starter blocks
    /// (air, grass, dirt, stone, sand, wood, leaves, glass, water).
    pub fn with_builtins() -> Self {
        use crate::voxel::block::{FaceTextures, Hardness};

        let mut r = Self::empty();

        r.register(BlockDefinition::air())
            .expect("builtin blocks must register cleanly");

        let mut solid = |name: &str, display: &str, hardness: f32| {
            r.register(BlockDefinition {
                hardness: Hardness::Value(hardness),
                ..BlockDefinition::new(BlockId(0), name, display)
            })
            .expect("builtin blocks must register cleanly")
        };

        solid("grass", "Grass", 0.6);
        solid("dirt", "Dirt", 0.5);
        solid("stone", "Stone", 1.5);
        solid("sand", "Sand", 0.5);

        r.register(BlockDefinition {
            textures: FaceTextures::tbs("wood_top", "wood_top", "wood_side"),
            hardness: Hardness::Value(2.0),
            ..BlockDefinition::new(BlockId(0), "wood", "Wood")
        })
        .expect("builtin blocks must register cleanly");

        r.register(BlockDefinition {
            transparent: true,
            hardness: Hardness::Value(0.2),
            ..BlockDefinition::new(BlockId(0), "leaves", "Leaves")
        })
        .expect("builtin blocks must register cleanly");

        r.register(BlockDefinition {
            transparent: true,
            hardness: Hardness::Value(0.3),
            ..BlockDefinition::new(BlockId(0), "glass", "Glass")
        })
        .expect("builtin blocks must register cleanly");

        r.register(BlockDefinition {
            solid: true, // water gets its own swim physics in the water milestone
            transparent: true,
            opaque: false,
            hardness: Hardness::Instant,
            ..BlockDefinition::new(BlockId(0), "water", "Water")
        })
        .expect("builtin blocks must register cleanly");

        r
    }

    /// Registers a new block definition, assigning its final id.
    ///
    /// The `id` field of `def` is overwritten with the assigned id; name
    /// uniqueness is enforced.
    pub fn register(&mut self, mut def: BlockDefinition) -> Result<BlockId, RegistryError> {
        if self.by_name.contains_key(&def.name) {
            return Err(RegistryError::DuplicateName(def.name));
        }
        let id = BlockId(self.blocks.len() as u16);
        def.id = id;
        self.by_name.insert(def.name.clone(), id);
        self.blocks.push(def);
        Ok(id)
    }

    /// The definition for `id`, or `None` if out of range.
    pub fn definition(&self, id: BlockId) -> Option<&BlockDefinition> {
        self.blocks.get(id.0 as usize)
    }

    /// The definition for a block name.
    pub fn by_name(&self, name: &str) -> Result<&BlockDefinition, RegistryError> {
        self.by_name
            .get(name)
            .and_then(|&id| self.definition(id))
            .ok_or_else(|| RegistryError::UnknownName(name.to_string()))
    }

    /// Resolves a name to its id.
    pub fn id_of(&self, name: &str) -> Result<BlockId, RegistryError> {
        self.by_name
            .get(name)
            .copied()
            .ok_or_else(|| RegistryError::UnknownName(name.to_string()))
    }

    /// The air block id (always [`AIR_ID`], always registered).
    pub fn air(&self) -> BlockId {
        BlockId(AIR_ID)
    }

    /// Number of registered blocks.
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voxel::block::Hardness;

    #[test]
    fn builtins_register_in_order() {
        let r = BlockRegistry::with_builtins();
        assert_eq!(r.id_of("air").unwrap(), BlockId(AIR_ID));
        assert_eq!(r.id_of("grass").unwrap(), BlockId(1));
        assert_eq!(r.id_of("dirt").unwrap(), BlockId(2));
        assert_eq!(r.id_of("stone").unwrap(), BlockId(3));
        assert_eq!(r.id_of("sand").unwrap(), BlockId(4));
        assert_eq!(r.id_of("wood").unwrap(), BlockId(5));
        assert_eq!(r.id_of("leaves").unwrap(), BlockId(6));
        assert_eq!(r.id_of("glass").unwrap(), BlockId(7));
        assert_eq!(r.id_of("water").unwrap(), BlockId(8));
        assert_eq!(r.len(), 9);
    }

    #[test]
    fn definition_lookup_by_id_and_name() {
        let r = BlockRegistry::with_builtins();
        let stone = r.by_name("stone").unwrap();
        assert_eq!(stone.name, "stone");
        assert_eq!(stone.hardness, Hardness::Value(1.5));
        let by_id = r.definition(BlockId(3)).unwrap();
        assert_eq!(by_id.name, "stone");
        assert!(r.definition(BlockId(999)).is_none());
    }

    #[test]
    fn wood_has_different_face_textures() {
        let r = BlockRegistry::with_builtins();
        let wood = r.by_name("wood").unwrap();
        assert_eq!(wood.textures.top, "wood_top");
        assert_eq!(wood.textures.side, "wood_side");
        assert_ne!(wood.textures.top, wood.textures.side);
    }

    #[test]
    fn duplicate_names_rejected() {
        let mut r = BlockRegistry::empty();
        r.register(BlockDefinition::new(BlockId(0), "rock", "Rock")).unwrap();
        assert_eq!(
            r.register(BlockDefinition::new(BlockId(0), "rock", "Rock")),
            Err(RegistryError::DuplicateName("rock".to_string()))
        );
    }

    #[test]
    fn ids_are_sequential_and_overwritten() {
        let mut r = BlockRegistry::empty();
        let a = r.register(BlockDefinition::new(BlockId(77), "a", "A")).unwrap();
        let b = r.register(BlockDefinition::new(BlockId(77), "b", "B")).unwrap();
        assert_eq!(a, BlockId(0));
        assert_eq!(b, BlockId(1));
        assert_eq!(r.definition(a).unwrap().id, a, "id field must be overwritten");
        assert_eq!(r.definition(b).unwrap().id, b);
    }

    #[test]
    fn unknown_names_error() {
        let r = BlockRegistry::with_builtins();
        assert_eq!(
            r.id_of("diamond_ore").unwrap_err(),
            RegistryError::UnknownName("diamond_ore".to_string())
        );
    }
}
