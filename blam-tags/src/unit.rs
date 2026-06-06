//! `unit_struct_definition` substruct — shared parent of `.biped`
//! (bipd), `.vehicle` (vehi), and `.giant` (gint).
//!
//! Schema: `definitions/halo3_mcc/unit.json` → `unit_struct_definition`
//! (size 700, parent_tag `obje`).
//! Ares source: `source/units/units.h`.
//!
//! Composition: each derived unit tag's root holds a `unit` substruct
//! which holds an `object` substruct.

use crate::api::TagStruct;
use crate::math::{AngleBounds, Bounds, RealVector3d};
use crate::object::ObjectDefinition;
use crate::typed_enums::{Enum, Flags};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Typed enums / flags (schema-name-resolved). Several are shared with
// `creature` (a unit subtype) and imported there.
// ---------------------------------------------------------------------------

/// `unit_flags` (long_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum UnitFlags {
    #[strum(serialize = "circular aiming")] CircularAiming = 0,
    #[strum(serialize = "destroyed after dying")] DestroyedAfterDying = 1,
    #[strum(serialize = "fires from camera")] FiresFromCamera = 2,
    #[strum(serialize = "doesn't show readied weapon")] DoesntShowReadiedWeapon = 3,
    #[strum(serialize = "causes passenger dialogue")] CausesPassengerDialogue = 4,
    #[strum(serialize = "resists pings")] ResistsPings = 5,
    #[strum(serialize = "melee attack is fatal")] MeleeAttackIsFatal = 6,
    #[strum(serialize = "don't reface during pings")] DontRefaceDuringPings = 7,
    #[strum(serialize = "has no aiming")] HasNoAiming = 8,
    #[strum(serialize = "simple creature")] SimpleCreature = 9,
    #[strum(serialize = "cannot open doors automatically")] CannotOpenDoorsAutomatically = 10,
    #[strum(serialize = "not instantly killed by melee")] NotInstantlyKilledByMelee = 11,
    #[strum(serialize = "flashlight power doesnt transfer to weapon")] FlashlightPowerDoesntTransferToWeapon = 12,
    #[strum(serialize = "top level for AOE damage")] TopLevelForAoeDamage = 13,
    #[strum(serialize = "special cinematic unit")] SpecialCinematicUnit = 14,
    #[strum(serialize = "ignored by autoaiming")] IgnoredByAutoaiming = 15,
    #[strum(serialize = "use velocity as acceleration")] UseVelocityAsAcceleration = 16,
    #[strum(serialize = "acts as gunner for parent")] ActsAsGunnerForParent = 17,
    #[strum(serialize = "controlled by parent gunner")] ControlledByParentGunner = 18,
    #[strum(serialize = "parent's primary weapon")] ParentsPrimaryWeapon = 19,
    #[strum(serialize = "unit has boost")] UnitHasBoost = 20,
    #[strum(serialize = "allow aim while opening or closing")] AllowAimWhileOpeningOrClosing = 21,
}

/// `unit_default_teams`. Shared with `creature`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum UnitDefaultTeam {
    #[default]
    #[strum(serialize = "default")] Default = 0,
    #[strum(serialize = "player")] Player = 1,
    #[strum(serialize = "human")] Human = 2,
    #[strum(serialize = "covenant")] Covenant = 3,
    #[strum(serialize = "flood")] Flood = 4,
    #[strum(serialize = "sentinel")] Sentinel = 5,
    #[strum(serialize = "heretic")] Heretic = 6,
    #[strum(serialize = "prophet")] Prophet = 7,
    #[strum(serialize = "guilty")] Guilty = 8,
    #[strum(serialize = "unused9")] Unused9 = 9,
    #[strum(serialize = "unused10")] Unused10 = 10,
    #[strum(serialize = "unused11")] Unused11 = 11,
    #[strum(serialize = "unused12")] Unused12 = 12,
    #[strum(serialize = "unused13")] Unused13 = 13,
    #[strum(serialize = "unused14")] Unused14 = 14,
    #[strum(serialize = "unused15")] Unused15 = 15,
}

/// `ai_sound_volume_enum`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum AiSoundVolume {
    #[default]
    #[strum(serialize = "silent")] Silent = 0,
    #[strum(serialize = "medium")] Medium = 1,
    #[strum(serialize = "loud")] Loud = 2,
    #[strum(serialize = "shout")] Shout = 3,
    #[strum(serialize = "quiet")] Quiet = 4,
}

