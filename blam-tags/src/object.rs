//! `object_definition` common substruct — the `object` block shared by
//! all 14 object subgroups (.scenery, .crate, .weapon, .biped, .vehicle,
//! .equipment, .device_machine, .device_control, .device_terminal,
//! .projectile, .creature, .giant, .effect_scenery, .sound_scenery).
//! Each subgroup's tag file embeds this same `object_struct_definition`
//! at a path that depends on its inheritance chain:
//!   `.object` (obje, abstract base) → `` (the tag root)
//!   `.scenery / .crate / .sound_scenery / .creature / .projectile / .effect_scenery / .giant`
//!                                      → `object`
//!   `.biped / .vehicle`                → `unit/object`
//!   `.weapon`                          → `weapon/item/object`
//!   `.equipment`                       → `item/object`
//!   `.device_machine / .device_control / .device_terminal` → `device/object`
//!
//! Schema reference: `definitions/halo3_mcc/object.json`
//! → `object_struct_definition` (size 248, guid
//! `6c5aa9947a45fcf55742a488f0943380`). The field set below mirrors the
//! schema's field order with runtime-only fields
//! (`runtime object type!`, `runtime flags!*`) and not-yet-consumed
//! sub-blocks (`early mover OBB`, `ai properties`, `attachments`,
//! `widgets`, `change colors`, `multiplayer object`, `health packs`)
//! omitted — add them as consumers come online.
//!
//! Drives two runtime paths:
//!
//! 1. **`object_get_function_value @ dllcache 0x1807DBA60`** — when a
//!    render-method asks for an input by name (e.g. `bar` on
//!    `marinebeacon.scenery`) and the type-specific
//!    `<type>_compute_function_value` returns false, the engine walks
//!    `functions[]` looking for an entry whose `export_name` matches
//!    the requested name, then evaluates that entry's curve via
//!    `object_function_get_function_value @ 0x1807E85B0`.
//!
//! 2. **`object_get_bounding_sphere @ 0x1802473A0`** — reads
//!    `(bounding_offset, bounding_radius)` and transforms by the
//!    object's runtime matrix for cull/shadow/lights bookkeeping.

use crate::api::TagStruct;
use crate::file::TagFile;
use crate::math::{RealEulerAngles3d, RealPoint3d, RealRgbColor};
use crate::tag_function::TagFunction;
use crate::typed_enums::{Enum, Flags};

/// All 14 object subgroups that share the `object_struct_definition`.
/// Any tag in this set has a valid `object` substruct (under the
/// inheritance-chain prefix returned by [`OBJECT_INHERITANCE_PREFIXES`]).
pub const OBJECT_SUBGROUPS: &[[u8; 4]] = &[
    *b"obje", // object_definition (abstract base)
    *b"scen", // scenery
    *b"bipd", // biped
    *b"vehi", // vehicle
    *b"weap", // weapon
    *b"eqip", // equipment
    *b"ssce", // sound_scenery
    *b"bloc", // crate (.crate extension → `bloc` FOURCC per crate.json:3)
    *b"mach", // device_machine
    *b"ctrl", // device_control
    *b"term", // device_terminal
    *b"proj", // projectile
    *b"crea", // creature
    *b"gint", // giant
    *b"efsc", // effect_scenery
];

/// Inheritance-chain prefixes that wrap the `object_struct_definition`
/// in each subgroup's tag layout. Ordered deepest-first so the more
/// specific path wins when the same struct-name collides at multiple
/// levels (it shouldn't in practice, but cheap insurance).
const OBJECT_INHERITANCE_PREFIXES: &[&str] = &[
    "weapon/item/object", // .weapon
    "item/object",        // .equipment
    "unit/object",        // .biped / .vehicle / .giant
    "device/object",      // .device_machine / .device_control / .device_terminal
    "object",             // .scenery / .crate / .sound_scenery / .creature / .projectile / .effect_scenery
    "",                   // .object itself (abstract base)
];

/// Errors from `object_definition` tag walking.
#[derive(Debug)]
pub enum ObjectDefinitionError {
    WrongGroup { actual: [u8; 4] },
}

impl std::fmt::Display for ObjectDefinitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongGroup { actual } => write!(
                f,
                "tag group '{}' is not in OBJECT_SUBGROUPS — not an object_definition tag",
                std::str::from_utf8(actual).unwrap_or("?"),
            ),
        }
    }
}

impl std::error::Error for ObjectDefinitionError {}

// ---------------------------------------------------------------------------
// object_definition_flags
// ---------------------------------------------------------------------------

