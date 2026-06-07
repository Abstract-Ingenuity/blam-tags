//! `.weapon` (`weap`) tag walker — and the intermediate `.item` (`item`)
//! parent. Surfaces the authored fields the runtime
//! `weapon_compute_function_value` reads from
//! `weapon_definition` / `item_struct_definition`.
//!
//! Schema reference:
//! - `definitions/halo3_mcc/weapon.json` → `weapon_group` (size 1244,
//!   parent_tag `item`).
//! - `definitions/halo3_mcc/item.json` → `item_struct_definition`
//!   (parent_tag `obje`).
//! - `definitions/halo3_mcc/object.json` → `object_struct_definition`
//!   (already walked by [`crate::object::ObjectDefinition`]).
//!
//! Composition mirrors the engine layout: a weapon tag's root carries
//! an `item` substruct which carries an `object` substruct. Each
//! layer's `Definition` struct holds its parent via `Arc<...>` so
//! consumers can grab any layer cheaply:
//!
//! ```text
//!   WeaponDefinition          ← from `weap` tag root
//!     item: Arc<ItemDefinition>
//!         object: Arc<ObjectDefinition>
//! ```
//!
//! Drives the protomorph `weapon_compute_function_value` cases that
//! read weapon-definition fields (overheat thresholds, magazine
//! ammo maxima, heat illumination, barrel/trigger counts).

use crate::api::TagStruct;
use crate::file::TagFile;
use crate::item::ItemDefinition;
use crate::math::{Bounds, RealEulerAngles2d, RealVector2d, RealVector3d};
use crate::object::ObjectDefinition;
use crate::typed_enums::{Enum, Flags};
use std::sync::Arc;

/// `weapon_definition_flags` (long_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum WeaponDefinitionFlags {
    // NOTE: this is the FULL 32-flag H3 layout the actual .weapon tags carry.
    // The MCC JSON schema is newer and PRUNED 6 flags (vertical heat display,
    // mutually exclusive triggers, attacks automatically on bump, must be
    // picked up, holds triggers when dropped, enables integrated night
    // vision), leaving 26. We supersize the enum so the still-present
    // historical bits resolve by name instead of fail-loud panicking.
    // Discriminants follow the tag's real bit positions.
    #[strum(serialize = "vertical heat display")] VerticalHeatDisplay = 0,
    #[strum(serialize = "mutually exclusive triggers")] MutuallyExclusiveTriggers = 1,
    #[strum(serialize = "attacks automatically on bump")] AttacksAutomaticallyOnBump = 2,
    #[strum(serialize = "must be readied")] MustBeReadied = 3,
    #[strum(serialize = "doesn't count toward maximum")] DoesntCountTowardMaximum = 4,
    #[strum(serialize = "aim assists only when zoomed")] AimAssistsOnlyWhenZoomed = 5,
    #[strum(serialize = "prevents grenade throwing")] PreventsGrenadeThrowing = 6,
    #[strum(serialize = "must be picked up")] MustBePickedUp = 7,
    #[strum(serialize = "holds triggers when dropped")] HoldsTriggersWhenDropped = 8,
    #[strum(serialize = "prevents melee attack")] PreventsMeleeAttack = 9,
    #[strum(serialize = "detonates when dropped")] DetonatesWhenDropped = 10,
    #[strum(serialize = "cannot fire at maximum age")] CannotFireAtMaximumAge = 11,
    #[strum(serialize = "secondary trigger overrides grenades")] SecondaryTriggerOverridesGrenades = 12,
    #[strum(serialize = "support weapon")] SupportWeapon = 13,
    #[strum(serialize = "enables integrated night vision")] EnablesIntegratedNightVision = 14,
    #[strum(serialize = "AIs use weapon melee damage")] AisUseWeaponMeleeDamage = 15,
    #[strum(serialize = "forces no binoculars")] ForcesNoBinoculars = 16,
    #[strum(serialize = "loop fp firing animation")] LoopFpFiringAnimation = 17,
    #[strum(serialize = "prevents crouching")] PreventsCrouching = 18,
    #[strum(serialize = "cannot fire while boosting")] CannotFireWhileBoosting = 19,
    #[strum(serialize = "use empty melee on empty")] UseEmptyMeleeOnEmpty = 20,
    #[strum(serialize = "uses 3rd person camera")] Uses3rdPersonCamera = 21,
    #[strum(serialize = "can be dual wielded")] CanBeDualWielded = 22,
    #[strum(serialize = "can only be dual wielded")] CanOnlyBeDualWielded = 23,
    #[strum(serialize = "melee only")] MeleeOnly = 24,
    #[strum(serialize = "cant fire if parent dead")] CantFireIfParentDead = 25,
    #[strum(serialize = "weapon ages with each kill")] WeaponAgesWithEachKill = 26,
    #[strum(serialize = "weapon uses old dual fire error code")] UsesOldDualFireErrorCode = 27,
    #[strum(serialize = "allows unaimed lunge")] AllowsUnaimedLunge = 28,
    #[strum(serialize = "cannot be used by player")] CannotBeUsedByPlayer = 29,
    #[strum(serialize = "hold fp firing animation")] HoldFpFiringAnimation = 30,
    #[strum(serialize = "strict deviation angle")] StrictDeviationAngle = 31,
}

/// `weapon_definition_secondary_flags` (long_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum WeaponDefinitionSecondaryFlags {
    #[strum(serialize = "magnitizes only when zoomed")] MagnitizesOnlyWhenZoomed = 0,
    #[strum(serialize = "force enable equipment tossing")] ForceEnableEquipmentTossing = 1,
    #[strum(serialize = "non-lunge melee dash disabled")] NonLungeMeleeDashDisabled = 2,
}

/// `secondary_trigger_modes` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum SecondaryTriggerMode {
    #[default]
    #[strum(serialize = "normal")] Normal = 0,
    #[strum(serialize = "slaved to primary")] SlavedToPrimary = 1,
    #[strum(serialize = "inhibits primary")] InhibitsPrimary = 2,
    #[strum(serialize = "loads alterate ammunition")] LoadsAlternateAmmunition = 3,
    #[strum(serialize = "loads multiple primary ammunition")] LoadsMultiplePrimaryAmmunition = 4,
}