/// `global_chud_blip_type_definition`. Shared with `creature`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum GlobalChudBlipType {
    #[default]
    #[strum(serialize = "medium")] Medium = 0,
    #[strum(serialize = "small")] Small = 1,
    #[strum(serialize = "large")] Large = 2,
}

/// `unit_item_owner_size_enum`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum UnitItemOwnerSize {
    #[default]
    #[strum(serialize = "small")] Small = 0,
    #[strum(serialize = "medium")] Medium = 1,
    #[strum(serialize = "player")] Player = 2,
    #[strum(serialize = "large")] Large = 3,
}

/// `global_grenade_type_enum`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum GlobalGrenadeType {
    #[default]
    #[strum(serialize = "human fragmentation")] HumanFragmentation = 0,
    #[strum(serialize = "covenant plasma")] CovenantPlasma = 1,
    #[strum(serialize = "brute claymore")] BruteClaymore = 2,
    #[strum(serialize = "firebomb")] Firebomb = 3,
}

/// `unit_camera_flags_definition` (word_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u16)]
pub enum UnitCameraFlags {
    #[strum(serialize = "pitch bounds absolute space")] PitchBoundsAbsoluteSpace = 0,
}

/// `unit_seat_flags` (long_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum UnitSeatFlags {
    #[strum(serialize = "invisible")] Invisible = 0,
    #[strum(serialize = "locked")] Locked = 1,
    #[strum(serialize = "driver")] Driver = 2,
    #[strum(serialize = "gunner")] Gunner = 3,
    #[strum(serialize = "third person camera")] ThirdPersonCamera = 4,
    #[strum(serialize = "allows weapons")] AllowsWeapons = 5,
    #[strum(serialize = "third person on enter")] ThirdPersonOnEnter = 6,
    #[strum(serialize = "first person camera slaved to gun.")] FirstPersonCameraSlavedToGun = 7,
    #[strum(serialize = "allow vehicle communication animations")] AllowVehicleCommunicationAnimations = 8,
    #[strum(serialize = "not valid without driver")] NotValidWithoutDriver = 9,
    #[strum(serialize = "allow AI noncombatants")] AllowAiNoncombatants = 10,
    #[strum(serialize = "boarding seat")] BoardingSeat = 11,
    #[strum(serialize = "ai firing disabled by max acceleration")] AiFiringDisabledByMaxAcceleration = 12,
    #[strum(serialize = "boarding enters seat")] BoardingEntersSeat = 13,
    #[strum(serialize = "boarding need any passenger")] BoardingNeedAnyPassenger = 14,
    #[strum(serialize = "controls open and close")] ControlsOpenAndClose = 15,
    #[strum(serialize = "invalid for player")] InvalidForPlayer = 16,
    #[strum(serialize = "invalid for non-player")] InvalidForNonPlayer = 17,
    #[strum(serialize = "gunner (player only)")] GunnerPlayerOnly = 18,
    #[strum(serialize = "invisible under major damage")] InvisibleUnderMajorDamage = 19,
    #[strum(serialize = "melee instant killable")] MeleeInstantKillable = 20,
    #[strum(serialize = "leader preference")] LeaderPreference = 21,
    #[strum(serialize = "allows exit and detach")] AllowsExitAndDetach = 22,
    #[strum(serialize = "blocks headshots")] BlocksHeadshots = 23,
    #[strum(serialize = "exits to ground")] ExitsToGround = 24,
    #[strum(serialize = "closes early, opens late")] ClosesEarlyOpensLate = 25,
    #[strum(serialize = "forward from attachment")] ForwardFromAttachment = 26,
    #[strum(serialize = "disallow AI shooting")] DisallowAiShooting = 27,
    #[strum(serialize = "closes early, opens early")] ClosesEarlyOpensEarly = 28,
    #[strum(serialize = "closes late, opens late")] ClosesLateOpensLate = 29,
    #[strum(serialize = "prevents weapon stowing")] PreventsWeaponStowing = 30,
}

/// `global_ai_seat_type_enum`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum GlobalAiSeatType {
    #[default]
    #[strum(serialize = "NONE")] None = 0,
    #[strum(serialize = "passenger")] Passenger = 1,
    #[strum(serialize = "gunner")] Gunner = 2,
    #[strum(serialize = "small cargo")] SmallCargo = 3,
    #[strum(serialize = "large cargo")] LargeCargo = 4,
    #[strum(serialize = "driver")] Driver = 5,
}

