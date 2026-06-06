//! `.equipment` (`eqip`) tag walker. Surfaces the authored fields the
//! runtime `equipment_compute_function_value` reads from
//! `equipment_definition` / `item_struct_definition`.
//!
//! Schema reference:
//! - `definitions/halo3_mcc/equipment.json` → `equipment_group`
//!   (size 620, parent_tag `item`).
//! - `definitions/halo3_mcc/item.json` (already walked by
//!   [`crate::item::ItemDefinition`]).
//!
//! Composition: weapon-side mirror — the eqip tag's root holds an
//! `item` substruct → `object` substruct chain. Equipment-specific
//! fields plus 10 mutually-exclusive equipment-type sub-blocks
//! (super_shield, multiplayer_powerup, spawner, proximity_mine,
//! motion_tracker_noise, showme, invisibility_mode, invincibility,
//! tree_of_life, health_pack) make up the rest. The
//! `equipment_definition_has_type(def, idx)` predicate in
//! `equipment_compute_function_value` maps to "this type's block has
//! at least one element".

use crate::api::TagStruct;
use crate::file::TagFile;
use crate::item::ItemDefinition;
use crate::object::ObjectDefinition;
use crate::typed_enums::{Enum, Flags};
use std::sync::Arc;

const EQUIPMENT_GROUP: [u8; 4] = *b"eqip";

/// `equipment_flags` (word_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u16)]
pub enum EquipmentFlags {
    #[strum(serialize = "pathfinding obstacle")] PathfindingObstacle = 0,
    #[strum(serialize = "gravity lift collision group")] GravityLiftCollisionGroup = 1,
    #[strum(serialize = "equipment is dangerous to ai")] EquipmentIsDangerousToAi = 2,
    #[strum(serialize = "protects parent from AOE")] ProtectsParentFromAoe = 3,
    #[strum(serialize = "never dropped by ai")] NeverDroppedByAi = 4,
}

/// `equipment_spawner_spawn_type` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum EquipmentSpawnerSpawnType {
    #[default]
    #[strum(serialize = "along aiming vector")] AlongAimingVector = 0,
    #[strum(serialize = "camera pos z plane")] CameraPosZPlane = 1,
    #[strum(serialize = "foot pos z plane")] FootPosZPlane = 2,
}

/// `multiplayer_powerup_flavor` (long_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i32)]
pub enum MultiplayerPowerupFlavor {
    #[default]
    #[strum(serialize = "red powerup")] Red = 0,
    #[strum(serialize = "blue powerup")] Blue = 1,
    #[strum(serialize = "yellow powerup")] Yellow = 2,
}

/// `equipment_type` discriminant — engine assert range is `[0, 8]`
/// per `equipment_definition_has_type`. Block declaration order in
/// `equipment_group` matches these indices.
pub const EQUIPMENT_TYPE_SUPER_SHIELD:          u32 = 0;
pub const EQUIPMENT_TYPE_MULTIPLAYER_POWERUP:   u32 = 1;
pub const EQUIPMENT_TYPE_SPAWNER:               u32 = 2;
pub const EQUIPMENT_TYPE_PROXIMITY_MINE:        u32 = 3;
pub const EQUIPMENT_TYPE_MOTION_TRACKER_NOISE:  u32 = 4;
pub const EQUIPMENT_TYPE_SHOWME:                u32 = 5;
pub const EQUIPMENT_TYPE_INVISIBILITY_MODE:     u32 = 6;
pub const EQUIPMENT_TYPE_INVINCIBILITY:         u32 = 7;
pub const EQUIPMENT_TYPE_TREE_OF_LIFE:          u32 = 8;
// NB: health_pack is the 10th block but the engine assert caps at 8;
// it's accessed by a different code path.

#[derive(Debug)]
pub enum EquipmentError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
    MissingSubstruct { path: &'static str },
    ObjectDefinition(crate::object::ObjectDefinitionError),
}

impl std::fmt::Display for EquipmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongGroup { expected, actual } => write!(
                f,
                "expected group '{}', got '{}'",
                std::str::from_utf8(expected).unwrap_or("?"),
                std::str::from_utf8(actual).unwrap_or("?"),
            ),
            Self::MissingSubstruct { path } => {
                write!(f, "tag missing required substruct '{path}'")
            }
            Self::ObjectDefinition(e) => write!(f, "object substruct: {e}"),
        }
    }
}

impl std::error::Error for EquipmentError {}

impl From<crate::object::ObjectDefinitionError> for EquipmentError {
    fn from(e: crate::object::ObjectDefinitionError) -> Self {
        Self::ObjectDefinition(e)
    }
}

// ---------------------------------------------------------------------------
// EquipmentDefinition — `equipment_group`
// ---------------------------------------------------------------------------