/// `object_definition_flags` (word_flags). Bit 0 `DoesNotCastShadow`
/// drives `render_object_has_lightmap_shadow @ 0x180696EE0`; bit 1
/// `SearchCardinalDirectionLightmapsOnFailure` (engine
/// `_object_searches_lightmaps_on_failure_bit`) selects the 9-ray
/// sideways lightprobe branch in `lights_prepare_for_object_static_new`.
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u16)]
pub enum ObjectDefinitionFlags {
    #[strum(serialize = "does not cast shadow")] DoesNotCastShadow = 0,
    #[strum(serialize = "search cardinal direction lightmaps on failure")] SearchCardinalDirectionLightmapsOnFailure = 1,
    #[strum(serialize = "preserves initial damage owner")] PreservesInitialDamageOwner = 2,
    #[strum(serialize = "not a pathfinding obstacle")] NotAPathfindingObstacle = 3,
    #[strum(serialize = "extension of parent")] ExtensionOfParent = 4,
    #[strum(serialize = "does not cause collision damage")] DoesNotCauseCollisionDamage = 5,
    #[strum(serialize = "early mover")] EarlyMover = 6,
    #[strum(serialize = "early mover localized physics")] EarlyMoverLocalizedPhysics = 7,
    #[strum(serialize = "use static massive lightmap sample")] UseStaticMassiveLightmapSample = 8,
    #[strum(serialize = "object scales attachments")] ObjectScalesAttachments = 9,
    #[strum(serialize = "non physical in map editor")] NonPhysicalInMapEditor = 10,
    #[strum(serialize = "attach to clusters by dynamic sphere")] AttachToClustersByDynamicSphere = 11,
    #[strum(serialize = "effects created by this object do not spawn objects in multiplayer")] EffectsDoNotSpawnObjectsInMultiplayer = 12,
    #[strum(serialize = "does not collide with camera")] DoesNotCollideWithCamera = 13,
    #[strum(serialize = "damage not blocked by obstructions")] DamageNotBlockedByObstructions = 14,
}

/// `object_definition_secondary_flags` (word_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u16)]
pub enum ObjectDefinitionSecondaryFlags {
    #[strum(serialize = "does not affect projectile aiming")] DoesNotAffectProjectileAiming = 0,
}

/// `lightmap_shadow_mode_enum` (short_enum). Gate in
/// `render_object_has_lightmap_shadow`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum LightmapShadowMode {
    #[default]
    #[strum(serialize = "default")] Default = 0,
    #[strum(serialize = "never")] Never = 1,
    #[strum(serialize = "always")] Always = 2,
    #[strum(serialize = "blur")] Blur = 3,
}

/// `sweetener_size_enum` (char_enum) — sound-system field.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i8)]
pub enum SweetenerSize {
    #[default]
    #[strum(serialize = "default")] Default = 0,
    #[strum(serialize = "small")] Small = 1,
    #[strum(serialize = "medium")] Medium = 2,
    #[strum(serialize = "large")] Large = 3,
}

/// `water_density_type_enum` (char_enum) — physics field.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i8)]
pub enum WaterDensityType {
    #[default]
    #[strum(serialize = "default")] Default = 0,
    #[strum(serialize = "super_floater")] SuperFloater = 1,
    #[strum(serialize = "floater")] Floater = 2,
    #[strum(serialize = "neutral")] Neutral = 3,
    #[strum(serialize = "sinker")] Sinker = 4,
    #[strum(serialize = "super_sinker")] SuperSinker = 5,
    #[strum(serialize = "none")] None = 6,
}

// ---------------------------------------------------------------------------
// object_function_flags
// ---------------------------------------------------------------------------

/// `object_function_flags` (long_flags). Variant docs note the engine
/// behaviour the protomorph evaluator keys off each bit.
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum ObjectFunctionFlags {
    /// Bit 0 — invert the resolved magnitude (`*m = 1.0 - *m`).
    #[strum(serialize = "invert")] Invert = 0,
    /// Bit 1 — the curve mapping can make the function active/inactive
    /// (protomorph treats CLEAR as "additive": always evaluate).
    #[strum(serialize = "mapping does not controls active")] MappingDoesNotControlsActive = 1,
    /// Bit 2 — force `result = 1` regardless of curve output.
    #[strum(serialize = "always active")] AlwaysActive = 2,
    /// Bit 3 — periodic eval adds a per-object random time offset.
    #[strum(serialize = "random time offset")] RandomTimeOffset = 3,
    /// Bit 4 — emit the curve magnitude even when the entry is inactive.
    #[strum(serialize = "always exports value")] AlwaysExportsValue = 4,
    /// Bit 5 — `turn_off_with` additionally requires non-zero magnitude.
    #[strum(serialize = "turn off with uses magnitude")] TurnOffWithUsesMagnitude = 5,
}

// ---------------------------------------------------------------------------
// AI properties / attachment / change-color / multiplayer enums
// ---------------------------------------------------------------------------

/// `ai_properties_flags` (long_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum AiPropertiesFlags {
    #[strum(serialize = "detroyable cover")] DetroyableCover = 0,
    #[strum(serialize = "pathfinding ignore when dead")] PathfindingIgnoreWhenDead = 1,
    #[strum(serialize = "dynamic cover")] DynamicCover = 2,
    #[strum(serialize = "non flight-blocking")] NonFlightBlocking = 3,
    #[strum(serialize = "dynamic cover from centre")] DynamicCoverFromCentre = 4,
    #[strum(serialize = "has corner markers")] HasCornerMarkers = 5,
}

/// `ai_size_enum` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum AiSize {
    #[default]
    #[strum(serialize = "default")] Default = 0,
    #[strum(serialize = "tiny")] Tiny = 1,
    #[strum(serialize = "small")] Small = 2,
    #[strum(serialize = "medium")] Medium = 3,
    #[strum(serialize = "large")] Large = 4,
    #[strum(serialize = "huge")] Huge = 5,
    #[strum(serialize = "immobile")] Immobile = 6,
}