/// `global_damage_reporting_enum_definition` (char_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i8)]
pub enum GlobalDamageReporting {
    #[default]
    #[strum(serialize = "teh guardians")] TehGuardians = 0,
    #[strum(serialize = "falling damage")] FallingDamage = 1,
    #[strum(serialize = "generic collision damage")] GenericCollisionDamage = 2,
    #[strum(serialize = "generic melee damage")] GenericMeleeDamage = 3,
    #[strum(serialize = "generic explosion")] GenericExplosion = 4,
    #[strum(serialize = "magnum pistol")] MagnumPistol = 5,
    #[strum(serialize = "plasma pistol")] PlasmaPistol = 6,
    #[strum(serialize = "needler")] Needler = 7,
    #[strum(serialize = "excavator")] Excavator = 8,
    #[strum(serialize = "smg")] Smg = 9,
    #[strum(serialize = "plasma rifle")] PlasmaRifle = 10,
    #[strum(serialize = "battle rifle")] BattleRifle = 11,
    #[strum(serialize = "carbine")] Carbine = 12,
    #[strum(serialize = "shotgun")] Shotgun = 13,
    #[strum(serialize = "sniper rifle")] SniperRifle = 14,
    #[strum(serialize = "beam rifle")] BeamRifle = 15,
    #[strum(serialize = "assault rifle")] AssaultRifle = 16,
    #[strum(serialize = "spike rifle")] SpikeRifle = 17,
    #[strum(serialize = "flak cannon")] FlakCannon = 18,
    #[strum(serialize = "missile launcher")] MissileLauncher = 19,
    #[strum(serialize = "rocket launcher")] RocketLauncher = 20,
    #[strum(serialize = "spartan laser")] SpartanLaser = 21,
    #[strum(serialize = "brute shot")] BruteShot = 22,
    #[strum(serialize = "flame thrower")] FlameThrower = 23,
    #[strum(serialize = "sentinal gun")] SentinalGun = 24,
    #[strum(serialize = "energy sword")] EnergySword = 25,
    #[strum(serialize = "gravity hammer")] GravityHammer = 26,
    #[strum(serialize = "frag grenade")] FragGrenade = 27,
    #[strum(serialize = "plasma grenade")] PlasmaGrenade = 28,
    #[strum(serialize = "claymore grenade")] ClaymoreGrenade = 29,
    #[strum(serialize = "firebomb grenade")] FirebombGrenade = 30,
    #[strum(serialize = "flag melee damage")] FlagMeleeDamage = 31,
    #[strum(serialize = "bomb melee damage")] BombMeleeDamage = 32,
    #[strum(serialize = "bomb explosion damage")] BombExplosionDamage = 33,
    #[strum(serialize = "ball melee damage")] BallMeleeDamage = 34,
    #[strum(serialize = "human turret")] HumanTurret = 35,
    #[strum(serialize = "plasma cannon")] PlasmaCannon = 36,
    #[strum(serialize = "plasma mortar")] PlasmaMortar = 37,
    #[strum(serialize = "plasma turret")] PlasmaTurret = 38,
    #[strum(serialize = "banshee")] Banshee = 39,
    #[strum(serialize = "ghost")] Ghost = 40,
    #[strum(serialize = "mongoose")] Mongoose = 41,
    #[strum(serialize = "scorpion")] Scorpion = 42,
    #[strum(serialize = "scorpion gunner")] ScorpionGunner = 43,
    #[strum(serialize = "warthog driver")] WarthogDriver = 44,
    #[strum(serialize = "warthog gunner")] WarthogGunner = 45,
    #[strum(serialize = "warthog gunner gauss")] WarthogGunnerGauss = 46,
    #[strum(serialize = "wraith")] Wraith = 47,
    #[strum(serialize = "wraith anti-infantry")] WraithAntiInfantry = 48,
    #[strum(serialize = "tank")] Tank = 49,
    #[strum(serialize = "chopper")] Chopper = 50,
    #[strum(serialize = "hornet")] Hornet = 51,
    #[strum(serialize = "mantis")] Mantis = 52,
    #[strum(serialize = "mauler")] Mauler = 53,
    #[strum(serialize = "sentinel beam")] SentinelBeam = 54,
    #[strum(serialize = "sentinel rpg")] SentinelRpg = 55,
    #[strum(serialize = "teleporter")] Teleporter = 56,
    #[strum(serialize = "prox-mine")] ProxMine = 57,
    #[strum(serialize = "elephant turret")] ElephantTurret = 58,
    #[strum(serialize = "shade turret")] ShadeTurret = 59,
    #[strum(serialize = "silenced smg")] SilencedSmg = 60,
    #[strum(serialize = "automag")] Automag = 61,
    #[strum(serialize = "brute plasma rifle")] BrutePlasmaRifle = 62,
}

/// `movement_penalty_modes` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum MovementPenaltyMode {
    #[default]
    #[strum(serialize = "always")] Always = 0,
    #[strum(serialize = "when zoomed")] WhenZoomed = 1,
    #[strum(serialize = "when zoomed or reloading")] WhenZoomedOrReloading = 2,
}

/// `multiplayer_weapon_types` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum MultiplayerWeaponType {
    #[default]
    #[strum(serialize = "none")] None = 0,
    #[strum(serialize = "ctf flag")] CtfFlag = 1,
    #[strum(serialize = "oddball ball")] OddballBall = 2,
    #[strum(serialize = "headhunter head")] HeadhunterHead = 3,
    #[strum(serialize = "juggernaut powerup")] JuggernautPowerup = 4,
}

/// `weapon_types` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum WeaponType {
    #[default]
    #[strum(serialize = "undefined")] Undefined = 0,
    #[strum(serialize = "shotgun")] Shotgun = 1,
    #[strum(serialize = "needler")] Needler = 2,
    #[strum(serialize = "plasma pistol")] PlasmaPistol = 3,
    #[strum(serialize = "plasma rifle")] PlasmaRifle = 4,
    #[strum(serialize = "rocket launcher")] RocketLauncher = 5,
    #[strum(serialize = "energy blade")] EnergyBlade = 6,
    #[strum(serialize = "splaser")] Splaser = 7,
}

