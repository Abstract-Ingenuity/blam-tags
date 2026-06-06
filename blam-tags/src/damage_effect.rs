//! `damage_effect` (`jpt!`) tag walker — area-of-effect damage carrier
//! referenced by effect parts (via `effect_part_definition.reference`
//! with `runtime_tag_reference_base_class_tag = b"jpt!"`) and direct
//! gameplay damage paths.
//!
//! The tag splits into three runtime concerns:
//!
//! 1. **Damage math** (gameplay) — radius, cutoff, AOE core, damage
//!    bounds, cone angles, stun, acceleration, rider transfer scales.
//!    Out of the effect-rendering port's scope; captured for fidelity
//!    so gameplay consumers can read the same values.
//! 2. **Per-player feedback** — `player_responses[]` containing screen
//!    flash, rumble, and sound-effect sub-structs. Consumed by the
//!    runtime player-effects subsystem (Tier 9 of the effects port).
//! 3. **Camera impulse / shake** — temporary camera pose perturbation
//!    parameters (duration, fade, jitter, wobble). Consumed by
//!    `player_effect_apply_camera_effect_matrix`.
//!
//! Schema: `definitions/halo3_mcc/damage_effect.json`.

use crate::api::TagStruct;
use crate::fields::TagFieldData;
use crate::file::TagFile;
use crate::math::{RealArgbColor, RealBounds};
use crate::tag_function::TagFunction;
use crate::typed_enums::{Enum, Flags};

/// `damage_effect_flags` (long_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum DamageEffectFlags {
    #[strum(serialize = "don't scale damage by distance")] DontScaleDamageByDistance = 0,
    #[strum(serialize = "area damage players only")] AreaDamagePlayersOnly = 1,
}

/// `damage_side_effects` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum DamageSideEffect {
    #[default]
    #[strum(serialize = "none")] None = 0,
    #[strum(serialize = "harmless")] Harmless = 1,
    #[strum(serialize = "lethal to the unsuspecting")] LethalToTheUnsuspecting = 2,
    #[strum(serialize = "emp")] Emp = 3,
}

/// `damage_categories` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum DamageCategory {
    #[default]
    #[strum(serialize = "none")] None = 0,
    #[strum(serialize = "falling")] Falling = 1,
    #[strum(serialize = "bullet")] Bullet = 2,
    #[strum(serialize = "grenade")] Grenade = 3,
    #[strum(serialize = "high explosive")] HighExplosive = 4,
    #[strum(serialize = "sniper")] Sniper = 5,
    #[strum(serialize = "melee")] Melee = 6,
    #[strum(serialize = "flame")] Flame = 7,
    #[strum(serialize = "mounted weapon")] MountedWeapon = 8,
    #[strum(serialize = "vehicle")] Vehicle = 9,
    #[strum(serialize = "plasma")] Plasma = 10,
    #[strum(serialize = "needle")] Needle = 11,
    #[strum(serialize = "shotgun")] Shotgun = 12,
}

/// `damage_flags` (long_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum DamageFlags {
    #[strum(serialize = "does not hurt owner")] DoesNotHurtOwner = 0,
    #[strum(serialize = "can cause headshots")] CanCauseHeadshots = 1,
    #[strum(serialize = "pings resistant units")] PingsResistantUnits = 2,
    #[strum(serialize = "does not hurt friends")] DoesNotHurtFriends = 3,
    #[strum(serialize = "does not ping units")] DoesNotPingUnits = 4,
    #[strum(serialize = "detonates explosives")] DetonatesExplosives = 5,
    #[strum(serialize = "only hurts shields")] OnlyHurtsShields = 6,
    #[strum(serialize = "causes flaming death")] CausesFlamingDeath = 7,
    #[strum(serialize = "damage indicators always point down")] DamageIndicatorsAlwaysPointDown = 8,
    #[strum(serialize = "skips shields")] SkipsShields = 9,
    #[strum(serialize = "only hurts one infection form")] OnlyHurtsOneInfectionForm = 10,
    #[strum(serialize = "transfer dmg always uses min")] TransferDmgAlwaysUsesMin = 11,
    #[strum(serialize = "infection form pop")] InfectionFormPop = 12,
    #[strum(serialize = "ignore seat scale for dir. dmg")] IgnoreSeatScaleForDirDmg = 13,
    #[strum(serialize = "forces hard ping if body dmg")] ForcesHardPingIfBodyDmg = 14,
    #[strum(serialize = "does not hurt players")] DoesNotHurtPlayers = 15,
    #[strum(serialize = "does not overcombine")] DoesNotOvercombine = 16,
    #[strum(serialize = "enables special death")] EnablesSpecialDeath = 17,
    #[strum(serialize = "cannot cause betrayals")] CannotCauseBetrayals = 18,
    #[strum(serialize = "uses old EMP behavior")] UsesOldEmpBehavior = 19,
    #[strum(serialize = "ignores damage resistance")] IgnoresDamageResistance = 20,
    #[strum(serialize = "force s_kill on death")] ForceSKillOnDeath = 21,
    #[strum(serialize = "cause magic deceleration")] CauseMagicDeceleration = 22,
}