/// `campaign_metagame_bucket_flags`. Shared with `creature`/`crate`.
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u8)]
pub enum CampaignMetagameBucketFlags {
    #[strum(serialize = "only counts with riders")] OnlyCountsWithRiders = 0,
}

/// `campaign_metagame_bucket_type_enum`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i8)]
pub enum CampaignMetagameBucketType {
    #[default]
    #[strum(serialize = "brute")] Brute = 0,
    #[strum(serialize = "grunt")] Grunt = 1,
    #[strum(serialize = "jackel")] Jackel = 2,
    #[strum(serialize = "marine")] Marine = 3,
    #[strum(serialize = "bugger")] Bugger = 4,
    #[strum(serialize = "hunter")] Hunter = 5,
    #[strum(serialize = "flood_infection")] FloodInfection = 6,
    #[strum(serialize = "flood_carrier")] FloodCarrier = 7,
    #[strum(serialize = "flood_combat")] FloodCombat = 8,
    #[strum(serialize = "flood_pure")] FloodPure = 9,
    #[strum(serialize = "sentinel")] Sentinel = 10,
    #[strum(serialize = "elite")] Elite = 11,
    #[strum(serialize = "turret")] Turret = 12,
    #[strum(serialize = "mongoose")] Mongoose = 13,
    #[strum(serialize = "warthog")] Warthog = 14,
    #[strum(serialize = "scorpion")] Scorpion = 15,
    #[strum(serialize = "hornet")] Hornet = 16,
    #[strum(serialize = "pelican")] Pelican = 17,
    #[strum(serialize = "shade")] Shade = 18,
    #[strum(serialize = "watchtower")] Watchtower = 19,
    #[strum(serialize = "ghost")] Ghost = 20,
    #[strum(serialize = "chopper")] Chopper = 21,
    #[strum(serialize = "mauler")] Mauler = 22,
    #[strum(serialize = "wraith")] Wraith = 23,
    #[strum(serialize = "banshee")] Banshee = 24,
    #[strum(serialize = "phantom")] Phantom = 25,
    #[strum(serialize = "scarab")] Scarab = 26,
    #[strum(serialize = "guntower")] Guntower = 27,
}

/// `campaign_metagame_bucket_class_enum`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i8)]
pub enum CampaignMetagameBucketClass {
    #[default]
    #[strum(serialize = "infantry")] Infantry = 0,
    #[strum(serialize = "leader")] Leader = 1,
    #[strum(serialize = "hero")] Hero = 2,
    #[strum(serialize = "specialist")] Specialist = 3,
    #[strum(serialize = "light vehicle")] LightVehicle = 4,
    #[strum(serialize = "heavy vehicle")] HeavyVehicle = 5,
    #[strum(serialize = "giant vehicle")] GiantVehicle = 6,
    #[strum(serialize = "standard vehicle")] StandardVehicle = 7,
}

// ---------------------------------------------------------------------------
// Sub-struct + block element types (schema-ordered)
// ---------------------------------------------------------------------------

/// `unit_camera_struct` (size 60). Field order matches schema verbatim.
/// Deeper `camera tracks` / `camera acceleration` blocks are
/// surfaced as count-only — their element structs (camera tracks,
/// camera-acceleration-displacement functions) defer until consumers
/// need them.
#[derive(Debug, Clone, Default)]
pub struct UnitCamera {
    pub flags: Flags<UnitCameraFlags, u16>,
    pub camera_marker_name: String,
    pub camera_submerged_marker_name: String,
    pub pitch_auto_level: f32,
    pub pitch_range: AngleBounds,
    pub camera_tracks_count: usize,
    pub pitch_minimum_spring: f32,
    pub pitch_maximum_spring: f32,
    pub spring_velocity: f32,
    pub camera_acceleration_count: usize,
}