/// Walked `equipment_group` (Ares `source/items/equipment.h`
/// definition tag struct, size 620). Field set covers everything
/// `equipment_compute_function_value` reads (the type-discriminator
/// blocks for `equipment_definition_has_type` + proximity_mine fuse).
///
/// The 10 `*_blocks` Vecs each represent ONE of the
/// mutually-exclusive equipment type categories. A non-empty Vec
/// means "this equipment IS that type" — engine
/// `equipment_definition_has_type` returns `count != 0`.
#[derive(Debug, Clone, Default)]
pub struct EquipmentDefinition {
    /// Inherited item substruct (and its embedded object substruct).
    pub item: Arc<ItemDefinition>,

    /// `duration` (real) — equipment-wide lifetime; semantics vary
    /// per type.
    pub duration: f32,
    /// `phantom volume activation time` (real).
    pub phantom_volume_activation_time: f32,
    /// `charges` (short_integer). -1 = unlimited, 0 = fire on creation.
    pub charges: i16,
    /// `flags` (word_flags).
    pub flags: Flags<EquipmentFlags, u16>,

    /// `danger radius` (real).
    pub danger_radius: f32,
    /// `min deployment distance:wus` (real).
    pub min_deployment_distance: f32,
    /// `awareness time:seconds` (real).
    pub awareness_time: f32,

    // -- type-discriminator sub-blocks (max-1 element each, but stored
    // as Vec so .len() gives the engine's "count" predicate) --
    pub super_shield_blocks:          Vec<EquipmentTypeSuperShield>,
    pub multiplayer_powerup_blocks:   Vec<EquipmentTypeMultiplayerPowerup>,
    pub spawner_blocks:               Vec<EquipmentTypeSpawner>,
    pub proximity_mine_blocks:        Vec<EquipmentTypeProximityMine>,
    pub motion_tracker_noise_blocks:  Vec<EquipmentTypeMotionTrackerNoise>,
    pub showme_blocks:                Vec<EquipmentTypeShowme>,
    pub invisibility_mode_blocks:     Vec<EquipmentTypeInvisibilityMode>,
    pub invincibility_blocks:         Vec<EquipmentTypeInvincibility>,
    pub tree_of_life_blocks:          Vec<EquipmentTypeTreeOfLife>,
    pub health_pack_blocks:           Vec<EquipmentTypeHealthPack>,
}

impl EquipmentDefinition {
    /// Engine `equipment_definition_has_type(def, equipment_type)`.
    /// Returns true when the type's sub-block has at least one entry.
    /// Indices 0..=8 mirror the `EQUIPMENT_TYPE_*` constants.
    pub fn has_type(&self, equipment_type: u32) -> bool {
        match equipment_type {
            EQUIPMENT_TYPE_SUPER_SHIELD          => !self.super_shield_blocks.is_empty(),
            EQUIPMENT_TYPE_MULTIPLAYER_POWERUP   => !self.multiplayer_powerup_blocks.is_empty(),
            EQUIPMENT_TYPE_SPAWNER               => !self.spawner_blocks.is_empty(),
            EQUIPMENT_TYPE_PROXIMITY_MINE        => !self.proximity_mine_blocks.is_empty(),
            EQUIPMENT_TYPE_MOTION_TRACKER_NOISE  => !self.motion_tracker_noise_blocks.is_empty(),
            EQUIPMENT_TYPE_SHOWME                => !self.showme_blocks.is_empty(),
            EQUIPMENT_TYPE_INVISIBILITY_MODE     => !self.invisibility_mode_blocks.is_empty(),
            EQUIPMENT_TYPE_INVINCIBILITY         => !self.invincibility_blocks.is_empty(),
            EQUIPMENT_TYPE_TREE_OF_LIFE          => !self.tree_of_life_blocks.is_empty(),
            _ => false,
        }
    }

    pub fn from_tag(tag: &TagFile) -> Result<Self, EquipmentError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != EQUIPMENT_GROUP {
            return Err(EquipmentError::WrongGroup {
                expected: EQUIPMENT_GROUP,
                actual,
            });
        }
        let object = Arc::new(ObjectDefinition::from_tag(tag)?);
        let root = tag.root();
        let item_struct = root
            .descend("item")
            .ok_or(EquipmentError::MissingSubstruct { path: "item" })?;
        let item = Arc::new(ItemDefinition::from_item_struct(object, &item_struct));

