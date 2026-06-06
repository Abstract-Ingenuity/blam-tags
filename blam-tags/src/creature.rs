//! `.creature` (`crea`) tag walker.
//!
//! Schema: `definitions/halo3_mcc/creature.json` →
//! `creature_struct_definition` (size 504, parent_tag `obje`).
//! Ares source: `source/creatures/creatures.h`.

use crate::biped::BipedPhysics;
use crate::file::TagFile;
use crate::math::Bounds;
use crate::object::ObjectDefinition;
use crate::typed_enums::{Enum, Flags};
use crate::unit::{GlobalChudBlipType, UnitDefaultTeam};
use std::sync::Arc;

const CREATURE_GROUP: [u8; 4] = *b"crea";

/// `creature_definition_flags` (long_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum CreatureDefinitionFlags {
    #[strum(serialize = "unused")] Unused = 0,
    #[strum(serialize = "infection form")] InfectionForm = 1,
    #[strum(serialize = "immune to falling damage")] ImmuneToFallingDamage = 2,
    #[strum(serialize = "rotate while airborne")] RotateWhileAirborne = 3,
    #[strum(serialize = "zapped by shields")] ZappedByShields = 4,
    #[strum(serialize = "attach upon impact")] AttachUponImpact = 5,
    #[strum(serialize = "not on motion sensor")] NotOnMotionSensor = 6,
}

#[derive(Debug)]
pub enum CreatureError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
    ObjectDefinition(crate::object::ObjectDefinitionError),
}

impl std::fmt::Display for CreatureError {
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

impl std::error::Error for CreatureError {}

impl From<crate::object::ObjectDefinitionError> for CreatureError {
    fn from(e: crate::object::ObjectDefinitionError) -> Self {
        Self::ObjectDefinition(e)
    }
}

/// Walked `creature_struct_definition` (size 504). Field order
/// matches schema verbatim. Shares `character_physics_struct` with
/// `BipedDefinition` via the reused `BipedPhysics` walker.
#[derive(Debug, Clone, Default)]
pub struct CreatureDefinition {
    pub object: Arc<ObjectDefinition>,
    pub flags: Flags<CreatureDefinitionFlags, u32>,
    pub default_team: Enum<UnitDefaultTeam, i16>,
    pub motion_sensor_blip_size: Enum<GlobalChudBlipType, i16>,
    /// `turning velocity maximum:degrees per second` (radians on disk).
    pub turning_velocity_maximum: f32,
    /// `turning acceleration maximum:degrees per second squared`.
    pub turning_acceleration_maximum: f32,
    /// `casual turning modifier:[0,1]`.
    pub casual_turning_modifier: f32,
    /// `autoaim width:world units`.
    pub autoaim_width: f32,
    /// `physics` — `character_physics_struct`, shared with bipeds.
    pub physics: BipedPhysics,
    pub impact_damage: String,
    /// `impact shield damage` — falls back to `impact damage` if unset.
    pub impact_shield_damage: String,
    /// `campaign metagame bucket` block — count only.
    pub campaign_metagame_bucket_count: usize,
    /// `destroy after death time:seconds` (real_bounds).
    pub destroy_after_death_time: Bounds<f32>,
}

impl CreatureDefinition {
    pub fn from_tag(tag: &TagFile) -> Result<Self, CreatureError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != CREATURE_GROUP {
            return Err(CreatureError::WrongGroup { expected: CREATURE_GROUP, actual });
        }
        let object = Arc::new(ObjectDefinition::from_tag(tag)?);
        let root = tag.root();
        let campaign_metagame_bucket_count = root
            .field("campaign metagame bucket")
            .and_then(|f| f.as_block())
            .map(|b| b.len())
            .unwrap_or(0);
        let physics = root
            .field("physics")
            .and_then(|f| f.as_struct())
            .map(|s| BipedPhysics::from_struct(&s))
            .unwrap_or_default();
        Ok(Self {
            object,
            flags: root.try_read_flags("flags").unwrap_or_default(),
            default_team: root.read_enum("default team"),
            motion_sensor_blip_size: root.read_enum("motion sensor blip size"),
            turning_velocity_maximum: root.read_real("turning velocity maximum").unwrap_or(0.0),
            turning_acceleration_maximum: root
                .read_real("turning acceleration maximum")
                .unwrap_or(0.0),
            casual_turning_modifier: root.read_real("casual turning modifier").unwrap_or(0.0),
            autoaim_width: root.read_real("autoaim width").unwrap_or(0.0),
            physics,
            impact_damage: root.read_tag_ref_path("impact damage").unwrap_or_default(),
            impact_shield_damage: root
                .read_tag_ref_path("impact shield damage")
                .unwrap_or_default(),
            campaign_metagame_bucket_count,
            destroy_after_death_time: root.read_real_bounds("destroy after death time"),
        })
    }
}