impl UnitCamera {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        let camera_tracks_count = s
            .field("camera tracks")
            .and_then(|f| f.as_block())
            .map(|b| b.len())
            .unwrap_or(0);
        let camera_acceleration_count = s
            .field("camera acceleration")
            .and_then(|f| f.as_block())
            .map(|b| b.len())
            .unwrap_or(0);
        Self {
            flags: s.try_read_flags("flags").unwrap_or_default(),
            camera_marker_name: s.read_string_id("camera marker name").unwrap_or_default(),
            camera_submerged_marker_name: s
                .read_string_id("camera submerged marker name")
                .unwrap_or_default(),
            pitch_auto_level: s.read_real("pitch auto-level").unwrap_or(0.0),
            pitch_range: s.read_angle_bounds("pitch range"),
            camera_tracks_count,
            pitch_minimum_spring: s.read_real("pitch minimum spring").unwrap_or(0.0),
            // Schema typo `mmaximum` — preserved per source-of-truth.
            pitch_maximum_spring: s.read_real("pitch mmaximum spring").unwrap_or(0.0),
            spring_velocity: s.read_real("spring velocity").unwrap_or(0.0),
            camera_acceleration_count,
        }
    }
}

/// `unit_seat_acceleration_struct` (size 20).
#[derive(Debug, Clone, Default)]
pub struct UnitSeatAcceleration {
    /// `acceleration range:world units per second squared` (Vec3).
    pub acceleration_range: RealVector3d,
    pub accel_action_scale: f32,
    pub accel_attach_scale: f32,
}

impl UnitSeatAcceleration {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            acceleration_range: s.read_vec3("acceleration range"),
            accel_action_scale: s.read_real("accel action scale").unwrap_or(0.0),
            accel_attach_scale: s.read_real("accel attach scale").unwrap_or(0.0),
        }
    }
}

/// `unit_additional_node_names_struct` (size 4).
#[derive(Debug, Clone, Default)]
pub struct UnitAdditionalNodeNames {
    /// `preferred_gun_node` (string_id).
    pub preferred_gun_node: String,
}

impl UnitAdditionalNodeNames {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            preferred_gun_node: s.read_string_id("preferred_gun_node").unwrap_or_default(),
        }
    }
}

/// `unit_boarding_melee_struct` (size 112) — 7 melee tag refs.
#[derive(Debug, Clone, Default)]
pub struct UnitBoardingMelee {
    pub boarding_melee_damage: String,
    pub boarding_melee_response: String,
    pub eviction_melee_damage: String,
    pub eviction_melee_response: String,
    pub landing_melee_damage: String,
    pub flurry_melee_damage: String,
    pub obstacle_smash_damage: String,
}

impl UnitBoardingMelee {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            boarding_melee_damage: s.read_tag_ref_path("boarding melee damage").unwrap_or_default(),
            boarding_melee_response: s
                .read_tag_ref_path("boarding melee response")
                .unwrap_or_default(),
            eviction_melee_damage: s
                .read_tag_ref_path("eviction melee damage")
                .unwrap_or_default(),
            eviction_melee_response: s
                .read_tag_ref_path("eviction melee response")
                .unwrap_or_default(),
            landing_melee_damage: s
                .read_tag_ref_path("landing melee damage")
                .unwrap_or_default(),
            flurry_melee_damage: s
                .read_tag_ref_path("flurry melee damage")
                .unwrap_or_default(),
            obstacle_smash_damage: s
                .read_tag_ref_path("obstacle smash damage")
                .unwrap_or_default(),
        }
    }
}

/// `unit_boost_struct` (size 36).
#[derive(Debug, Clone, Default)]
pub struct UnitBoost {
    pub boost_collision_damage: String,
    pub boost_peak_power: f32,
    pub boost_rise_power: f32,
    pub boost_peak_time: f32,
    pub boost_fall_power: f32,
    pub dead_time: f32,
}

impl UnitBoost {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            boost_collision_damage: s
                .read_tag_ref_path("boost collision damage")
                .unwrap_or_default(),
            boost_peak_power: s.read_real("boost peak power").unwrap_or(0.0),
            boost_rise_power: s.read_real("boost rise power").unwrap_or(0.0),
            boost_peak_time: s.read_real("boost peak time").unwrap_or(0.0),
            boost_fall_power: s.read_real("boost fall power").unwrap_or(0.0),
            dead_time: s.read_real("dead time").unwrap_or(0.0),
        }
    }
}

/// `powered_seat_block` (size 8) entry.
#[derive(Debug, Clone, Default)]
pub struct UnitPoweredSeat {
    pub driver_powerup_time: f32,
    pub driver_powerdown_time: f32,
}

