//! `.crate` (`bloc`) tag walker. NB: file extension is `.crate` but
//! FOURCC is `bloc` per `crate.json:3`. Module is named
//! `crate_definition` (not `crate`) to avoid the Rust keyword.
//!
//! Schema: `definitions/halo3_mcc/crate.json` →
//! `crate_struct_definition` (size 268, parent_tag `obje`).
//! Ares source: `source/objects/crates.h`.

use crate::file::TagFile;
use crate::object::ObjectDefinition;
use crate::typed_enums::Flags;
use std::sync::Arc;

const CRATE_GROUP: [u8; 4] = *b"bloc";

/// `crate_flags` (word_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u16)]
pub enum CrateFlags {
    #[strum(serialize = "does not block AOE")] DoesNotBlockAoe = 0,
    #[strum(serialize = "attach texture camera hack")] AttachTextureCameraHack = 1,
}

#[derive(Debug)]
pub enum CrateError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
    ObjectDefinition(crate::object::ObjectDefinitionError),
}

impl std::fmt::Display for CrateError {
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

impl std::error::Error for CrateError {}

impl From<crate::object::ObjectDefinitionError> for CrateError {
    fn from(e: crate::object::ObjectDefinitionError) -> Self {
        Self::ObjectDefinition(e)
    }
}

/// Walked `crate_struct_definition` (size 268). Field order matches
/// schema verbatim. Crate has no compute_function_value at the leaf
/// (engine returns 0) — surface is structural.
#[derive(Debug, Clone, Default)]
pub struct CrateDefinition {
    pub object: Arc<ObjectDefinition>,
    /// `flags` (word_flags).
    pub flags: Flags<CrateFlags, u16>,
    /// `campaign metagame bucket` block — count only.
    pub campaign_metagame_bucket_count: usize,
    /// `self destruction timer:seconds`.
    pub self_destruction_timer: i32,
}

impl CrateDefinition {
    pub fn from_tag(tag: &TagFile) -> Result<Self, CrateError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != CRATE_GROUP {
            return Err(CrateError::WrongGroup { expected: CRATE_GROUP, actual });
        }
        let object = Arc::new(ObjectDefinition::from_tag(tag)?);
        let root = tag.root();
        let campaign_metagame_bucket_count = root
            .field("campaign metagame bucket")
            .and_then(|f| f.as_block())
            .map(|b| b.len())
            .unwrap_or(0);
        Ok(Self {
            object,
            flags: root.try_read_flags("flags").unwrap_or_default(),
            campaign_metagame_bucket_count,
            self_destruction_timer: root.read_int_any("self destruction timer").unwrap_or(0) as i32,
        })
    }
}