/// `global_ai_jump_height_enum` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum GlobalAiJumpHeight {
    #[default]
    #[strum(serialize = "NONE")] None = 0,
    #[strum(serialize = "down")] Down = 1,
    #[strum(serialize = "step")] Step = 2,
    #[strum(serialize = "crouch")] Crouch = 3,
    #[strum(serialize = "stand")] Stand = 4,
    #[strum(serialize = "storey")] Storey = 5,
    #[strum(serialize = "tower")] Tower = 6,
    #[strum(serialize = "infinite")] Infinite = 7,
}

/// `global_object_change_color_enum` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum GlobalObjectChangeColor {
    #[default]
    #[strum(serialize = "none")] None = 0,
    #[strum(serialize = "primary")] Primary = 1,
    #[strum(serialize = "secondary")] Secondary = 2,
    #[strum(serialize = "tertiary")] Tertiary = 3,
    #[strum(serialize = "quaternary")] Quaternary = 4,
}

/// `global_rgb_interpolation_flags` (long_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum GlobalRgbInterpolationFlags {
    #[strum(serialize = "blend in hsv")] BlendInHsv = 0,
    #[strum(serialize = "...more colors")] MoreColors = 1,
}

/// `global_game_engine_type_flags` (word_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u16)]
pub enum GlobalGameEngineTypeFlags {
    #[strum(serialize = "ctf")] Ctf = 0,
    #[strum(serialize = "slayer")] Slayer = 1,
    #[strum(serialize = "oddball")] Oddball = 2,
    #[strum(serialize = "king")] King = 3,
    #[strum(serialize = "juggernaut")] Juggernaut = 4,
    #[strum(serialize = "territories")] Territories = 5,
    #[strum(serialize = "assault")] Assault = 6,
    #[strum(serialize = "vip")] Vip = 7,
    #[strum(serialize = "infection")] Infection = 8,
    #[strum(serialize = "target training")] TargetTraining = 9,
}

/// `multiplayer_object_type` (char_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i8)]
pub enum MultiplayerObjectType {
    #[default]
    #[strum(serialize = "ordinary")] Ordinary = 0,
    #[strum(serialize = "weapon")] Weapon = 1,
    #[strum(serialize = "grenade")] Grenade = 2,
    #[strum(serialize = "projectile")] Projectile = 3,
    #[strum(serialize = "powerup")] Powerup = 4,
    #[strum(serialize = "equipment")] Equipment = 5,
    #[strum(serialize = "light land vehicle")] LightLandVehicle = 6,
    #[strum(serialize = "heavy land vehicle")] HeavyLandVehicle = 7,
    #[strum(serialize = "flying vehicle")] FlyingVehicle = 8,
    #[strum(serialize = "teleporter 2way")] Teleporter2Way = 9,
    #[strum(serialize = "teleporter sender")] TeleporterSender = 10,
    #[strum(serialize = "teleporter receiver")] TeleporterReceiver = 11,
    #[strum(serialize = "player spawn location")] PlayerSpawnLocation = 12,
    #[strum(serialize = "player respawn zone")] PlayerRespawnZone = 13,
    #[strum(serialize = "oddball spawn location")] OddballSpawnLocation = 14,
    #[strum(serialize = "ctf flag spawn location")] CtfFlagSpawnLocation = 15,
    #[strum(serialize = "target spawn location")] TargetSpawnLocation = 16,
    #[strum(serialize = "ctf flag return area")] CtfFlagReturnArea = 17,
    #[strum(serialize = "koth hill area")] KothHillArea = 18,
    #[strum(serialize = "infection safe area")] InfectionSafeArea = 19,
    #[strum(serialize = "territory area")] TerritoryArea = 20,
    #[strum(serialize = "vip influence area")] VipInfluenceArea = 21,
    #[strum(serialize = "vip destination zone")] VipDestinationZone = 22,
    #[strum(serialize = "juggernaut destination zone")] JuggernautDestinationZone = 23,
}

/// `teleporter_passability_flags` (byte_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u8)]
pub enum TeleporterPassabilityFlags {
    #[strum(serialize = "disallow players")] DisallowPlayers = 0,
    #[strum(serialize = "allow light land vehicles")] AllowLightLandVehicles = 1,
    #[strum(serialize = "allow heavy land vehicles")] AllowHeavyLandVehicles = 2,
    #[strum(serialize = "allow flying vehicles")] AllowFlyingVehicles = 3,
    #[strum(serialize = "allow projectiles")] AllowProjectiles = 4,
}

/// `multiplayer_object_flags` (word_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u16)]
pub enum MultiplayerObjectFlags {
    #[strum(serialize = "only visible in editor")] OnlyVisibleInEditor = 0,
    #[strum(serialize = "valid initial player spawn")] ValidInitialPlayerSpawn = 1,
    #[strum(serialize = "fixed boundary orientation")] FixedBoundaryOrientation = 2,
    #[strum(serialize = "candy monitor should ignore")] CandyMonitorShouldIgnore = 3,
}

