//! `.scenery` (`scen`) tag walker.
//!
//! Schema: `definitions/halo3_mcc/scenery.json` → `scenery_group`
//! (size 256, parent_tag `obje`).
//! Ares source: `source/objects/scenery.h`.

use crate::file::TagFile;
use crate::object::ObjectDefinition;
use std::sync::Arc;

const SCENERY_GROUP: [u8; 4] = *b"scen";

#[derive(Debug)]
pub enum SceneryError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
    ObjectDefinition(crate::object::ObjectDefinitionError),
}

impl std::fmt::Display for SceneryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongGroup { expected, actual } => write!(
                f,
                "expected group '{}', got '{}'",
                std::str::from_utf8(expected).unwrap_or("?"),
                std::str::from_utf8(actual).unwrap_or("?"),
            ),
            Self::ObjectDefinition(e) => write!(f, "object substruct: {e}"),
        }
    }
}

impl std::error::Error for SceneryError {}

impl From<crate::object::ObjectDefinitionError> for SceneryError {
    fn from(e: crate::object::ObjectDefinitionError) -> Self {
        Self::ObjectDefinition(e)
    }
}

/// Walked subset of `scenery_group` (size 256). Scenery has no
/// compute_function_value at the leaf (chain reduces to `[object]`),
/// so this surface is purely for consumers that read pathfinding /
/// lightmapping authoring policies.
#[derive(Debug, Clone, Default)]
pub struct SceneryDefinition {
    pub object: Arc<ObjectDefinition>,
    /// `pathfinding policy` enum.
    pub pathfinding_policy: i16,
    /// `flags` (word_flags).
    pub flags: u16,
    /// `lightmapping policy` enum.
    pub lightmapping_policy: i16,
}

impl SceneryDefinition {
    pub fn from_tag(tag: &TagFile) -> Result<Self, SceneryError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != SCENERY_GROUP {
            return Err(SceneryError::WrongGroup { expected: SCENERY_GROUP, actual });
        }
        let object = Arc::new(ObjectDefinition::from_tag(tag)?);
        let root = tag.root();
        Ok(Self {
            object,
            pathfinding_policy: root.read_int_any("pathfinding policy").unwrap_or(0) as i16,
            flags: root.read_int_any("flags").unwrap_or(0) as u16,
            lightmapping_policy: root.read_int_any("lightmapping policy").unwrap_or(0) as i16,
        })
    }
}