/// `global_reverse_transition_functions_enum` (short_enum). Owned here;
/// imported by `lens_flare`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum GlobalReverseTransitionFunction {
    #[default]
    #[strum(serialize = "linear")] Linear = 0,
    #[strum(serialize = "late")] Late = 1,
    #[strum(serialize = "very late")] VeryLate = 2,
    #[strum(serialize = "early")] Early = 3,
    #[strum(serialize = "very early")] VeryEarly = 4,
    #[strum(serialize = "cosine")] Cosine = 5,
    #[strum(serialize = "zero")] Zero = 6,
    #[strum(serialize = "one")] One = 7,
}

/// `global_periodic_functions_enum` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum GlobalPeriodicFunction {
    #[default]
    #[strum(serialize = "one")] One = 0,
    #[strum(serialize = "zero")] Zero = 1,
    #[strum(serialize = "cosine")] Cosine = 2,
    #[strum(serialize = "cosine (variable period)")] CosineVariablePeriod = 3,
    #[strum(serialize = "diagonal wave")] DiagonalWave = 4,
    #[strum(serialize = "diagonal wave (variable period)")] DiagonalWaveVariablePeriod = 5,
    #[strum(serialize = "slide")] Slide = 6,
    #[strum(serialize = "slide (variable period)")] SlideVariablePeriod = 7,
    #[strum(serialize = "noise")] Noise = 8,
    #[strum(serialize = "jitter")] Jitter = 9,
    #[strum(serialize = "wander")] Wander = 10,
    #[strum(serialize = "spark")] Spark = 11,
}

/// `damage_effect_player_response_types` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum DamageEffectPlayerResponseType {
    #[default]
    #[strum(serialize = "shielded")] Shielded = 0,
    #[strum(serialize = "unshielded")] Unshielded = 1,
    #[strum(serialize = "all")] All = 2,
}

/// `screen_flash_types` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum ScreenFlashType {
    #[default]
    #[strum(serialize = "none")] None = 0,
    #[strum(serialize = "lighten")] Lighten = 1,
    #[strum(serialize = "darken")] Darken = 2,
    #[strum(serialize = "max")] Max = 3,
    #[strum(serialize = "min")] Min = 4,
    #[strum(serialize = "invert")] Invert = 5,
    #[strum(serialize = "tint")] Tint = 6,
}

/// `screen_flash_priorities` (short_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum ScreenFlashPriority {
    #[default]
    #[strum(serialize = "low")] Low = 0,
    #[strum(serialize = "medium")] Medium = 1,
    #[strum(serialize = "high")] High = 2,
}

const JPT_GROUP: [u8; 4] = *b"jpt!";

#[derive(Debug)]
pub enum DamageEffectError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
}

impl std::fmt::Display for DamageEffectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongGroup { expected, actual } => write!(
                f,
                "damage_effect: wrong tag group (expected {:?}, got {:?})",
                std::str::from_utf8(expected).unwrap_or("????"),
                std::str::from_utf8(actual).unwrap_or("????"),
            ),
        }
    }
}

impl std::error::Error for DamageEffectError {}


/// `screen_flash_definition_struct` — full-screen color blend feedback.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScreenFlash {
    /// `type` (short_enum) — flash kind (linear, lit, max, etc.).
    pub flash_type: Enum<ScreenFlashType, i16>,
    /// `priority` (short_enum) — display order vs other flashes.
    pub priority: Enum<ScreenFlashPriority, i16>,
    /// Total duration in seconds.
    pub duration_seconds: f32,
    /// `fade_function` (short_enum) — `fade_function_enum`.
    pub fade_function: Enum<GlobalReverseTransitionFunction, i16>,
    /// `maximum intensity` (0..1).
    pub maximum_intensity: f32,
    /// `color` (real_argb_color).
    pub color: RealArgbColor,
}