impl UnitPoweredSeat {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            driver_powerup_time: s.read_real("driver powerup time").unwrap_or(0.0),
            driver_powerdown_time: s.read_real("driver powerdown time").unwrap_or(0.0),
        }
    }
}

/// `unit_weapon_block` (size 16) entry — single tag_reference.
#[derive(Debug, Clone, Default)]
pub struct UnitWeaponEntry {
    pub weapon: String,
}

impl UnitWeaponEntry {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            weapon: s.read_tag_ref_path("weapon").unwrap_or_default(),
        }
    }
}

/// `unit_hud_reference_block` (size 16) entry.
#[derive(Debug, Clone, Default)]
pub struct UnitHudReference {
    pub chud_interface: String,
}

impl UnitHudReference {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            chud_interface: s.read_tag_ref_path("chud interface").unwrap_or_default(),
        }
    }
}

/// `unit_seat_block` (size 212) — the largest unit block element.
/// Embeds a nested `unit_camera_struct` and a `unit_hud_reference_block`.
#[derive(Debug, Clone, Default)]
pub struct UnitSeat {
    pub flags: Flags<UnitSeatFlags, u32>,
    /// `label^` (old_string_id).
    pub label: String,
    pub marker_name: String,
    pub entry_markers_name: String,
    pub boarding_grenade_marker: String,
    pub boarding_grenade_string: String,
    pub boarding_melee_string: String,
    pub in_seat_string: String,
    pub ping_scale: f32,
    pub turnover_time: f32,
    pub acceleration: UnitSeatAcceleration,
    pub ai_scariness: f32,
    pub ai_seat_type: Enum<GlobalAiSeatType, i16>,
    pub boarding_seat: i16,
    pub listener_interpolation_factor: f32,
    pub yaw_rate_bounds: Bounds<f32>,
    pub pitch_rate_bounds: Bounds<f32>,
    pub pitch_interpolation_time: f32,
    pub min_speed_reference: f32,
    pub max_speed_reference: f32,
    pub speed_exponent: f32,
    pub camera: UnitCamera,
    pub hud_interface: Vec<UnitHudReference>,
    pub enter_seat_string: String,
    pub yaw_minimum: f32,
    pub yaw_maximum: f32,
    pub entry_radius: f32,
    pub entry_marker_cone_angle: f32,
    pub entry_marker_facing_angle: f32,
    pub maximum_relative_velocity: f32,
    pub invisible_seat_region: String,
    /// `runtime invisible seat region index*` — runtime field, kept
    /// for layout completeness.
    pub runtime_invisible_seat_region_index: i32,
}

impl UnitSeat {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        let acceleration = s
            .field("acceleration")
            .and_then(|f| f.as_struct())
            .map(|sub| UnitSeatAcceleration::from_struct(&sub))
            .unwrap_or_default();
        let camera = s
            .field("unit camera")
            .and_then(|f| f.as_struct())
            .map(|sub| UnitCamera::from_struct(&sub))
            .unwrap_or_default();
        let hud_interface = read_block_vec(s, "unit hud interface", UnitHudReference::from_struct);
        Self {
            flags: s.try_read_flags("flags").unwrap_or_default(),
            label: s.read_string_id("label").unwrap_or_default(),
            marker_name: s.read_string_id("marker name").unwrap_or_default(),
            entry_markers_name: s.read_string_id("entry marker(s) name").unwrap_or_default(),
            boarding_grenade_marker: s
                .read_string_id("boarding grenade marker")
                .unwrap_or_default(),
            boarding_grenade_string: s
                .read_string_id("boarding grenade string")
                .unwrap_or_default(),
            boarding_melee_string: s.read_string_id("boarding melee string").unwrap_or_default(),
            in_seat_string: s.read_string_id("in-seat string").unwrap_or_default(),
            ping_scale: s.read_real("ping scale").unwrap_or(0.0),
            turnover_time: s.read_real("turnover time").unwrap_or(0.0),
            acceleration,
            ai_scariness: s.read_real("AI scariness").unwrap_or(0.0),
            ai_seat_type: s.read_enum("ai seat type"),
            boarding_seat: s.read_block_index("boarding seat"),
            listener_interpolation_factor: s
                .read_real("listener interpolation factor")
                .unwrap_or(0.0),
            yaw_rate_bounds: s.read_real_bounds("yaw rate bounds"),
            pitch_rate_bounds: s.read_real_bounds("pitch rate bounds"),
            pitch_interpolation_time: s.read_real("pitch interpolation time").unwrap_or(0.0),
            min_speed_reference: s.read_real("min speed reference").unwrap_or(0.0),
            max_speed_reference: s.read_real("max speed reference").unwrap_or(0.0),
            speed_exponent: s.read_real("speed exponent").unwrap_or(0.0),
            camera,
            hud_interface,
            enter_seat_string: s.read_string_id("enter seat string").unwrap_or_default(),
            yaw_minimum: s.read_real("yaw minimum").unwrap_or(0.0),
            yaw_maximum: s.read_real("yaw maximum").unwrap_or(0.0),
            entry_radius: s.read_real("entry radius").unwrap_or(0.0),
            entry_marker_cone_angle: s.read_real("entry marker cone angle").unwrap_or(0.0),
            entry_marker_facing_angle: s.read_real("entry marker facing angle").unwrap_or(0.0),
            maximum_relative_velocity: s.read_real("maximum relative velocity").unwrap_or(0.0),
            invisible_seat_region: s.read_string_id("invisible seat region").unwrap_or_default(),
            runtime_invisible_seat_region_index: s
                .read_int_any("runtime invisible seat region index")
                .unwrap_or(0) as i32,
        }
    }
}