/// `weapon_tracking_types` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum WeaponTrackingType {
    #[default]
    #[strum(serialize = "no tracking")] NoTracking = 0,
    #[strum(serialize = "human tracking")] HumanTracking = 1,
    #[strum(serialize = "plasma tracking")] PlasmaTracking = 2,
}

/// `magazine_flags` (long_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum MagazineFlags {
    #[strum(serialize = "wastes rounds when reloaded")] WastesRoundsWhenReloaded = 0,
    #[strum(serialize = "every round must be chambered")] EveryRoundMustBeChambered = 1,
}

/// `weapon_barrel_flags` (long_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum WeaponBarrelFlags {
    #[strum(serialize = "tracks fired projectile")] TracksFiredProjectile = 0,
    #[strum(serialize = "random firing effects")] RandomFiringEffects = 1,
    #[strum(serialize = "can fire with partial ammo")] CanFireWithPartialAmmo = 2,
    #[strum(serialize = "projectiles use weapon origin")] ProjectilesUseWeaponOrigin = 3,
    #[strum(serialize = "ejects during chamber")] EjectsDuringChamber = 4,
    #[strum(serialize = "use error when unzoomed")] UseErrorWhenUnzoomed = 5,
    #[strum(serialize = "projectile vector cannot be adjusted")] ProjectileVectorCannotBeAdjusted = 6,
    #[strum(serialize = "projectiles have identical error")] ProjectilesHaveIdenticalError = 7,
    #[strum(serialize = "projectiles fire parallel")] ProjectilesFireParallel = 8,
    #[strum(serialize = "cant fire when others firing")] CantFireWhenOthersFiring = 9,
    #[strum(serialize = "cant fire when others recovering")] CantFireWhenOthersRecovering = 10,
    #[strum(serialize = "don't clear fire bit after recovering")] DontClearFireBitAfterRecovering = 11,
    #[strum(serialize = "stagger fire across multiple markers")] StaggerFireAcrossMultipleMarkers = 12,
    #[strum(serialize = "fires locked projectiles")] FiresLockedProjectiles = 13,
    #[strum(serialize = "can fire at maximum age")] CanFireAtMaximumAge = 14,
    #[strum(serialize = "use 1 firing effect per burst")] Use1FiringEffectPerBurst = 15,
    #[strum(serialize = "ignore tracked object")] IgnoreTrackedObject = 16,
}

/// `weapon_trigger_definition_flags` (long_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum WeaponTriggerDefinitionFlags {
    #[strum(serialize = "autofire single action only")] AutofireSingleActionOnly = 0,
}

/// `weapon_trigger_inputs` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum WeaponTriggerInput {
    #[default]
    #[strum(serialize = "right trigger")] RightTrigger = 0,
    #[strum(serialize = "left trigger")] LeftTrigger = 1,
    #[strum(serialize = "melee attack")] MeleeAttack = 2,
}

/// `weapon_trigger_behaviors` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum WeaponTriggerBehavior {
    #[default]
    #[strum(serialize = "spew")] Spew = 0,
    #[strum(serialize = "latch")] Latch = 1,
    #[strum(serialize = "latch-autofire")] LatchAutofire = 2,
    #[strum(serialize = "charge")] Charge = 3,
    #[strum(serialize = "latch-zoom")] LatchZoom = 4,
    #[strum(serialize = "latch-rocketlauncher")] LatchRocketlauncher = 5,
    #[strum(serialize = "spew-charge")] SpewCharge = 6,
    #[strum(serialize = "sword-charge")] SwordCharge = 7,
}

/// `trigger_prediction_type_enum` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum TriggerPredictionType {
    #[default]
    #[strum(serialize = "none")] None = 0,
    #[strum(serialize = "spew")] Spew = 1,
    #[strum(serialize = "charge")] Charge = 2,
}

/// `weapon_trigger_autofire_actions` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum WeaponTriggerAutofireAction {
    #[default]
    #[strum(serialize = "fire")] Fire = 0,
    #[strum(serialize = "charge")] Charge = 1,
    #[strum(serialize = "track")] Track = 2,
    #[strum(serialize = "fire other")] FireOther = 3,
}

/// `weapon_trigger_overcharged_actions` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum WeaponTriggerOverchargedAction {
    #[default]
    #[strum(serialize = "none")] None = 0,
    #[strum(serialize = "explode")] Explode = 1,
    #[strum(serialize = "discharge")] Discharge = 2,
}

const WEAPON_GROUP: [u8; 4] = *b"weap";

/// Errors from `.weapon` / `.item` tag walking.
#[derive(Debug)]
pub enum WeaponError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
    MissingSubstruct { path: &'static str },
    ObjectDefinition(crate::object::ObjectDefinitionError),
}

impl std::fmt::Display for WeaponError {
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

impl std::error::Error for WeaponError {}

impl From<crate::object::ObjectDefinitionError> for WeaponError {
    fn from(e: crate::object::ObjectDefinitionError) -> Self {
        Self::ObjectDefinition(e)
    }
}

// ---------------------------------------------------------------------------
// WeaponDefinition — `weapon_group`
// ---------------------------------------------------------------------------

/// Walked `weapon_group`. Field order matches `weapon.json` verbatim
/// (size 1244, parent_tag `item`). Deep substructs
/// (`melee_damage_parameters_struct`, `aim_assist_struct`,
/// `weapon_tracking_struct`, `weapon_interface_struct`) are surfaced
/// via the dedicated `WeaponMeleeDamageParameters`, `WeaponAimAssist`,
/// `WeaponTracking`, and `WeaponInterface` types defined below.
#[derive(Debug, Clone, Default)]
pub struct WeaponDefinition {
    /// Inherited `item_struct_definition` body. `Arc`-wrapped so any
    /// consumer that wants the `item` view (or further down to
    /// `object`) gets a cheap clone.
    pub item: Arc<ItemDefinition>,