/// One frequency lane of `rumble_definition_struct`. The tag carries
/// two — low and high frequency — composited as the controller motor
/// drive signal.
#[derive(Debug, Clone, Default)]
pub struct RumbleFrequency {
    /// Total rumble duration in seconds.
    pub duration_seconds: f32,
    /// Amplitude envelope curve (`whore function` / `dirty whore`
    /// in schema). `None` when the curve blob is missing.
    pub envelope: Option<TagFunction>,
}

/// `rumble_definition_struct` — controller rumble pair.
#[derive(Debug, Clone, Default)]
pub struct Rumble {
    pub low_frequency: RumbleFrequency,
    pub high_frequency: RumbleFrequency,
}

/// `damage_effect_sound_effect_definition` — one-shot sound played in
/// response to the damage event (separate from the top-level `sound`
/// tag_reference which is the impact sound).
#[derive(Debug, Clone, Default)]
pub struct DamageSoundEffect {
    /// `effect name` — string_id resolved against the sound-effect
    /// scaffolding to pick a per-platform variant.
    pub effect_name: String,
    /// `duration` in seconds (engine clamps to 0 when sound source is
    /// longer than this).
    pub duration_seconds: f32,
    /// `effect scale function` curve — applies per-instance scaling.
    pub scale_function: Option<TagFunction>,
}

/// One `player_responses[]` entry — the engine dispatches by
/// [`PlayerResponse::response_type`] to choose which inner struct
/// (screen_flash / rumble / sound_effect) actually fires.
#[derive(Debug, Clone, Default)]
pub struct PlayerResponse {
    pub response_type: Enum<DamageEffectPlayerResponseType, i16>,
    pub screen_flash: ScreenFlash,
    pub rumble: Rumble,
    pub sound_effect: DamageSoundEffect,
}

/// Temporary camera impulse — fires regardless of player_response on
/// every damage event.
#[derive(Debug, Clone, Default)]
pub struct CameraImpulse {
    /// `impulse duration` in seconds.
    pub duration_seconds: f32,
    /// `fade function` (short_enum).
    pub fade_function: Enum<GlobalReverseTransitionFunction, i16>,
    /// `rotation` in degrees.
    pub rotation_degrees: f32,
    /// `pushback` in world units (forward motion impulse).
    pub pushback: f32,
    /// `jitter` bounds in world units (random per-frame offset).
    pub jitter: RealBounds,
}

/// Camera shake (longer-duration than impulse, oscillating).
#[derive(Debug, Clone, Default)]
pub struct CameraShake {
    /// `shake duration` in seconds.
    pub duration_seconds: f32,
    /// `falloff function` (short_enum) — envelopes amplitude over time.
    pub falloff_function: Enum<GlobalReverseTransitionFunction, i16>,
    /// Random per-frame translation amplitude in world units.
    pub random_translation: f32,
    /// Random per-frame rotation amplitude in degrees.
    pub random_rotation_degrees: f32,
    /// `wobble function` (short_enum) — perturbs frequency / phase.
    pub wobble_function: Enum<GlobalPeriodicFunction, i16>,
    /// `wobble function period` in seconds.
    pub wobble_period_seconds: f32,
    /// `wobble weight` (0..1) — 0 = wobble has no effect.
    pub wobble_weight: f32,
}

/// Breaking impulse applied to dynamics objects.
#[derive(Debug, Clone, Default)]
pub struct BreakingImpulse {
    /// Forward (along impact direction) push.
    pub forward_velocity: f32,
    pub forward_radius: f32,
    pub forward_exponent: f32,
    /// Outward (radial from impact) push.
    pub outward_velocity: f32,
    pub outward_radius: f32,
    pub outward_exponent: f32,
}