/// `campaign_metagame_bucket_block` entry (size 8).
#[derive(Debug, Clone, Default)]
pub struct CampaignMetagameBucket {
    pub flags: Flags<CampaignMetagameBucketFlags, u8>,
    pub bucket_type: Enum<CampaignMetagameBucketType, i8>,
    pub bucket_class: Enum<CampaignMetagameBucketClass, i8>,
    pub point_count: i16,
}

impl CampaignMetagameBucket {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            flags: s.try_read_flags("flags").unwrap_or_default(),
            bucket_type: s.read_enum("type"),
            bucket_class: s.read_enum("class"),
            point_count: s.read_int_any("point count").unwrap_or(0) as i16,
        }
    }
}

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
// UnitDefinition
// ---------------------------------------------------------------------------

/// Walked `unit_struct_definition`. Field order matches schema verbatim.
#[derive(Debug, Clone, Default)]
pub struct UnitDefinition {
    pub object: Arc<ObjectDefinition>,
    pub flags: Flags<UnitFlags, u32>,
    pub default_team: Enum<UnitDefaultTeam, i16>,
    pub constant_sound_volume: Enum<AiSoundVolume, i16>,
    pub campaign_metagame_bucket: Vec<CampaignMetagameBucket>,
    pub camera_field_of_view: f32,
    pub unit_camera: UnitCamera,
    pub acceleration: UnitSeatAcceleration,
    pub soft_ping_threshold: f32,
    pub soft_ping_interrupt_time: f32,
    pub hard_ping_threshold: f32,
    pub hard_ping_interrupt_time: f32,
    pub hard_death_threshold: f32,
    pub distance_of_dive_anim: f32,
    pub spawned_turret_character: String,
    pub aiming_velocity_maximum: f32,
    pub aiming_acceleration_maximum: f32,
    pub casual_aiming_modifier: f32,
    pub looking_velocity_maximum: f32,
    pub looking_acceleration_maximum: f32,
    pub right_hand_node: String,
    pub left_hand_node: String,
    pub more_damn_nodes: UnitAdditionalNodeNames,
    pub melee_damage: String,
    pub your_momma: UnitBoardingMelee,
    pub motion_sensor_blip_size: Enum<GlobalChudBlipType, i16>,
    pub item_owner_size: Enum<UnitItemOwnerSize, i16>,
    pub new_hud_interfaces: Vec<UnitHudReference>,
    pub grenade_velocity: f32,
    pub grenade_type: Enum<GlobalGrenadeType, i16>,
    pub grenade_count: i16,
    pub powered_seats: Vec<UnitPoweredSeat>,
    pub weapons: Vec<UnitWeaponEntry>,
    pub seats: Vec<UnitSeat>,
    pub emp_disabled_time: f32,
    pub emp_disabled_effect: String,
    pub boost: UnitBoost,
    pub exit_and_detach_damage: String,
    pub exit_and_detach_weapon: String,
}