    /// `weapon_definition_flags` (long_flags).
    pub flags: Flags<WeaponDefinitionFlags, u32>,
    /// `secondary flags` (long_flags).
    pub secondary_flags: Flags<WeaponDefinitionSecondaryFlags, u32>,
    /// `unused label!` (old_string_id) — retained for layout parity.
    pub unused_label: String,
    /// `secondary trigger mode` (short_enum).
    pub secondary_trigger_mode: Enum<SecondaryTriggerMode, i16>,
    /// `maximum alternate shots loaded` (short_integer).
    pub maximum_alternate_shots_loaded: i16,
    /// `turn on time` (real).
    pub turn_on_time: f32,
    /// `ready time:seconds` (real).
    pub ready_time: f32,
    /// `ready effect` (tag_reference).
    pub ready_effect: String,
    /// `ready damage effect` (tag_reference).
    pub ready_damage_effect: String,

    // -- heat curve --
    pub heat_recovery_threshold: f32,
    pub overheated_threshold: f32,
    pub heat_detonation_threshold: f32,
    pub heat_detonation_fraction: f32,
    pub heat_loss_per_second: f32,
    pub heat_illumination: f32,
    pub overheated_heat_loss_per_second: f32,

    pub overheated: String,
    pub overheated_damage_effect: String,
    pub detonation: String,
    pub detonation_damage_effect: String,

    pub player_melee_damage: String,
    pub player_melee_response: String,
    /// `melee damage parameters` substruct — pyramid angles + depth +
    /// 12 melee damage/response tag refs across hit / lunge / empty /
    /// clang variants.
    pub melee_damage_parameters: WeaponMeleeDamageParameters,
    pub clang_effect: String,
    /// `melee damage reporting type` (char_enum).
    pub melee_damage_reporting_type: Enum<GlobalDamageReporting, i8>,

    // -- zoom --
    pub magnification_levels: i16,
    pub magnification_range: Bounds<f32>,
    /// `weapon aim assist` substruct — autoaim/magnetism/deviation
    /// angle + range parameters.
    pub aim_assist: WeaponAimAssist,

    // -- movement --
    /// `movement penalized` (short_enum).
    pub movement_penalized: Enum<MovementPenaltyMode, i16>,
    pub forward_movement_penalty: f32,
    pub sideways_movement_penalty: f32,

    // -- AI --
    pub ai_scariness: f32,

    // -- power on/off --
    pub weapon_power_on_time: f32,
    pub weapon_power_off_time: f32,
    pub weapon_power_on_effect: String,
    pub weapon_power_off_effect: String,

    // -- age (weapon "wear") --
    pub age_heat_recovery_penalty: f32,
    pub age_rate_of_fire_penalty: f32,
    pub age_misfire_start: f32,
    pub age_misfire_chance: f32,

    pub pickup_sound: String,
    pub zoom_in_sound: String,
    pub zoom_out_sound: String,

    pub active_camo_ding: f32,
    pub active_camo_regrowth_rate: f32,

    /// `handle node` (string_id) — node attached to unit's hand.
    pub handle_node: String,
    pub weapon_class: String,
    pub weapon_name: String,

    /// `multiplayer weapon type` (short_enum).
    pub multiplayer_weapon_type: Enum<MultiplayerWeaponType, i16>,
    /// `weapon type` (short_enum).
    pub weapon_type: Enum<WeaponType, i16>,
    /// `tracking` substruct — single `tracking_type` enum.
    pub tracking: WeaponTracking,
    /// `player interface` substruct — first-person interface block
    /// count + chud_interface ref.
    pub player_interface: WeaponInterface,

    /// `predicted resources` block — count only.
    pub predicted_resources_count: usize,
    pub magazines: Vec<WeaponDefinitionMagazine>,
    pub triggers: Vec<WeaponDefinitionTrigger>,
    pub barrels: Vec<WeaponDefinitionBarrel>,

    // -- first-person movement control --
    pub max_movement_acceleration: f32,
    pub max_movement_velocity: f32,
    pub max_turning_acceleration: f32,
    pub max_turning_velocity: f32,

    /// `deployed vehicle` (tag_reference) — weapon that becomes a
    /// vehicle on deploy.
    pub deployed_vehicle: String,
    pub tossed_weapon: String,
    pub age_effect: String,
    pub aged_material_effects: String,

    pub external_aging_amount: f32,
    pub campaign_external_aging_amount: f32,

    /// `first person weapon offset` (Vec3).
    pub first_person_weapon_offset: RealVector3d,
    /// `first person weapon offset override` (Vec3) — for centered crosshair.
    pub first_person_weapon_offset_override: RealVector3d,
    /// `first person scope size` (Vec2).
    pub first_person_scope_size: RealVector2d,
    /// `support third person camera range:degrees` (Bounds, radians on disk).
    pub support_third_person_camera_range: Bounds<f32>,

    pub weapon_zoom_time: f32,
    pub weapon_ready_for_use_time: f32,
    pub unit_stow_anchor_name: String,
}

/// `melee_damage_parameters_struct` (size 204).
#[derive(Debug, Clone, Default)]
pub struct WeaponMeleeDamageParameters {
    pub damage_pyramid_angles: RealEulerAngles2d,
    pub damage_pyramid_depth: f32,
    pub first_hit_melee_damage: String,
    pub first_hit_melee_response: String,
    pub second_hit_melee_damage: String,
    pub second_hit_melee_response: String,
    pub third_hit_melee_damage: String,
    pub third_hit_melee_response: String,
    /// `lunge melee damage` — energy sword.
    pub lunge_melee_damage: String,
    pub lunge_melee_response: String,
    /// `empty melee damage` — energy sword empty-battery variant.
    pub empty_melee_damage: String,
    pub empty_melee_response: String,
    /// `clang melee damage` — energy sword clang.
    pub clang_melee_damage: String,
    pub clang_melee_response: String,
}

impl WeaponMeleeDamageParameters {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            damage_pyramid_angles: s.read_euler2d("damage pyramid angles"),
            damage_pyramid_depth: s.read_real("damage pyramid depth").unwrap_or(0.0),
            first_hit_melee_damage: s.read_tag_ref_path("1st hit melee damage").unwrap_or_default(),
            first_hit_melee_response: s
                .read_tag_ref_path("1st hit melee response")
                .unwrap_or_default(),
            second_hit_melee_damage: s.read_tag_ref_path("2nd hit melee damage").unwrap_or_default(),
            second_hit_melee_response: s
                .read_tag_ref_path("2nd hit melee response")
                .unwrap_or_default(),
            third_hit_melee_damage: s.read_tag_ref_path("3rd hit melee damage").unwrap_or_default(),
            third_hit_melee_response: s
                .read_tag_ref_path("3rd hit melee response")
                .unwrap_or_default(),
            lunge_melee_damage: s.read_tag_ref_path("lunge melee damage").unwrap_or_default(),
            lunge_melee_response: s.read_tag_ref_path("lunge melee response").unwrap_or_default(),
            empty_melee_damage: s.read_tag_ref_path("empty melee damage").unwrap_or_default(),
            empty_melee_response: s.read_tag_ref_path("empty melee response").unwrap_or_default(),
            clang_melee_damage: s.read_tag_ref_path("clang melee damage").unwrap_or_default(),
            clang_melee_response: s.read_tag_ref_path("clang melee response").unwrap_or_default(),
        }
    }
}