/// Walked `damage_effect` (jpt!) tag.
#[derive(Debug, Clone, Default)]
pub struct DamageEffect {
    // ---- damage math (gameplay) ----
    pub radius: RealBounds,
    pub cutoff_scale: f32,
    pub effect_flags: Flags<DamageEffectFlags, u32>,
    pub side_effect: Enum<DamageSideEffect, i16>,
    pub category: Enum<DamageCategory, i16>,
    pub flags: Flags<DamageFlags, u32>,
    pub aoe_core_radius: f32,
    pub damage_lower_bound: f32,
    pub damage_upper_bound: RealBounds,
    pub inner_cone_angle_radians: f32,
    pub outer_cone_angle_radians: f32,
    pub active_camo_damage: f32,
    pub stun: f32,
    pub maximum_stun: f32,
    pub stun_time_seconds: f32,
    pub instantaneous_acceleration: f32,
    pub rider_direct_damage_scale: f32,
    pub rider_maximum_transfer_damage_scale: f32,
    pub rider_minimum_transfer_damage_scale: f32,
    pub general_damage: String,
    pub specific_damage: String,
    pub ai_stun_radius: f32,
    pub ai_stun_bounds: RealBounds,
    pub shake_radius: f32,
    pub emp_radius: f32,

    // ---- per-player feedback (rendering / input) ----
    pub player_responses: Vec<PlayerResponse>,

    // ---- camera (rendering) ----
    pub camera_impulse: CameraImpulse,
    pub camera_shake: CameraShake,

    // ---- sound + dynamics ----
    /// Top-level impact sound tag reference (`sound` field). Distinct
    /// from per-response `sound_effect` entries.
    pub sound: Option<String>,
    pub breaking_impulse: BreakingImpulse,
}

impl DamageEffect {
    pub fn from_tag(tag: &TagFile) -> Result<Self, DamageEffectError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != JPT_GROUP {
            return Err(DamageEffectError::WrongGroup { expected: JPT_GROUP, actual });
        }
        Ok(Self::from_struct(&tag.root()))
    }

    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        // outer cone angle lives inside the "blah" sub-struct (a
        // schema-named opaque wrapper around damage_outer_cone_angle_struct).
        let outer_cone = s
            .field("blah")
            .and_then(|f| f.as_struct())
            .and_then(|inner| inner.read_real("dmg outer cone angle"))
            .unwrap_or(0.0);

        Self {
            radius: s.read_real_bounds("radius"),
            cutoff_scale: s.read_real("cutoff scale").unwrap_or(0.0),
            effect_flags: s.try_read_flags("effect flags").unwrap_or_default(),
            side_effect: s.try_read_enum("side effect").unwrap_or_default(),
            category: s.try_read_enum("category").unwrap_or_default(),
            flags: s.try_read_flags("flags").unwrap_or_default(),
            aoe_core_radius: s.read_real("AOE core radius").unwrap_or(0.0),
            damage_lower_bound: s.read_real("damage lower bound").unwrap_or(0.0),
            damage_upper_bound: s.read_real_bounds("damage upper bound"),
            inner_cone_angle_radians: s.read_real("dmg inner cone angle").unwrap_or(0.0),
            outer_cone_angle_radians: outer_cone,
            active_camo_damage: s.read_real("active camouflage damage").unwrap_or(0.0),
            stun: s.read_real("stun").unwrap_or(0.0),
            maximum_stun: s.read_real("maximum stun").unwrap_or(0.0),
            stun_time_seconds: s.read_real("stun time").unwrap_or(0.0),
            instantaneous_acceleration: s.read_real("instantaneous acceleration").unwrap_or(0.0),
            rider_direct_damage_scale: s.read_real("rider direct damage scale").unwrap_or(0.0),
            rider_maximum_transfer_damage_scale: s.read_real("rider maximum transfer damage scale").unwrap_or(0.0),
            rider_minimum_transfer_damage_scale: s.read_real("rider minimum transfer damage scale").unwrap_or(0.0),
            general_damage: s.read_string_id("general_damage").unwrap_or_default(),
            specific_damage: s.read_string_id("specific_damage").unwrap_or_default(),
            ai_stun_radius: s.read_real("AI stun radius").unwrap_or(0.0),
            ai_stun_bounds: s.read_real_bounds("AI stun bounds"),
            shake_radius: s.read_real("shake radius").unwrap_or(0.0),
            emp_radius: s.read_real("EMP radius").unwrap_or(0.0),

            player_responses: read_player_responses(s),

            camera_impulse: CameraImpulse {
                duration_seconds: s.read_real("impulse duration").unwrap_or(0.0),
                fade_function: s.try_read_enum("fade function").unwrap_or_default(),
                rotation_degrees: s.read_real("rotation").unwrap_or(0.0),
                pushback: s.read_real("pushback").unwrap_or(0.0),
                jitter: s.read_real_bounds("jitter"),
            },

            camera_shake: CameraShake {
                duration_seconds: s.read_real("shake duration").unwrap_or(0.0),
                falloff_function: s.try_read_enum("falloff function").unwrap_or_default(),
                random_translation: s.read_real("random translation").unwrap_or(0.0),
                random_rotation_degrees: s.read_real("random rotation").unwrap_or(0.0),
                wobble_function: s.try_read_enum("wobble function").unwrap_or_default(),
                wobble_period_seconds: s.read_real("wobble function period").unwrap_or(0.0),
                wobble_weight: s.read_real("wobble weight").unwrap_or(0.0),
            },

            sound: s.read_tag_ref_path("sound"),
            breaking_impulse: BreakingImpulse {
                forward_velocity: s.read_real("forward velocity").unwrap_or(0.0),
                forward_radius: s.read_real("forward radius").unwrap_or(0.0),
                forward_exponent: s.read_real("forward exponent").unwrap_or(0.0),
                outward_velocity: s.read_real("outward velocity").unwrap_or(0.0),
                outward_radius: s.read_real("outward radius").unwrap_or(0.0),
                outward_exponent: s.read_real("outward exponent").unwrap_or(0.0),
            },
        }
    }
}