/// `multiplayer_object_boundary_shape` (char_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i8)]
pub enum MultiplayerObjectBoundaryShape {
    #[default]
    #[strum(serialize = "unused")] Unused = 0,
    #[strum(serialize = "sphere")] Sphere = 1,
    #[strum(serialize = "cylinder")] Cylinder = 2,
    #[strum(serialize = "box")] Box = 3,
}

/// `multiplayer_object_spawn_timer_types` (char_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i8)]
pub enum MultiplayerObjectSpawnTimerTypes {
    #[default]
    #[strum(serialize = "starts on death")] StartsOnDeath = 0,
    #[strum(serialize = "starts on disturbance")] StartsOnDisturbance = 1,
}

// ---------------------------------------------------------------------------
// ObjectFunctionDefinition
// ---------------------------------------------------------------------------

/// One entry of `object_definition::functions[]`. Engine
/// `s_object_function_definition` (44 bytes). Schema:
/// `object_function_block` (`object.json:331-381`).
#[derive(Debug, Clone, Default)]
pub struct ObjectFunctionDefinition {
    /// `flags` (long_flags, `object_function_flags`).
    pub flags: Flags<ObjectFunctionFlags, u32>,
    /// `import name` (string_id) — the name whose magnitude this entry
    /// reads. Resolved via `object_get_function_value(object_index,
    /// import_name, …)` — may recurse into another entry in this
    /// `functions[]` block, into a `<type>_compute_function_value`
    /// case, or hit the built-in `""`/`"one"`/`"zero"` shortcuts.
    pub import_name: String,
    /// `export name` (string_id) — the name this entry defines. The
    /// engine's `object_get_function_value` walker matches against
    /// this when looking for an unresolved input.
    pub export_name: String,
    /// `turn off with` (string_id) — engine field name
    /// `turn_off_with_function_name`. If non-empty, the entry is
    /// inactive when the named function fails to resolve.
    pub turn_off_with_function_name: String,
    /// `min value` (real). Engine field name `lower_bound`. When > 0,
    /// the entry is active only when the curve output exceeds it.
    pub lower_bound: f32,
    /// `default function` (struct, `mapping_function`). Engine field
    /// name `function_value`. The curve applied to the resolved input.
    /// `None` when the authored curve was empty/unset.
    pub function_value: Option<TagFunction>,
    /// `scale by` (string_id). If non-empty, multiply the curve output
    /// by `object_get_function_value(object_index, scale_by, …)`.
    pub scale_by: String,
}

impl ObjectFunctionDefinition {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        let flags = s.try_read_flags("flags").unwrap_or_default();
        let import_name = s.read_string_id("import name").unwrap_or_default();
        let export_name = s.read_string_id("export name").unwrap_or_default();
        let turn_off_with_function_name =
            s.read_string_id("turn off with").unwrap_or_default();
        let lower_bound = s.read_real("min value").unwrap_or(0.0);
        let function_value = s
            .field("default function")
            .and_then(|f| f.as_struct())
            .and_then(|inner| inner.field("data"))
            .and_then(|f| f.as_function());
        let scale_by = s.read_string_id("scale by").unwrap_or_default();
        Self {
            flags,
            import_name,
            export_name,
            turn_off_with_function_name,
            lower_bound,
            function_value,
            scale_by,
        }
    }
}

// ---------------------------------------------------------------------------
// Sub-block structs (in schema declaration order)
// ---------------------------------------------------------------------------

/// `object_early_mover_obb_block` (40 bytes). Field order matches
/// `object.json:254-298` verbatim.
#[derive(Debug, Clone, Default)]
pub struct ObjectEarlyMoverObb {
    /// `node name` (string_id) — empty means object space.
    pub node_name: String,
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
    pub z0: f32,
    pub z1: f32,
    /// `angles` (real_euler_angles_3d).
    pub angles: RealEulerAngles3d,
}

impl ObjectEarlyMoverObb {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            node_name: s.read_string_id("node name").unwrap_or_default(),
            x0: s.read_real("x0").unwrap_or(0.0),
            x1: s.read_real("x1").unwrap_or(0.0),
            y0: s.read_real("y0").unwrap_or(0.0),
            y1: s.read_real("y1").unwrap_or(0.0),
            z0: s.read_real("z0").unwrap_or(0.0),
            z1: s.read_real("z1").unwrap_or(0.0),
            angles: s.read_euler3d("angles"),
        }
    }
}

/// `object_ai_properties_block` (12 bytes). Field order matches
/// `object.json:299-330` verbatim.
#[derive(Debug, Clone, Default)]
pub struct ObjectAiProperties {
    /// `ai flags` (long_flags, `ai_properties_flags`).
    pub ai_flags: Flags<AiPropertiesFlags, u32>,
    /// `ai type name` (string_id) — combat dialogue category.
    pub ai_type_name: String,
    /// `ai size` (short_enum, `ai_size_enum`).
    pub ai_size: Enum<AiSize, i16>,
    /// `leap jump speed` (short_enum, `global_ai_jump_height_enum`).
    pub leap_jump_speed: Enum<GlobalAiJumpHeight, i16>,
}

