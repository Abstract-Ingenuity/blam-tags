//! `.sound_scenery` (`ssce`) tag walker.
//!
//! Schema: `definitions/halo3_mcc/sound_scenery.json` →
//! `sound_scenery_struct_definition` (size 264, parent_tag `obje`).
//! Ares source: `source/objects/sound_scenery.h`.

use crate::file::TagFile;
use crate::object::ObjectDefinition;
use std::sync::Arc;

const SOUND_SCENERY_GROUP: [u8; 4] = *b"ssce";

#[derive(Debug)]
pub enum SoundSceneryError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
    ObjectDefinition(crate::object::ObjectDefinitionError),
}

impl std::fmt::Display for SoundSceneryError {
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

impl std::error::Error for SoundSceneryError {}

impl From<crate::object::ObjectDefinitionError> for SoundSceneryError {
    fn from(e: crate::object::ObjectDefinitionError) -> Self {
        Self::ObjectDefinition(e)
    }
}

/// Walked `sound_scenery_struct_definition`. NULL compute at leaf
/// (engine `part_definitions[]` chain reduces to `[object]`).
#[derive(Debug, Clone, Default)]
pub struct SoundSceneryDefinition {
    pub object: Arc<ObjectDefinition>,
}

impl SoundSceneryDefinition {
    pub fn from_tag(tag: &TagFile) -> Result<Self, SoundSceneryError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != SOUND_SCENERY_GROUP {
            return Err(SoundSceneryError::WrongGroup {
                expected: SOUND_SCENERY_GROUP,
                actual,
            });
        }
        let object = Arc::new(ObjectDefinition::from_tag(tag)?);
        Ok(Self { object })
    }
}