impl UnitDefinition {
    pub fn from_unit_struct(
        object: Arc<ObjectDefinition>,
        s: &TagStruct<'_>,
    ) -> Self {
        let unit_camera = s
            .field("unit camera")
            .and_then(|f| f.as_struct())
            .map(|sub| UnitCamera::from_struct(&sub))
            .unwrap_or_default();
        let acceleration = s
            .field("acceleration")
            .and_then(|f| f.as_struct())
            .map(|sub| UnitSeatAcceleration::from_struct(&sub))
            .unwrap_or_default();
        let more_damn_nodes = s
            .field("more damn nodes")
            .and_then(|f| f.as_struct())
            .map(|sub| UnitAdditionalNodeNames::from_struct(&sub))
            .unwrap_or_default();
        let your_momma = s
            .field("your momma")
            .and_then(|f| f.as_struct())
            .map(|sub| UnitBoardingMelee::from_struct(&sub))
            .unwrap_or_default();
        let boost = s
            .field("boost")
            .and_then(|f| f.as_struct())
            .map(|sub| UnitBoost::from_struct(&sub))
            .unwrap_or_default();

        Self {
            object,
            flags: s.try_read_flags("flags").unwrap_or_default(),
            default_team: s.read_enum("default team"),
            constant_sound_volume: s.read_enum("constant sound volume"),
            campaign_metagame_bucket: read_block_vec(
                s,
                "campaign metagame bucket",
                CampaignMetagameBucket::from_struct,
            ),
            camera_field_of_view: s.read_real("camera field of view").unwrap_or(0.0),
            unit_camera,
            acceleration,
            soft_ping_threshold: s.read_real("soft ping threshold").unwrap_or(0.0),
            soft_ping_interrupt_time: s.read_real("soft ping interrupt time").unwrap_or(0.0),
            hard_ping_threshold: s.read_real("hard ping threshold").unwrap_or(0.0),
            hard_ping_interrupt_time: s.read_real("hard ping interrupt time").unwrap_or(0.0),
            hard_death_threshold: s.read_real("hard death threshold").unwrap_or(0.0),
            distance_of_dive_anim: s.read_real("distance of dive anim").unwrap_or(0.0),
            spawned_turret_character: s
                .read_tag_ref_path("spawned turret character")
                .unwrap_or_default(),
            aiming_velocity_maximum: s.read_real("aiming velocity maximum").unwrap_or(0.0),
            aiming_acceleration_maximum: s
                .read_real("aiming acceleration maximum")
                .unwrap_or(0.0),
            casual_aiming_modifier: s.read_real("casual aiming modifier").unwrap_or(0.0),
            looking_velocity_maximum: s.read_real("looking velocity maximum").unwrap_or(0.0),
            looking_acceleration_maximum: s
                .read_real("looking acceleration maximum")
                .unwrap_or(0.0),
            right_hand_node: s.read_string_id("right_hand_node").unwrap_or_default(),
            left_hand_node: s.read_string_id("left_hand_node").unwrap_or_default(),
            more_damn_nodes,
            melee_damage: s.read_tag_ref_path("melee damage").unwrap_or_default(),
            your_momma,
            motion_sensor_blip_size: s.read_enum("motion sensor blip size"),
            item_owner_size: s.read_enum("item owner size"),
            new_hud_interfaces: read_block_vec(
                s,
                "NEW HUD INTERFACES",
                UnitHudReference::from_struct,
            ),
            grenade_velocity: s.read_real("grenade velocity").unwrap_or(0.0),
            grenade_type: s.read_enum("grenade type"),
            grenade_count: s.read_int_any("grenade count").unwrap_or(0) as i16,
            powered_seats: read_block_vec(s, "powered seats", UnitPoweredSeat::from_struct),
            weapons: read_block_vec(s, "weapons", UnitWeaponEntry::from_struct),
            seats: read_block_vec(s, "seats", UnitSeat::from_struct),
            emp_disabled_time: s.read_real("emp disabled time").unwrap_or(0.0),
            emp_disabled_effect: s.read_tag_ref_path("emp disabled effect").unwrap_or_default(),
            boost,
            exit_and_detach_damage: s
                .read_tag_ref_path("exit and detach damage")
                .unwrap_or_default(),
            exit_and_detach_weapon: s
                .read_tag_ref_path("exit and detach weapon")
                .unwrap_or_default(),
        }
    }
}