        Ok(Self {
            item,
            duration: root.read_real("duration").unwrap_or(0.0),
            phantom_volume_activation_time: root
                .read_real("phantom volume activation time")
                .unwrap_or(0.0),
            charges: root.read_int_any("charges").unwrap_or(0) as i16,
            flags: root.try_read_flags("flags").unwrap_or_default(),
            danger_radius: root.read_real("danger radius").unwrap_or(0.0),
            min_deployment_distance: root.read_real("min deployment distance").unwrap_or(0.0),
            awareness_time: root.read_real("awareness time").unwrap_or(0.0),
            super_shield_blocks: read_block(&root, "super shield",
                EquipmentTypeSuperShield::from_struct),
            multiplayer_powerup_blocks: read_block(&root, "multiplayer powerup",
                EquipmentTypeMultiplayerPowerup::from_struct),
            spawner_blocks: read_block(&root, "spawner",
                EquipmentTypeSpawner::from_struct),
            // NB: schema typo — block is named `moition tracker noise`
            // (sic) in equipment.json:127. Match the on-disk spelling.
            proximity_mine_blocks: read_block(&root, "proximity mine",
                EquipmentTypeProximityMine::from_struct),
            motion_tracker_noise_blocks: read_block(&root, "moition tracker noise",
                EquipmentTypeMotionTrackerNoise::from_struct),
            showme_blocks: read_block(&root, "showme",
                EquipmentTypeShowme::from_struct),
            invisibility_mode_blocks: read_block(&root, "invisibility mode",
                EquipmentTypeInvisibilityMode::from_struct),
            invincibility_blocks: read_block(&root, "invincibility mode",
                EquipmentTypeInvincibility::from_struct),
            tree_of_life_blocks: read_block(&root, "tree of life",
                EquipmentTypeTreeOfLife::from_struct),
            health_pack_blocks: read_block(&root, "health pack",
                EquipmentTypeHealthPack::from_struct),
        })
    }
}

fn read_block<T, F>(s: &TagStruct<'_>, name: &str, mut f: F) -> Vec<T>
where
    F: FnMut(&TagStruct<'_>) -> T,
{
    s.field(name)
        .and_then(|f| f.as_block())
        .map(|block| block.iter().map(|e| f(&e)).collect::<Vec<_>>())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Per-type sub-blocks
// ---------------------------------------------------------------------------

/// `equipment_type_super_shield_block` (60 bytes). Scales engine
/// shield recharge fields and attaches 3 effect tag-refs.
#[derive(Debug, Clone, Default)]
pub struct EquipmentTypeSuperShield {
    /// `shield recharge delay scale`. 0 = engine default 1.0.
    pub shield_recharge_delay_scale: f32,
    /// `shield recharge rate scale`. 0 = engine default 1.0.
    pub shield_recharge_rate_scale: f32,
    /// `shield ceiling scale`. 0 = engine default 1.0.
    pub shield_ceiling_scale: f32,
    pub shield_effect: String,
    pub overcharge_effect: String,
    pub overcharge_damage_effect: String,
}

impl EquipmentTypeSuperShield {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            shield_recharge_delay_scale: s.read_real("shield recharge delay scale").unwrap_or(0.0),
            shield_recharge_rate_scale: s.read_real("shield recharge rate scale").unwrap_or(0.0),
            shield_ceiling_scale: s.read_real("shield ceiling scale").unwrap_or(0.0),
            shield_effect: s.read_tag_ref_path("shield effect").unwrap_or_default(),
            overcharge_effect: s.read_tag_ref_path("overcharge effect").unwrap_or_default(),
            overcharge_damage_effect: s
                .read_tag_ref_path("overcharge damage effect")
                .unwrap_or_default(),
        }
    }
}

/// `equipment_type_multiplayer_powerup_block` (4 bytes).
#[derive(Debug, Clone, Default)]
pub struct EquipmentTypeMultiplayerPowerup {
    /// `flavor` (long_enum) — which MP powerup color this spawns.
    pub flavor: Enum<MultiplayerPowerupFlavor, i32>,
}

impl EquipmentTypeMultiplayerPowerup {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            flavor: s.read_enum("flavor"),
        }
    }
}

/// `equipment_type_spawner_block` (52 bytes).
#[derive(Debug, Clone, Default)]
pub struct EquipmentTypeSpawner {
    pub spawned_object: String,
    pub spawned_effect: String,
    pub spawn_radius: f32,
    pub spawn_z_offset: f32,
    pub spawn_area_radius: f32,
    /// `spawn velocity:WU/sec`.
    pub spawn_velocity: f32,
    /// `type` (short_enum `equipment_spawner_spawn_type`).
    pub spawn_type: Enum<EquipmentSpawnerSpawnType, i16>,
}

impl EquipmentTypeSpawner {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            spawned_object: s.read_tag_ref_path("spawned object").unwrap_or_default(),
            spawned_effect: s.read_tag_ref_path("spawned effect").unwrap_or_default(),
            spawn_radius: s.read_real("spawn radius").unwrap_or(0.0),
            spawn_z_offset: s.read_real("spawn z offset").unwrap_or(0.0),
            spawn_area_radius: s.read_real("spawn area radius").unwrap_or(0.0),
            spawn_velocity: s.read_real("spawn velocity").unwrap_or(0.0),
            spawn_type: s.read_enum("type"),
        }
    }
}