impl ObjectAiProperties {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            ai_flags: s.try_read_flags("ai flags").unwrap_or_default(),
            ai_type_name: s.read_string_id("ai type name").unwrap_or_default(),
            ai_size: s.try_read_enum("ai size").unwrap_or_default(),
            leap_jump_speed: s.try_read_enum("leap jump speed").unwrap_or_default(),
        }
    }
}

/// `object_attachment_block` (32 bytes). Field order matches
/// `object.json:410-461` verbatim.
#[derive(Debug, Clone, Default)]
pub struct ObjectAttachment {
    /// `type^` (tag_reference — many allowed groups, mostly effe/lens/snd!/cont/lsnd).
    pub type_ref: String,
    /// 4-byte big-endian group fourcc of the [`Self::type_ref`] tag —
    /// `b"effe"` / `b"lens"` / `b"snd!"` / `b"cont"` / `b"lsnd"` /
    /// `b"ligh"` / etc. Engine `attachments_new @ 0x1807E2F60`
    /// dispatches on this exact value (effe→effect_check_object_function_determinacy,
    /// ligh→light_new_attached, lsnd→game_looping_sound_attachment_new,
    /// lens→queued per-frame). `[0; 4]` when the attachment is null.
    pub type_group: [u8; 4],
    /// `marker` (old_string_id) — the node/marker the attachment binds to.
    pub marker: String,
    /// `change color` (short_enum, `global_object_change_color_enum`).
    pub change_color: Enum<GlobalObjectChangeColor, i16>,
    /// `primary scale` (string_id).
    pub primary_scale: String,
    /// `secondary scale` (string_id).
    pub secondary_scale: String,
}

impl ObjectAttachment {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        let (type_group_u32, type_ref) = s
            .read_tag_ref_with_group("type")
            .unwrap_or((0, String::new()));
        Self {
            type_ref,
            type_group: type_group_u32.to_be_bytes(),
            marker: s.read_string_id("marker").unwrap_or_default(),
            change_color: s.try_read_enum("change color").unwrap_or_default(),
            primary_scale: s.read_string_id("primary scale").unwrap_or_default(),
            secondary_scale: s.read_string_id("secondary scale").unwrap_or_default(),
        }
    }
}

/// `object_widget_block` (16 bytes). Field order matches
/// `object.json:463-?`.
#[derive(Debug, Clone, Default)]
pub struct ObjectWidget {
    /// `type` (tag_reference to a widget tag — antenna/light/glow/etc.).
    pub type_ref: String,
}

impl ObjectWidget {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            type_ref: s.read_tag_ref_path("type").unwrap_or_default(),
        }
    }
}

/// `object_change_color_initial_permutation` (32 bytes). Field order
/// matches schema verbatim.
#[derive(Debug, Clone, Default)]
pub struct ObjectChangeColorInitialPermutation {
    pub weight: f32,
    pub color_lower_bound: RealRgbColor,
    pub color_upper_bound: RealRgbColor,
    /// `variant name` (string_id) — empty = any variant.
    pub variant_name: String,
}

impl ObjectChangeColorInitialPermutation {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            weight: s.read_real("weight").unwrap_or(0.0),
            color_lower_bound: s.read_rgb("color lower bound"),
            color_upper_bound: s.read_rgb("color upper bound"),
            variant_name: s.read_string_id("variant name").unwrap_or_default(),
        }
    }
}

/// `object_change_color_function` (36 bytes). Field order matches
/// schema verbatim.
#[derive(Debug, Clone, Default)]
pub struct ObjectChangeColorFunction {
    /// `scale flags` (long_flags).
    pub scale_flags: Flags<GlobalRgbInterpolationFlags, u32>,
    pub color_lower_bound: RealRgbColor,
    pub color_upper_bound: RealRgbColor,
    pub darken_by: String,
    pub scale_by: String,
}

impl ObjectChangeColorFunction {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            scale_flags: s.try_read_flags("scale flags").unwrap_or_default(),
            color_lower_bound: s.read_rgb("color lower bound"),
            color_upper_bound: s.read_rgb("color upper bound"),
            darken_by: s.read_string_id("darken by").unwrap_or_default(),
            scale_by: s.read_string_id("scale by").unwrap_or_default(),
        }
    }
}

/// `object_change_colors` (24 bytes — holds 2 sub-blocks). Field
/// order matches schema verbatim.
#[derive(Debug, Clone, Default)]
pub struct ObjectChangeColors {
    pub initial_permutations: Vec<ObjectChangeColorInitialPermutation>,
    pub functions: Vec<ObjectChangeColorFunction>,
}

impl ObjectChangeColors {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            initial_permutations: read_block_vec(
                s,
                "initial permutations",
                ObjectChangeColorInitialPermutation::from_struct,
            ),
            functions: read_block_vec(s, "functions", ObjectChangeColorFunction::from_struct),
        }
    }
}