/// `aim_assist_struct` (size 48).
#[derive(Debug, Clone, Default)]
pub struct WeaponAimAssist {
    pub autoaim_angle: f32,
    pub autoaim_range: f32,
    pub autoaim_falloff_range: f32,
    pub magnetism_angle: f32,
    pub magnetism_range: f32,
    pub magnetism_falloff_range: f32,
    pub deviation_angle: f32,
}

impl WeaponAimAssist {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            autoaim_angle: s.read_real("autoaim angle").unwrap_or(0.0),
            autoaim_range: s.read_real("autoaim range").unwrap_or(0.0),
            autoaim_falloff_range: s.read_real("autoaim falloff range").unwrap_or(0.0),
            magnetism_angle: s.read_real("magnetism angle").unwrap_or(0.0),
            magnetism_range: s.read_real("magnetism range").unwrap_or(0.0),
            magnetism_falloff_range: s.read_real("magnetism falloff range").unwrap_or(0.0),
            deviation_angle: s.read_real("deviation angle").unwrap_or(0.0),
        }
    }
}

/// `weapon_tracking_struct` (size 4). Engine only authors a single
/// enum here.
#[derive(Debug, Clone, Default)]
pub struct WeaponTracking {
    /// `tracking type` enum (heat-seeking, lock-on, etc.).
    pub tracking_type: Enum<WeaponTrackingType, i16>,
}

impl WeaponTracking {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            tracking_type: s.try_read_enum("tracking type").unwrap_or_default(),
        }
    }
}

/// `weapon_interface_struct` (size 44). Engine substruct is mostly
/// nested data — `shared interface` is pad-only, `first person`
/// block surfaced as count.
#[derive(Debug, Clone, Default)]
pub struct WeaponInterface {
    /// `first person` block — count only. Each entry is a 32-byte
    /// `weapon_first_person_interface_block` (FP weapon-arm model
    /// references). Surface elements when consumers need them.
    pub first_person_count: usize,
    /// `chud interface` (tag_reference).
    pub chud_interface: String,
}

impl WeaponInterface {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        let first_person_count = s
            .field("first person")
            .and_then(|f| f.as_block())
            .map(|b| b.len())
            .unwrap_or(0);
        Self {
            first_person_count,
            chud_interface: s.read_tag_ref_path("chud interface").unwrap_or_default(),
        }
    }
}

/// Walked subset of `weapon_group.magazines[i]` (schema
/// `magazines` struct, size 128, weapon.json:1037-1107).
#[derive(Debug, Clone, Default)]
pub struct WeaponDefinitionMagazine {
    pub flags: Flags<MagazineFlags, u32>,
    pub rounds_recharged_per_second: i16,
    pub rounds_total_initial: i16,
    pub rounds_total_maximum: i16,
    /// Divisor for compute case `primary/secondary_ammunition` —
    /// `_weapon_datum.magazines[i].rounds_loaded /
    /// rounds_loaded_maximum`.
    pub rounds_loaded_maximum: i16,
    pub runtime_rounds_inventory_maximum: i16,
    /// `reload time:seconds`.
    pub reload_time_seconds: f32,
    pub rounds_reloaded: i16,
    /// `chamber time:seconds`.
    pub chamber_time_seconds: f32,
}

/// Walked subset of `weapon_group.new triggers[i]` (schema
/// `weapon_triggers` struct, size 144).
#[derive(Debug, Clone, Default)]
pub struct WeaponDefinitionTrigger {
    pub flags: Flags<WeaponTriggerDefinitionFlags, u32>,
    /// `input` enum — which trigger input drives this (primary/secondary).
    pub input: Enum<WeaponTriggerInput, i16>,
    /// `behavior` enum.
    pub behavior: Enum<WeaponTriggerBehavior, i16>,
    /// `primary barrel` block-index reference.
    pub primary_barrel: i16,
    /// `secondary barrel` block-index reference.
    pub secondary_barrel: i16,
    pub prediction: Enum<TriggerPredictionType, i16>,
    /// `autofire` substruct (`weapon_trigger_autofire_struct`, 12 bytes).
    pub autofire: WeaponDefinitionTriggerAutofire,
    /// `charging` substruct (`weapon_trigger_charging_struct`, 104 bytes)
    /// — `charging_time` gates the trigger-charge contribution to the
    /// `illumination` compute case; `charged_illumination` is the
    /// amplitude of that contribution.
    pub charging: WeaponDefinitionTriggerCharging,
    pub lock_on_hold_time: f32,
    pub lock_on_acquire_time: f32,
    pub lock_on_grace_time: f32,
}

/// Walked `weapon_trigger_autofire_struct` (12 bytes).
#[derive(Debug, Clone, Default)]
pub struct WeaponDefinitionTriggerAutofire {
    /// `autofire time` (seconds).
    pub autofire_time: f32,
    /// `autofire throw` (seconds).
    pub autofire_throw: f32,
    /// `secondary action` enum.
    pub secondary_action: Enum<WeaponTriggerAutofireAction, i16>,
    /// `primary action` enum.
    pub primary_action: Enum<WeaponTriggerAutofireAction, i16>,
}