/// `equipment_type_proximity_mine_block` (48 bytes). Read by the
/// `death` compute case for the fuse-countdown readout.
#[derive(Debug, Clone, Default)]
pub struct EquipmentTypeProximityMine {
    /// `arm time:seconds` — delay before the mine becomes active.
    pub arm_time: f32,
    /// `self destruct time:seconds` — auto-detonation timer once
    /// armed. 0 means never. The `death` compute case uses this as
    /// the fuse for its `1.0 - (self_destruct - elapsed)/5` readout.
    pub self_destruct_time: f32,
    /// `trigger time:seconds`.
    pub trigger_time: f32,
    /// `trigger velocity:WU/sec`.
    pub trigger_velocity: f32,
}

impl EquipmentTypeProximityMine {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            arm_time: s.read_real("arm time").unwrap_or(0.0),
            self_destruct_time: s.read_real("self destruct time").unwrap_or(0.0),
            trigger_time: s.read_real("trigger time").unwrap_or(0.0),
            trigger_velocity: s.read_real("trigger velocity").unwrap_or(0.0),
        }
    }
}

/// `equipment_type_motion_tracker_noise_block` (16 bytes).
#[derive(Debug, Clone, Default)]
pub struct EquipmentTypeMotionTrackerNoise {
    pub motion_tracker_noise_radius: f32,
    pub motion_tracker_noise_period: f32,
    pub motion_tracker_blip_height: f32,
    pub motion_tracker_blip_intensity: f32,
}

impl EquipmentTypeMotionTrackerNoise {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            motion_tracker_noise_radius: s
                .read_real("motion tracker noise radius")
                .unwrap_or(0.0),
            motion_tracker_noise_period: s
                .read_real("motion tracker noise period")
                .unwrap_or(0.0),
            motion_tracker_blip_height: s
                .read_real("motion tracker blip height")
                .unwrap_or(0.0),
            motion_tracker_blip_intensity: s
                .read_real("motion tracker blip intensity")
                .unwrap_or(0.0),
        }
    }
}

/// `equipment_type_showme_block` (4 bytes).
#[derive(Debug, Clone, Default)]
pub struct EquipmentTypeShowme {
    pub flags: u32,
}

impl EquipmentTypeShowme {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            flags: s.read_int_any("flags").unwrap_or(0) as u32,
        }
    }
}

/// `equipment_type_invisibility_mode_block` (8 bytes).
#[derive(Debug, Clone, Default)]
pub struct EquipmentTypeInvisibilityMode {
    pub camo_amount: f32,
    pub fade_in_time: f32,
}

impl EquipmentTypeInvisibilityMode {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            camo_amount: s.read_real("camo amount").unwrap_or(0.0),
            fade_in_time: s.read_real("fade in time").unwrap_or(0.0),
        }
    }
}

/// `equipment_type_invincibility_block` (60 bytes).
#[derive(Debug, Clone, Default)]
pub struct EquipmentTypeInvincibility {
    pub invincibility_material: String,
    /// `invincibility material type*!` — runtime material index.
    pub invincibility_material_type: i16,
    pub shield_recharge_time: f32,
    pub activation_effect: String,
    pub attached_effect: String,
    pub shutdown_effect: String,
}

impl EquipmentTypeInvincibility {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            invincibility_material: s
                .read_string_id("invincibility material")
                .unwrap_or_default(),
            invincibility_material_type: s
                .read_int_any("invincibility material type")
                .unwrap_or(0) as i16,
            shield_recharge_time: s.read_real("shield recharge time").unwrap_or(0.0),
            activation_effect: s.read_tag_ref_path("activation effect").unwrap_or_default(),
            attached_effect: s.read_tag_ref_path("attached effect").unwrap_or_default(),
            shutdown_effect: s.read_tag_ref_path("shutdown effect").unwrap_or_default(),
        }
    }
}

/// `equipment_type_treeoflife_block` (4 bytes).
#[derive(Debug, Clone, Default)]
pub struct EquipmentTypeTreeOfLife {
    pub flags: u32,
}

impl EquipmentTypeTreeOfLife {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            flags: s.read_int_any("flags").unwrap_or(0) as u32,
        }
    }
}

/// `equipment_type_health_pack_block` (8 bytes).
#[derive(Debug, Clone, Default)]
pub struct EquipmentTypeHealthPack {
    pub health_to_restore: f32,
    pub restore_time: f32,
}

impl EquipmentTypeHealthPack {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            health_to_restore: s.read_real("health to restore").unwrap_or(0.0),
            restore_time: s.read_real("restore time").unwrap_or(0.0),
        }
    }
}