/// `multiplayer_object_block` (196 bytes). Field order matches
/// schema verbatim.
#[derive(Debug, Clone, Default)]
pub struct MultiplayerObject {
    /// `game engine flags` (word_flags) — which gametypes include this.
    pub game_engine_flags: Flags<GlobalGameEngineTypeFlags, u16>,
    /// `type` (char_enum) — MP object type (weapon/grenade/spawn/etc.).
    pub mp_type: Enum<MultiplayerObjectType, i8>,
    /// `teleporter passability` (byte_flags) — teleporter-only.
    pub teleporter_passability: Flags<TeleporterPassabilityFlags, u8>,
    /// `flags` (word_flags) — MP-specific.
    pub flags: Flags<MultiplayerObjectFlags, u16>,
    /// `boundary shape` (char_enum).
    pub boundary_shape: Enum<MultiplayerObjectBoundaryShape, i8>,
    /// `spawn timer type` (char_enum).
    pub spawn_timer_type: Enum<MultiplayerObjectSpawnTimerTypes, i8>,
    pub default_spawn_time: i16,
    pub default_abandonment_time: i16,
    pub boundary_width_or_radius: f32,
    pub boundary_box_length: f32,
    pub boundary_positive_height: f32,
    pub boundary_negative_height: f32,
    pub normal_weight: f32,
    pub flag_away_weight: f32,
    pub flag_at_home_weight: f32,
    pub boundary_center_marker: String,
    pub spawned_object_marker_name: String,
    pub spawned_object: String,
    pub nyi_boundary_material: String,
    pub boundary_standard_shader: String,
    pub boundary_opaque_shader: String,
    pub sphere_standard_shader: String,
    pub sphere_opaque_shader: String,
    pub cylinder_standard_shader: String,
    pub cylinder_opaque_shader: String,
    pub box_standard_shader: String,
    pub box_opaque_shader: String,
}

impl MultiplayerObject {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            game_engine_flags: s.try_read_flags("game engine flags").unwrap_or_default(),
            mp_type: s.try_read_enum("type").unwrap_or_default(),
            teleporter_passability: s.try_read_flags("teleporter passability").unwrap_or_default(),
            flags: s.try_read_flags("flags").unwrap_or_default(),
            boundary_shape: s.try_read_enum("boundary shape").unwrap_or_default(),
            spawn_timer_type: s.try_read_enum("spawn timer type").unwrap_or_default(),
            default_spawn_time: s.read_int_any("default spawn time").unwrap_or(0) as i16,
            default_abandonment_time: s.read_int_any("default abandonment time").unwrap_or(0) as i16,
            boundary_width_or_radius: s.read_real("boundary width/radius").unwrap_or(0.0),
            boundary_box_length: s.read_real("boundary box length").unwrap_or(0.0),
            boundary_positive_height: s.read_real("boundary +height").unwrap_or(0.0),
            boundary_negative_height: s.read_real("boundary -height").unwrap_or(0.0),
            normal_weight: s.read_real("normal weight").unwrap_or(0.0),
            flag_away_weight: s.read_real("flag away weight").unwrap_or(0.0),
            flag_at_home_weight: s.read_real("flag at home weight").unwrap_or(0.0),
            boundary_center_marker: s.read_string_id("boundary center marker").unwrap_or_default(),
            spawned_object_marker_name: s
                .read_string_id("spawned object marker name")
                .unwrap_or_default(),
            spawned_object: s.read_tag_ref_path("spawned object").unwrap_or_default(),
            nyi_boundary_material: s
                .read_string_id("NYI boundary material")
                .unwrap_or_default(),
            boundary_standard_shader: s
                .read_tag_ref_path("boundary standard shader")
                .unwrap_or_default(),
            boundary_opaque_shader: s
                .read_tag_ref_path("boundary opaque shader")
                .unwrap_or_default(),
            sphere_standard_shader: s
                .read_tag_ref_path("sphere standard shader")
                .unwrap_or_default(),
            sphere_opaque_shader: s
                .read_tag_ref_path("sphere opaque shader")
                .unwrap_or_default(),
            cylinder_standard_shader: s
                .read_tag_ref_path("cylinder standard shader")
                .unwrap_or_default(),
            cylinder_opaque_shader: s
                .read_tag_ref_path("cylinder opaque shader")
                .unwrap_or_default(),
            box_standard_shader: s
                .read_tag_ref_path("box standard shader")
                .unwrap_or_default(),
            box_opaque_shader: s
                .read_tag_ref_path("box opaque shader")
                .unwrap_or_default(),
        }
    }
}

/// `object_health_pack_block` (16 bytes). Field order matches schema.
#[derive(Debug, Clone, Default)]
pub struct ObjectHealthPack {
    pub health_pack_equipment: String,
}

impl ObjectHealthPack {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            health_pack_equipment: s
                .read_tag_ref_path("health pack equipment")
                .unwrap_or_default(),
        }
    }
}