/// Walked `weapon_trigger_charging_struct` (104 bytes). Fields read
/// by `weapon_compute_function_value` case `illumination` (sid 516):
/// - `charging_time > 0` gates whether the trigger contributes
/// - `charged_illumination` is the per-trigger illumination amplitude
///   blended by the engine's `weapon_trigger_get_charged_fraction`.
#[derive(Debug, Clone, Default)]
pub struct WeaponDefinitionTriggerCharging {
    /// `charging time:seconds` — time to fully charge.
    pub charging_time: f32,
    /// `charged time:seconds` — time before overcharging.
    pub charged_time: f32,
    /// `overcharged action` enum.
    pub overcharged_action: Enum<WeaponTriggerOverchargedAction, i16>,
    /// `cancelled trigger throw`.
    pub cancelled_trigger_throw: i16,
    /// `charged illumination:[0,1]` — illumination when fully charged.
    pub charged_illumination: f32,
    /// `spew time:seconds`.
    pub spew_time: f32,
    /// `charged drain rate` — battery drain per second when charged.
    pub charged_drain_rate: f32,
    // Future: tag_references for charging_effect, charging_damage_effect,
    // charging_continuous_damage_response, discharge_effect,
    // discharge_damage_effect.
}

/// Walked subset of `weapon_group.barrels[i]` (schema
/// `weapon_barrels` struct, size 308).
#[derive(Debug, Clone, Default)]
pub struct WeaponDefinitionBarrel {
    pub flags: Flags<WeaponBarrelFlags, u32>,
    /// `firing` substruct (`weapon_barrel_firing_parameters_struct`,
    /// 36 bytes) — rate-of-fire curve + barrel spin scale.
    pub firing: WeaponDefinitionBarrelFiring,
    /// Magazine block-index this barrel draws from.
    pub magazine: i16,
    pub rounds_per_shot: i16,
    pub minimum_rounds_loaded: i16,
    pub rounds_between_tracers: i16,
    /// `optional barrel marker name` — string_id.
    pub optional_barrel_marker_name: String,
    pub projectiles_per_shot: i16,
    pub distribution_angle_degrees: f32,
    /// `ejection port recovery time` (seconds) — decay rate for the
    /// `primary_ejection_port` compute case's runtime field.
    pub ejection_port_recovery_time: f32,
    /// `illumination recovery time` (seconds) — decay rate for the
    /// per-barrel illumination contribution to the `illumination`
    /// compute case.
    pub illumination_recovery_time: f32,
    /// `heat generated per round:[0,1]`.
    pub heat_generated_per_round: f32,
    /// `age generated per round:[0,1]`.
    pub age_generated_per_round: f32,
    /// `CAMPAIGN age generated per round:[0,1]`.
    pub campaign_age_generated_per_round: f32,
    /// `overload time:seconds` — auto-fire cadence under sustained
    /// trigger hold.
    pub overload_time: f32,
}

/// Walked `weapon_barrel_firing_parameters_struct` (36 bytes).
#[derive(Debug, Clone, Default)]
pub struct WeaponDefinitionBarrelFiring {
    /// `rounds per second` lower bound.
    pub rounds_per_second_min: f32,
    /// `rounds per second` upper bound (the engine ramps between these
    /// based on continuous trigger time).
    pub rounds_per_second_max: f32,
    /// `acceleration time:seconds`.
    pub acceleration_time: f32,
    /// `deceleration time:seconds`.
    pub deceleration_time: f32,
    /// `barrel spin scale:[0,1]`.
    pub barrel_spin_scale: f32,
    /// `blurred rate of fire:[0,1]`.
    pub blurred_rate_of_fire: f32,
    /// `shots per fire` lower bound (short_bounds).
    pub shots_per_fire_min: i16,
    /// `shots per fire` upper bound.
    pub shots_per_fire_max: i16,
    /// `fire recovery time:seconds`.
    pub fire_recovery_time: f32,
    /// `soft recovery fraction:[0,1]`.
    pub soft_recovery_fraction: f32,
}

// ---------------------------------------------------------------------------
// Implementations
// ---------------------------------------------------------------------------

impl WeaponDefinition {
    /// Load a `.weapon` tag and surface the runtime-relevant
    /// authored fields. Errors when `tag.group() != weap` or when
    /// the embedded `item` / `item/object` substruct is missing.
    pub fn from_tag(tag: &TagFile) -> Result<Self, WeaponError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != WEAPON_GROUP {
            return Err(WeaponError::WrongGroup {
                expected: WEAPON_GROUP,
                actual,
            });
        }
        // Reuse the canonical `ObjectDefinition` walker — it descends
        // through the inheritance chain (`weapon/item/object`) for us.
        let object = Arc::new(ObjectDefinition::from_tag(tag)?);
        let root = tag.root();
        let item_struct = root
            .descend("item")
            .ok_or(WeaponError::MissingSubstruct { path: "item" })?;
        let item = Arc::new(ItemDefinition::from_item_struct(object, &item_struct));

