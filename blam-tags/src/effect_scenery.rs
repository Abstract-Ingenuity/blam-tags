//! `.effect_scenery` (`efsc`) tag walker.
//!
//! Schema: `definitions/halo3_mcc/effect_scenery.json` →
//! `effect_scenery_struct_definition` (size 248, parent_tag `obje`).
//! Ares source: `source/objects/effect_scenery.h`.

use crate::file::TagFile;
use crate::object::ObjectDefinition;
use std::sync::Arc;

const EFFECT_SCENERY_GROUP: [u8; 4] = *b"efsc";

#[derive(Debug)]
pub enum EffectSceneryError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
    ObjectDefinition(crate::object::ObjectDefinitionError),
}

impl std::fmt::Display for EffectSceneryError {
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

impl std::error::Error for EffectSceneryError {}

impl From<crate::object::ObjectDefinitionError> for EffectSceneryError {
    fn from(e: crate::object::ObjectDefinitionError) -> Self {
        Self::ObjectDefinition(e)
    }
}

/// Walked `effect_scenery_struct_definition`. NULL compute at leaf.
/// Tag is essentially just object — schema has only the inherited
/// object substruct + a terminator.
#[derive(Debug, Clone, Default)]
pub struct EffectSceneryDefinition {
    pub object: Arc<ObjectDefinition>,
}

impl EffectSceneryDefinition {
    pub fn from_tag(tag: &TagFile) -> Result<Self, EffectSceneryError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != EFFECT_SCENERY_GROUP {
            return Err(EffectSceneryError::WrongGroup {
                expected: EFFECT_SCENERY_GROUP,
                actual,
            });
        }
        let object = Arc::new(ObjectDefinition::from_tag(tag)?);
        Ok(Self { object })
    }
}