/// Helper: walk a tag block field and collect parsed elements.
fn read_block_vec<T, F>(s: &TagStruct<'_>, name: &str, mut f: F) -> Vec<T>
where
    F: FnMut(&TagStruct<'_>) -> T,
{
    s.field(name)
        .and_then(|f| f.as_block())
        .map(|block| block.iter().map(|e| f(&e)).collect::<Vec<_>>())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// ObjectDefinition
// ---------------------------------------------------------------------------

/// Walked subset of the engine `object_struct_definition` (size 248).
/// Field order matches the schema's `fields` array verbatim
/// (`object.json:75-251`) so a future side-by-side reads cleanly.
/// Not-yet-consumed sub-blocks (`early mover OBB`, `ai properties`,
/// `attachments`, `widgets`, `change colors`, `multiplayer object`,
/// `health packs`) are omitted — add them in schema order as
/// consumers come online.
#[derive(Debug, Clone, Default)]
pub struct ObjectDefinition {
    /// `flags` (word_flags, `object_definition_flags`).
    pub flags: Flags<ObjectDefinitionFlags, u16>,
    /// `bounding radius:world units` (real).
    pub bounding_radius: f32,
    /// `bounding offset` (real_point_3d).
    pub bounding_offset: RealPoint3d,
    /// `acceleration scale` (real) — AI movement scale; not used by
    /// renderer but kept for completeness.
    pub acceleration_scale: f32,
    /// `lightmap shadow mode` (short_enum). Gate in
    /// `render_object_has_lightmap_shadow`.
    pub lightmap_shadow_mode: Enum<LightmapShadowMode, i16>,
    /// `sweetener size` (char_enum) — sound-system field; not used
    /// by renderer.
    pub sweetener_size: Enum<SweetenerSize, i8>,
    /// `water density` (char_enum) — physics field; not used by
    /// renderer.
    pub water_density: Enum<WaterDensityType, i8>,
    /// `dynamic light sphere radius` (real). Override sphere for
    /// dynamic-lights bookkeeping; only used when non-zero.
    pub dynamic_light_sphere_radius: f32,
    /// `dynamic light sphere offset` (real_point_3d). Only consulted
    /// when `dynamic_light_sphere_radius > 0`.
    pub dynamic_light_sphere_offset: RealPoint3d,
    /// `default model variant` (string_id).
    pub default_model_variant: String,
    /// `model` (tag_reference → `hlmt`) — path to the `.model` tag.
    /// Empty when the placement is geometry-less. Chain:
    /// `model.render_model.path` → `.render_model` file.
    pub model: String,
    /// `crate object` (tag_reference → `bloc`).
    pub crate_object: String,
    /// `collision damage` (tag_reference → `cddf`).
    pub collision_damage: String,
    /// `early mover OBB` block (max 1) — engine
    /// `s_object_early_mover_obb_definition`.
    pub early_mover_obb: Vec<ObjectEarlyMoverObb>,
    /// `creation effect` (tag_reference → `effe`).
    pub creation_effect: String,
    /// `material effects` (tag_reference → `foot`).
    pub material_effects: String,
    /// `melee sound` (tag_reference → `snd!`).
    pub melee_sound: String,
    /// `ai properties` block (max 1) — combat dialogue + AI sizing.
    pub ai_properties: Vec<ObjectAiProperties>,
    /// `functions` (block, `object_function_block[]`, max 256).
    pub functions: Vec<ObjectFunctionDefinition>,
    /// `hud text message index` (short_integer).
    pub hud_text_message_index: i16,
    /// `secondary flags` (word_flags, `object_definition_secondary_flags`).
    pub secondary_flags: Flags<ObjectDefinitionSecondaryFlags, u16>,
    /// `attachments` block (max 16).
    pub attachments: Vec<ObjectAttachment>,
    /// `widgets` block (max 4).
    pub widgets: Vec<ObjectWidget>,
    /// `change colors` block (max 4) — initial-permutation + function
    /// pairs per color channel.
    pub change_colors: Vec<ObjectChangeColors>,
    /// `multiplayer object` block (max 1) — MP gametype inclusion +
    /// boundary shape + spawn timing + boundary shaders.
    pub multiplayer_object: Vec<MultiplayerObject>,
    /// `health packs` block (max 16) — health-pack equipment refs.
    pub health_packs: Vec<ObjectHealthPack>,
}

impl ObjectDefinition {
    /// Read the `object_struct_definition` out of any of the 14
    /// object-subgroup tag files. Probes each inheritance prefix
    /// (`weapon/item/object`, `item/object`, `unit/object`,
    /// `device/object`, `object`, the root) and uses the first one
    /// where the `object_struct_definition`-specific field
    /// `lightmap shadow mode` is readable — the unique identifier
    /// among the inheritance levels.
    ///
    /// Errors when the tag's group is not in [`OBJECT_SUBGROUPS`].
    /// Returns a default-filled struct when the tag has no probable
    /// object substruct (unusual — most authored tags should have one).
    pub fn from_tag(tag: &TagFile) -> Result<Self, ObjectDefinitionError> {
        let actual = tag.group().tag.to_be_bytes();
        if !OBJECT_SUBGROUPS.contains(&actual) {
            return Err(ObjectDefinitionError::WrongGroup { actual });
        }
        let root = tag.root();
        for prefix in OBJECT_INHERITANCE_PREFIXES {
            let s = if prefix.is_empty() {
                root.clone()
            } else {
                match root.descend(prefix) {
                    Some(s) => s,
                    None => continue,
                }
            };
            // `lightmap shadow mode` is authored only in the
            // object_struct_definition — use it as the probe field.
            if s.read_int_any("lightmap shadow mode").is_some() {
                return Ok(Self::from_object_struct(&s));
            }
        }
        Ok(Self::default())
    }

    /// Construct directly from a [`TagStruct`] that IS the
    /// `object_struct_definition` (i.e. the caller already descended
    /// to the right inheritance level). Useful when an outer tag
    /// reader has already walked there.
    pub fn from_object_struct(obj: &TagStruct<'_>) -> Self {
        let flags = obj.try_read_flags("flags").unwrap_or_default();
        let bounding_radius = obj.read_real("bounding radius").unwrap_or(0.0);
        let bounding_offset = obj.read_point3d("bounding offset");
        let acceleration_scale = obj.read_real("acceleration scale").unwrap_or(0.0);
        let lightmap_shadow_mode = obj.try_read_enum("lightmap shadow mode").unwrap_or_default();
        let sweetener_size = obj.try_read_enum("sweetener size").unwrap_or_default();
        let water_density = obj.try_read_enum("water density").unwrap_or_default();
        let dynamic_light_sphere_radius = obj
            .read_real("dynamic light sphere radius")
            .unwrap_or(0.0);
        let dynamic_light_sphere_offset = obj.read_point3d("dynamic light sphere offset");
        let default_model_variant = obj
            .read_string_id("default model variant")
            .unwrap_or_default();
        let model = obj.read_tag_ref_path("model").unwrap_or_default();
        let crate_object = obj.read_tag_ref_path("crate object").unwrap_or_default();
        let collision_damage = obj.read_tag_ref_path("collision damage").unwrap_or_default();
        let early_mover_obb = read_block_vec(obj, "early mover OBB", ObjectEarlyMoverObb::from_struct);
        let creation_effect = obj.read_tag_ref_path("creation effect").unwrap_or_default();
        let material_effects = obj.read_tag_ref_path("material effects").unwrap_or_default();
        let melee_sound = obj.read_tag_ref_path("melee sound").unwrap_or_default();
        let ai_properties = read_block_vec(obj, "ai properties", ObjectAiProperties::from_struct);
        let functions = read_block_vec(obj, "functions", ObjectFunctionDefinition::from_struct);
        let hud_text_message_index = obj
            .read_int_any("hud text message index")
            .unwrap_or(0) as i16;
        let secondary_flags = obj.try_read_flags("secondary flags").unwrap_or_default();
        let attachments = read_block_vec(obj, "attachments", ObjectAttachment::from_struct);
        let widgets = read_block_vec(obj, "widgets", ObjectWidget::from_struct);
        let change_colors = read_block_vec(obj, "change colors", ObjectChangeColors::from_struct);
        let multiplayer_object = read_block_vec(obj, "multiplayer object", MultiplayerObject::from_struct);
        let health_packs = read_block_vec(obj, "health packs", ObjectHealthPack::from_struct);

        Self {
            flags,
            bounding_radius,
            bounding_offset,
            acceleration_scale,
            lightmap_shadow_mode,
            sweetener_size,
            water_density,
            dynamic_light_sphere_radius,
            dynamic_light_sphere_offset,
            default_model_variant,
            model,
            crate_object,
            collision_damage,
            early_mover_obb,
            creation_effect,
            material_effects,
            melee_sound,
            ai_properties,
            functions,
            hud_text_message_index,
            secondary_flags,
            attachments,
            widgets,
            change_colors,
            multiplayer_object,
            health_packs,
        }
    }

    /// Linear scan of `functions[]` for an entry whose `export_name`
    /// matches `name`. Engine equivalent: the inner loop in
    /// `object_get_function_value @ 0x1807DBA60` between LABEL_34 and
    /// the `goto LABEL_34;` step (`if ( v20->export_name == v9 )
    /// break;`).
    pub fn find_function_by_export(&self, name: &str) -> Option<&ObjectFunctionDefinition> {
        self.functions.iter().find(|f| f.export_name == name)
    }

    /// `true` iff `bounding_radius` is non-zero — i.e. the tag has an
    /// authored sphere (vs `0` = "use autogen", a Bungie convention).
    /// Callers should fall through to the .model's
    /// `model object data[0]` auto-bake or a vertex-walk autogen when
    /// this returns false.
    pub fn has_authored_bounding_sphere(&self) -> bool {
        self.bounding_radius > 0.0
    }

    /// Engine-relaxed shadow-eligibility gate. Returns `false` when
    /// either:
    ///   - `flags & OBJ_FLAG_DOES_NOT_CAST_SHADOW` (explicit tag-time
    ///     opt-out), OR
    ///   - `lightmap_shadow_mode == 1` (`never`).
    ///
    /// Otherwise `true`. The full engine gate
    /// (`render_object_has_lightmap_shadow @ 0x180696EE0`) drops
    /// static scenery from the runtime shadow loop when the cache
    /// builder bakes their shadows offline into the lightmap atlas;
    /// protomorph doesn't have offline object-shadow baking yet, so
    /// we treat everything not explicitly opted-out as runtime-cast.
    /// See `protomorph/src/halo/loader.rs::read_casts_shadow_flag`
    /// for the prior in-place implementation.
    pub fn casts_shadow_runtime(&self) -> bool {
        if self.flags.contains(ObjectDefinitionFlags::DoesNotCastShadow) {
            return false;
        }
        if self.lightmap_shadow_mode.get() == LightmapShadowMode::Never {
            return false;
        }
        true
    }
}