        let magazines = root
            .field("magazines")
            .and_then(|f| f.as_block())
            .map(|block| {
                block
                    .iter()
                    .map(|s| WeaponDefinitionMagazine::from_struct(&s))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // Schema field name has a space — "new triggers" (the legacy
        // "triggers" name was retired; `new triggers` is the canonical
        // post-Halo-2 block per weapon.json:1241).
        let triggers = root
            .field("new triggers")
            .and_then(|f| f.as_block())
            .map(|block| {
                block
                    .iter()
                    .map(|s| WeaponDefinitionTrigger::from_struct(&s))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let barrels = root
            .field("barrels")
            .and_then(|f| f.as_block())
            .map(|block| {
                block
                    .iter()
                    .map(|s| WeaponDefinitionBarrel::from_struct(&s))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let predicted_resources_count = root
            .field("predicted resources")
            .and_then(|f| f.as_block())
            .map(|b| b.len())
            .unwrap_or(0);

        let melee_damage_parameters = root
            .field("melee damage parameters")
            .and_then(|f| f.as_struct())
            .map(|sub| WeaponMeleeDamageParameters::from_struct(&sub))
            .unwrap_or_default();
        let aim_assist = root
            .field("weapon aim assist")
            .and_then(|f| f.as_struct())
            .map(|sub| WeaponAimAssist::from_struct(&sub))
            .unwrap_or_default();
        let tracking = root
            .field("tracking")
            .and_then(|f| f.as_struct())
            .map(|sub| WeaponTracking::from_struct(&sub))
            .unwrap_or_default();
        let player_interface = root
            .field("player interface")
            .and_then(|f| f.as_struct())
            .map(|sub| WeaponInterface::from_struct(&sub))
            .unwrap_or_default();

        Ok(Self {
            item,
            flags: root.try_read_flags("flags").unwrap_or_default(),
            secondary_flags: root.try_read_flags("secondary flags").unwrap_or_default(),
            unused_label: root.read_string_id("unused label").unwrap_or_default(),
            secondary_trigger_mode: root.try_read_enum("secondary trigger mode").unwrap_or_default(),
            maximum_alternate_shots_loaded: root
                .read_int_any("maximum alternate shots loaded")
                .unwrap_or(0) as i16,
            turn_on_time: root.read_real("turn on time").unwrap_or(0.0),
            ready_time: root.read_real("ready time").unwrap_or(0.0),
            ready_effect: root.read_tag_ref_path("ready effect").unwrap_or_default(),
            ready_damage_effect: root
                .read_tag_ref_path("ready damage effect")
                .unwrap_or_default(),

            heat_recovery_threshold: root.read_real("heat recovery threshold").unwrap_or(0.0),
            overheated_threshold: root.read_real("overheated threshold").unwrap_or(0.0),
            heat_detonation_threshold: root
                .read_real("heat detonation threshold")
                .unwrap_or(0.0),
            heat_detonation_fraction: root
                .read_real("heat detonation fraction")
                .unwrap_or(0.0),
            heat_loss_per_second: root.read_real("heat loss per second").unwrap_or(0.0),
            heat_illumination: root.read_real("heat illumination").unwrap_or(0.0),
            overheated_heat_loss_per_second: root
                .read_real("overheated heat loss per second")
                .unwrap_or(0.0),

            overheated: root.read_tag_ref_path("overheated").unwrap_or_default(),
            overheated_damage_effect: root
                .read_tag_ref_path("overheated damage effect")
                .unwrap_or_default(),
            detonation: root.read_tag_ref_path("detonation").unwrap_or_default(),
            detonation_damage_effect: root
                .read_tag_ref_path("detonation damage effect")
                .unwrap_or_default(),

            player_melee_damage: root
                .read_tag_ref_path("player melee damage")
                .unwrap_or_default(),
            player_melee_response: root
                .read_tag_ref_path("player melee response")
                .unwrap_or_default(),
            melee_damage_parameters,
            clang_effect: root.read_tag_ref_path("clang effect").unwrap_or_default(),
            melee_damage_reporting_type: root
                .try_read_enum("melee damage reporting type")
                .unwrap_or_default(),

            magnification_levels: root.read_int_any("magnification levels").unwrap_or(0) as i16,
            magnification_range: root.read_real_bounds("magnification range"),
            aim_assist,

            movement_penalized: root.try_read_enum("movement penalized").unwrap_or_default(),
            forward_movement_penalty: root.read_real("forward movement penalty").unwrap_or(0.0),
            sideways_movement_penalty: root.read_real("sideways movement penalty").unwrap_or(0.0),

            ai_scariness: root.read_real("AI scariness").unwrap_or(0.0),

            weapon_power_on_time: root.read_real("weapon power-on time").unwrap_or(0.0),
            weapon_power_off_time: root.read_real("weapon power-off time").unwrap_or(0.0),
            weapon_power_on_effect: root
                .read_tag_ref_path("weapon power-on effect")
                .unwrap_or_default(),
            weapon_power_off_effect: root
                .read_tag_ref_path("weapon power-off effect")
                .unwrap_or_default(),

            age_heat_recovery_penalty: root.read_real("age heat recovery penalty").unwrap_or(0.0),
            age_rate_of_fire_penalty: root.read_real("age rate of fire penalty").unwrap_or(0.0),
            age_misfire_start: root.read_real("age misfire start").unwrap_or(0.0),
            age_misfire_chance: root.read_real("age misfire chance").unwrap_or(0.0),

            pickup_sound: root.read_tag_ref_path("pickup sound").unwrap_or_default(),
            zoom_in_sound: root.read_tag_ref_path("zoom-in sound").unwrap_or_default(),
            zoom_out_sound: root.read_tag_ref_path("zoom-out sound").unwrap_or_default(),

            active_camo_ding: root.read_real("active camo ding").unwrap_or(0.0),
            active_camo_regrowth_rate: root.read_real("active camo regrowth rate").unwrap_or(0.0),

            handle_node: root.read_string_id("handle node").unwrap_or_default(),
            weapon_class: root.read_string_id("weapon class").unwrap_or_default(),
            weapon_name: root.read_string_id("weapon name").unwrap_or_default(),
            multiplayer_weapon_type: root
                .try_read_enum("multiplayer weapon type")
                .unwrap_or_default(),
            weapon_type: root.try_read_enum("weapon type").unwrap_or_default(),
            tracking,
            player_interface,

            predicted_resources_count,
            magazines,
            triggers,
            barrels,

            max_movement_acceleration: root.read_real("max movement acceleration").unwrap_or(0.0),
            max_movement_velocity: root.read_real("max movement velocity").unwrap_or(0.0),
            max_turning_acceleration: root.read_real("max turning acceleration").unwrap_or(0.0),
            max_turning_velocity: root.read_real("max turning velocity").unwrap_or(0.0),

            deployed_vehicle: root.read_tag_ref_path("deployed vehicle").unwrap_or_default(),
            tossed_weapon: root.read_tag_ref_path("tossed weapon").unwrap_or_default(),
            age_effect: root.read_tag_ref_path("age effect").unwrap_or_default(),
            aged_material_effects: root
                .read_tag_ref_path("aged material effects")
                .unwrap_or_default(),

            external_aging_amount: root.read_real("external aging amount").unwrap_or(0.0),
            campaign_external_aging_amount: root
                .read_real("campaign external aging amount")
                .unwrap_or(0.0),

            first_person_weapon_offset: root.read_vec3("first person weapon offset"),
            first_person_weapon_offset_override: root
                .read_vec3("first person weapon offset override"),
            first_person_scope_size: root.read_vec2("first person scope size"),
            support_third_person_camera_range: root
                .read_real_bounds("support third person camera range"),

            weapon_zoom_time: root.read_real("weapon zoom time").unwrap_or(0.0),
            weapon_ready_for_use_time: root.read_real("weapon ready-for-use time").unwrap_or(0.0),
            unit_stow_anchor_name: root.read_string_id("unit stow anchor name").unwrap_or_default(),
        })
    }
}

impl WeaponDefinitionMagazine {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            flags: s.try_read_flags("flags").unwrap_or_default(),
            rounds_recharged_per_second: s
                .read_int_any("rounds recharged")
                .unwrap_or(0) as i16,
            rounds_total_initial: s
                .read_int_any("rounds total initial")
                .unwrap_or(0) as i16,
            rounds_total_maximum: s
                .read_int_any("rounds total maximum")
                .unwrap_or(0) as i16,
            rounds_loaded_maximum: s
                .read_int_any("rounds loaded maximum")
                .unwrap_or(0) as i16,
            runtime_rounds_inventory_maximum: s
                .read_int_any("runtime rounds inventory maximum")
                .unwrap_or(0) as i16,
            reload_time_seconds: s.read_real("reload time").unwrap_or(0.0),
            rounds_reloaded: s.read_int_any("rounds reloaded").unwrap_or(0) as i16,
            chamber_time_seconds: s.read_real("chamber time").unwrap_or(0.0),
        }
    }
}

impl WeaponDefinitionTrigger {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        let autofire = s
            .field("autofire")
            .and_then(|f| f.as_struct())
            .map(|sub| WeaponDefinitionTriggerAutofire::from_struct(&sub))
            .unwrap_or_default();
        let charging = s
            .field("charging")
            .and_then(|f| f.as_struct())
            .map(|sub| WeaponDefinitionTriggerCharging::from_struct(&sub))
            .unwrap_or_default();
        Self {
            flags: s.try_read_flags("flags").unwrap_or_default(),
            input: s.try_read_enum("input").unwrap_or_default(),
            behavior: s.try_read_enum("behavior").unwrap_or_default(),
            primary_barrel: s.read_block_index("primary barrel"),
            secondary_barrel: s.read_block_index("secondary barrel"),
            prediction: s.try_read_enum("prediction").unwrap_or_default(),
            autofire,
            charging,
            lock_on_hold_time: s.read_real("lock-on hold time").unwrap_or(0.0),
            lock_on_acquire_time: s.read_real("lock-on acquire time").unwrap_or(0.0),
            lock_on_grace_time: s.read_real("lock-on grace time").unwrap_or(0.0),
        }
    }
}

impl WeaponDefinitionTriggerAutofire {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            autofire_time: s.read_real("autofire time").unwrap_or(0.0),
            autofire_throw: s.read_real("autofire throw").unwrap_or(0.0),
            secondary_action: s.try_read_enum("secondary action").unwrap_or_default(),
            primary_action: s.try_read_enum("primary action").unwrap_or_default(),
        }
    }
}