fn read_player_responses(s: &TagStruct<'_>) -> Vec<PlayerResponse> {
    let block = match s.field("player responses").and_then(|f| f.as_block()) {
        Some(b) => b,
        None => return Vec::new(),
    };
    let mut out = Vec::with_capacity(block.len());
    for i in 0..block.len() {
        if let Some(elem) = block.element(i) {
            out.push(PlayerResponse::from_struct(&elem));
        }
    }
    out
}

impl PlayerResponse {
    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        let response_type = s.try_read_enum("response type").unwrap_or_default();
        let screen_flash = s
            .field("screen flash")
            .and_then(|f| f.as_struct())
            .map(|inner| ScreenFlash::from_struct(&inner))
            .unwrap_or_default();
        let rumble = s
            .field("rumble")
            .and_then(|f| f.as_struct())
            .map(|inner| Rumble::from_struct(&inner))
            .unwrap_or_default();
        let sound_effect = s
            .field("sound effect")
            .and_then(|f| f.as_struct())
            .map(|inner| DamageSoundEffect::from_struct(&inner))
            .unwrap_or_default();
        Self { response_type, screen_flash, rumble, sound_effect }
    }
}

impl ScreenFlash {
    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            flash_type: s.try_read_enum("type").unwrap_or_default(),
            priority: s.try_read_enum("priority").unwrap_or_default(),
            duration_seconds: s.read_real("duration").unwrap_or(0.0),
            fade_function: s.try_read_enum("fade function").unwrap_or_default(),
            maximum_intensity: s.read_real("maximum intensity").unwrap_or(0.0),
            color: read_real_argb(s, "color"),
        }
    }
}

impl Rumble {
    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            low_frequency: s
                .field("low frequency rumble")
                .and_then(|f| f.as_struct())
                .map(|inner| RumbleFrequency::from_struct(&inner))
                .unwrap_or_default(),
            high_frequency: s
                .field("high frequency rumble")
                .and_then(|f| f.as_struct())
                .map(|inner| RumbleFrequency::from_struct(&inner))
                .unwrap_or_default(),
        }
    }
}

impl RumbleFrequency {
    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        // The `dirty whore` sub-struct wraps a mapping_function — the
        // schema's compulsively-named indirection layer for curve
        // payloads. Walk through it to reach the data field.
        let envelope = s
            .field("dirty whore")
            .and_then(|f| f.as_struct())
            .and_then(|inner| inner.field("data").and_then(|f| f.as_function()));
        Self {
            duration_seconds: s.read_real("duration").unwrap_or(0.0),
            envelope,
        }
    }
}

fn read_real_argb(s: &TagStruct<'_>, name: &str) -> RealArgbColor {
    match s.field(name).and_then(|f| f.value()) {
        Some(TagFieldData::RealArgbColor(c)) => c,
        _ => RealArgbColor::default(),
    }
}

impl DamageSoundEffect {
    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        let scale_function = s
            .field("effect scale function")
            .and_then(|f| f.as_struct())
            .and_then(|inner| inner.field("data").and_then(|f| f.as_function()));
        Self {
            effect_name: s.read_string_id("effect name").unwrap_or_default(),
            duration_seconds: s.read_real("duration").unwrap_or(0.0),
            scale_function,
        }
    }
}