impl WeaponDefinitionTriggerCharging {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            charging_time: s.read_real("charging time").unwrap_or(0.0),
            charged_time: s.read_real("charged time").unwrap_or(0.0),
            overcharged_action: s.try_read_enum("overcharged action").unwrap_or_default(),
            cancelled_trigger_throw: s
                .read_int_any("cancelled trigger throw")
                .unwrap_or(0) as i16,
            charged_illumination: s.read_real("charged illumination").unwrap_or(0.0),
            spew_time: s.read_real("spew time").unwrap_or(0.0),
            charged_drain_rate: s.read_real("charged drain rate").unwrap_or(0.0),
        }
    }
}

impl WeaponDefinitionBarrel {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        let firing = s
            .field("firing")
            .and_then(|f| f.as_struct())
            .map(|sub| WeaponDefinitionBarrelFiring::from_struct(&sub))
            .unwrap_or_default();
        Self {
            flags: s.try_read_flags("flags").unwrap_or_default(),
            firing,
            magazine: s.read_block_index("magazine"),
            rounds_per_shot: s.read_int_any("rounds per shot").unwrap_or(0) as i16,
            minimum_rounds_loaded: s
                .read_int_any("minimum rounds loaded")
                .unwrap_or(0) as i16,
            rounds_between_tracers: s
                .read_int_any("rounds between tracers")
                .unwrap_or(0) as i16,
            optional_barrel_marker_name: s
                .read_string_id("optional barrel marker name")
                .unwrap_or_default(),
            projectiles_per_shot: s.read_int_any("projectiles per shot").unwrap_or(0) as i16,
            distribution_angle_degrees: s.read_real("distribution angle").unwrap_or(0.0),
            ejection_port_recovery_time: s
                .read_real("ejection port recovery time")
                .unwrap_or(0.0),
            illumination_recovery_time: s
                .read_real("illumination recovery time")
                .unwrap_or(0.0),
            heat_generated_per_round: s.read_real("heat generated per round").unwrap_or(0.0),
            age_generated_per_round: s.read_real("age generated per round").unwrap_or(0.0),
            campaign_age_generated_per_round: s
                .read_real("CAMPAIGN age generated per round")
                .unwrap_or(0.0),
            overload_time: s.read_real("overload time").unwrap_or(0.0),
        }
    }
}

impl WeaponDefinitionBarrelFiring {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        let rps = s.read_real_bounds("rounds per second");
        let shots = s.read_short_bounds("shots per fire");
        Self {
            rounds_per_second_min: rps.lower,
            rounds_per_second_max: rps.upper,
            acceleration_time: s.read_real("acceleration time").unwrap_or(0.0),
            deceleration_time: s.read_real("deceleration time").unwrap_or(0.0),
            barrel_spin_scale: s.read_real("barrel spin scale").unwrap_or(0.0),
            blurred_rate_of_fire: s.read_real("blurred rate of fire").unwrap_or(0.0),
            shots_per_fire_min: shots.lower,
            shots_per_fire_max: shots.upper,
            fire_recovery_time: s.read_real("fire recovery time").unwrap_or(0.0),
            soft_recovery_fraction: s.read_real("soft recovery fraction").unwrap_or(0.0),
        }
    }
}
