//! `.biped` (`bipd`) tag walker.
//!
//! Schema: `definitions/halo3_mcc/biped.json` → `biped_group` (size
//! 1168, parent_tag `unit`).
//! Ares source: `source/units/bipeds.h`.
//!
//! Surfaces the authored fields read by
//! `biped_compute_function_value` (currently
//! `physics.flying_physics.max_velocity` for the `flying_speed` case).

use crate::api::TagStruct;
use crate::file::TagFile;
use crate::math::{AngleBounds, Bounds};
use crate::object::ObjectDefinition;
use crate::typed_enums::Flags;
use crate::unit::UnitDefinition;
use std::sync::Arc;

const BIPED_GROUP: [u8; 4] = *b"bipd";

/// `biped_definition_flags` (long_flags). `unusedN` bits are
/// schema-hidden (`!`) and not author-meaningful.
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum BipedDefinitionFlags {
    #[strum(serialize = "turns without animating")] TurnsWithoutAnimating = 0,
    #[strum(serialize = "unused4")] Unused4 = 1,
    #[strum(serialize = "immune to falling damage")] ImmuneToFallingDamage = 2,
    #[strum(serialize = "unused0")] Unused0 = 3,
    #[strum(serialize = "unused1")] Unused1 = 4,
    #[strum(serialize = "unused2")] Unused2 = 5,
    #[strum(serialize = "random speed increase")] RandomSpeedIncrease = 6,
    #[strum(serialize = "unused3")] Unused3 = 7,
    #[strum(serialize = "spawn death children on destroy")] SpawnDeathChildrenOnDestroy = 8,
    #[strum(serialize = "stunned by emp damage")] StunnedByEmpDamage = 9,
    #[strum(serialize = "dead physics when stunned")] DeadPhysicsWhenStunned = 10,
    #[strum(serialize = "always ragdoll when dead")] AlwaysRagdollWhenDead = 11,
    #[strum(serialize = "snaps turns")] SnapsTurns = 12,
}

/// `character_physics_flags` (long_flags). Shared by biped + creature
/// (both hang `character_physics_struct` off their root).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum CharacterPhysicsFlags {
    #[strum(serialize = "centered_at_origin")] CenteredAtOrigin = 0,
    #[strum(serialize = "shape spherical")] ShapeSpherical = 1,
    #[strum(serialize = "use player physics")] UsePlayerPhysics = 2,
    #[strum(serialize = "climb any surface")] ClimbAnySurface = 3,
    #[strum(serialize = "flying")] Flying = 4,
    #[strum(serialize = "not physical")] NotPhysical = 5,
    #[strum(serialize = "dead character collision group")] DeadCharacterCollisionGroup = 6,
    #[strum(serialize = "suppress ground planes on bipeds")] SuppressGroundPlanesOnBipeds = 7,
}

/// `flying_physics_flags` (long_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum FlyingPhysicsFlags {
    #[strum(serialize = "use world up")] UseWorldUp = 0,
}

/// `biped_lock_on_flags_definition` (long_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum BipedLockOnFlags {
    #[strum(serialize = "locked by human targeting")] LockedByHumanTargeting = 0,
    #[strum(serialize = "locked by plasma targeting")] LockedByPlasmaTargeting = 1,
    #[strum(serialize = "always locked by plasma targeting")] AlwaysLockedByPlasmaTargeting = 2,
}

/// `biped_leap_flags_definition` (long_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum BipedLeapFlags {
    #[strum(serialize = "force early roll")] ForceEarlyRoll = 0,
}

#[derive(Debug)]
pub enum BipedError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
    MissingSubstruct { path: &'static str },
    ObjectDefinition(crate::object::ObjectDefinitionError),
}

impl std::fmt::Display for BipedError {
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

impl std::error::Error for BipedError {}

impl From<crate::object::ObjectDefinitionError> for BipedError {
    fn from(e: crate::object::ObjectDefinitionError) -> Self {
        Self::ObjectDefinition(e)
    }
}

// ---------------------------------------------------------------------------
// Physics substructs
// ---------------------------------------------------------------------------

/// Walked `character_physics_flying_struct` (48 bytes). Used by the
/// `flying_speed` compute case (sid 570) — engine divides
/// `sqrt(|translational_velocity|²)` by `max_velocity`.
#[derive(Debug, Clone, Default)]
pub struct BipedFlyingPhysics {
    /// `bank angle:degrees`.
    pub bank_angle: f32,
    pub bank_apply_time: f32,
    pub bank_decay_time: f32,
    pub pitch_ratio: f32,
    /// `max velocity:world units per second` — divisor for compute
    /// case `flying_speed`.
    pub max_velocity: f32,
    pub max_sidestep_velocity: f32,
    pub acceleration: f32,
    pub deceleration: f32,
    pub angular_velocity_maximum: f32,
    pub angular_acceleration_maximum: f32,
    /// `crouch velocity modifier:[0,1]`.
    pub crouch_velocity_modifier: f32,
    pub flags: Flags<FlyingPhysicsFlags, u32>,
}

/// Walked `character_physics_ground_struct` (64 bytes). All
/// authored fields surfaced; the engine-populated `runtime_*`
/// slopes are recomputed at load and intentionally omitted.
#[derive(Debug, Clone, Default)]
pub struct BipedGroundPhysics {
    pub maximum_slope_angle: f32,
    pub downhill_falloff_angle: f32,
    pub downhill_cutoff_angle: f32,
    pub uphill_falloff_angle: f32,
    pub uphill_cutoff_angle: f32,
    pub downhill_velocity_scale: f32,
    pub uphill_velocity_scale: f32,
    /// `climb inflection angle` — angle at which biped climb
    /// direction switches between up/down.
    pub climb_inflection_angle: f32,
    pub scale_airborne_reaction_time: f32,
    pub scale_ground_adhesion_velocity: f32,
    pub gravity_scale: f32,
}

/// Walked `biped_lock_on_data_struct` (size 8).
#[derive(Debug, Clone, Default)]
pub struct BipedLockOnData {
    pub flags: Flags<BipedLockOnFlags, u32>,
    pub lock_on_distance: f32,
}

/// Walked `biped_leaping_data_struct` (size 48).
#[derive(Debug, Clone, Default)]
pub struct BipedLeapingData {
    pub leap_flags: Flags<BipedLeapFlags, u32>,
    /// `dampening scale:[0,1]` — 1 = very slow changes.
    pub dampening_scale: f32,
    /// `roll delay:[0,1]` — 1 = roll fast and late.
    pub roll_delay: f32,
    /// `cannonball off-axis scale:[0,1]` weight.
    pub cannonball_off_axis_scale: f32,
    /// `cannonball off-track scale:[0,1]` weight.
    pub cannonball_off_track_scale: f32,
    /// `cannonball roll bounds:degrees per second`.
    pub cannonball_roll_bounds: AngleBounds,
    /// `anticipation ratio bounds`.
    pub anticipation_ratio_bounds: Bounds<f32>,
    /// `reaction force bounds:units per second`.
    pub reaction_force_bounds: Bounds<f32>,
    /// `lobbing desire`. 1 = heavy arc, 0 = no arc.
    pub lobbing_desire: f32,
}

/// Walked `biped_ground_fitting_data_struct` (size 48).
#[derive(Debug, Clone, Default)]
pub struct BipedGroundFittingData {
    pub ground_normal_dampening: f32,
    pub root_offset_max_scale: f32,
    pub root_offset_dampening: f32,
    pub following_cam_scale: f32,
    pub root_leaning_scale: f32,
    /// `foot roll max:degrees`.
    pub foot_roll_max: f32,
    /// `foot pitch max:degrees`.
    pub foot_pitch_max: f32,
    pub pivot_on_foot_scale: f32,
    pub pivot_min_foot_delta: f32,
    pub pivot_stride_length_scale: f32,
    pub pivot_throttle_scale: f32,
    pub pivot_offset_dampening: f32,
}

/// Walked `character_physics_struct` (180 bytes). Field order
/// mirrors the schema. The three on-disk shape blocks
/// (`dead sphere shapes`, `pill shapes`, `sphere shapes`) are
/// surfaced as counts — the actual capsule/sphere bodies are
/// consumed directly by havok at load and aren't needed by
/// `*_compute_function_value`.
#[derive(Debug, Clone, Default)]
pub struct BipedPhysics {
    pub flags: Flags<CharacterPhysicsFlags, u32>,
    pub height_standing: f32,
    pub height_crouching: f32,
    pub radius: f32,
    pub mass: f32,
    pub living_material_name: String,
    pub dead_material_name: String,
    pub runtime_global_material_type: i16,
    pub runtime_dead_global_material_type: i16,
    pub dead_sphere_shape_count: usize,
    pub pill_shape_count: usize,
    pub sphere_shape_count: usize,
    pub ground: BipedGroundPhysics,
    pub flying: BipedFlyingPhysics,
}

// ---------------------------------------------------------------------------
// BipedDefinition
// ---------------------------------------------------------------------------

/// Walked `biped_group` (size 1168). Wraps an `Arc<UnitDefinition>`
/// parent + biped-specific authored fields.
#[derive(Debug, Clone, Default)]
pub struct BipedDefinition {
    pub unit: Arc<UnitDefinition>,

    /// `moving turning speed:degrees per second`.
    pub moving_turning_speed: f32,
    /// `flags` (long_flags) — biped-specific.
    pub flags: Flags<BipedDefinitionFlags, u32>,
    /// `stationary turning threshold` (angle).
    pub stationary_turning_threshold: f32,

    // -- jumping/landing block --
    pub jump_velocity: f32,
    pub maximum_soft_landing_time: f32,
    pub maximum_hard_landing_time: f32,
    pub minimum_soft_landing_velocity: f32,
    pub minimum_hard_landing_velocity: f32,
    pub maximum_hard_landing_velocity: f32,
    pub death_hard_landing_velocity: f32,
    pub stun_duration: f32,

    // -- camera --
    pub standing_camera_height: f32,
    pub crouching_camera_height: f32,
    pub crouch_transition_time: f32,
    /// `camera interpolation start:degrees`.
    pub camera_interpolation_start: f32,
    /// `camera interpolation end:degrees`.
    pub camera_interpolation_end: f32,
    pub camera_forward_movement_scale: f32,
    pub camera_side_movement_scale: f32,
    pub camera_vertical_movement_scale: f32,
    pub camera_exclusion_distance: f32,
    pub autoaim_width: f32,

    /// `lock-on data` substruct.
    pub lock_on_data: BipedLockOnData,

    // -- runtime / authored physics+aim ratios --
    pub runtime_physics_control_node_index: i16,
    pub runtime_cosine_stationary_turning_threshold: f32,
    pub runtime_crouch_transition_velocity: f32,
    pub runtime_pelvis_node_index: i16,
    pub runtime_head_node_index: i16,
    pub head_shot_acc_scale: f32,
    pub area_damage_effect: String,

    /// `physics` substruct.
    pub physics: BipedPhysics,

    /// `contact points` block — `marker name` per entry (string_id).
    pub contact_points: Vec<String>,

    pub reanimation_character: String,
    pub reanimation_morph_muffins: String,
    pub death_spawn_character: String,
    pub death_spawn_count: i16,

    /// `leaping data` substruct.
    pub leaping_data: BipedLeapingData,
    /// `ground fitting data` substruct.
    pub ground_fitting_data: BipedGroundFittingData,
}

impl BipedDefinition {
    pub fn from_tag(tag: &TagFile) -> Result<Self, BipedError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != BIPED_GROUP {
            return Err(BipedError::WrongGroup {
                expected: BIPED_GROUP,
                actual,
            });
        }
        let object = Arc::new(ObjectDefinition::from_tag(tag)?);
        let root = tag.root();
        let unit_struct = root
            .descend("unit")
            .ok_or(BipedError::MissingSubstruct { path: "unit" })?;
        let unit = Arc::new(UnitDefinition::from_unit_struct(object, &unit_struct));

        let physics = root
            .field("physics")
            .and_then(|f| f.as_struct())
            .map(|s| parse_physics(&s))
            .unwrap_or_default();

        let lock_on_data = root
            .field("lock-on data")
            .and_then(|f| f.as_struct())
            .map(|s| parse_lock_on(&s))
            .unwrap_or_default();
        let leaping_data = root
            .field("leaping data")
            .and_then(|f| f.as_struct())
            .map(|s| parse_leaping(&s))
            .unwrap_or_default();
        let ground_fitting_data = root
            .field("ground fitting data")
            .and_then(|f| f.as_struct())
            .map(|s| parse_ground_fitting(&s))
            .unwrap_or_default();
        let contact_points = root
            .field("contact points")
            .and_then(|f| f.as_block())
            .map(|block| {
                block
                    .iter()
                    .map(|e| e.read_string_id("marker name").unwrap_or_default())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(Self {
            unit,
            moving_turning_speed: root.read_real("moving turning speed").unwrap_or(0.0),
            flags: root.try_read_flags("flags").unwrap_or_default(),
            stationary_turning_threshold: root
                .read_real("stationary turning threshold")
                .unwrap_or(0.0),
            jump_velocity: root.read_real("jump velocity").unwrap_or(0.0),
            maximum_soft_landing_time: root
                .read_real("maximum soft landing time")
                .unwrap_or(0.0),
            maximum_hard_landing_time: root
                .read_real("maximum hard landing time")
                .unwrap_or(0.0),
            minimum_soft_landing_velocity: root
                .read_real("minimum soft landing velocity")
                .unwrap_or(0.0),
            minimum_hard_landing_velocity: root
                .read_real("minimum hard landing velocity")
                .unwrap_or(0.0),
            maximum_hard_landing_velocity: root
                .read_real("maximum hard landing velocity")
                .unwrap_or(0.0),
            death_hard_landing_velocity: root
                .read_real("death hard landing velocity")
                .unwrap_or(0.0),
            stun_duration: root.read_real("stun duration").unwrap_or(0.0),
            standing_camera_height: root.read_real("standing camera height").unwrap_or(0.0),
            crouching_camera_height: root.read_real("crouching camera height").unwrap_or(0.0),
            crouch_transition_time: root.read_real("crouch transition time").unwrap_or(0.0),
            camera_interpolation_start: root
                .read_real("camera interpolation start")
                .unwrap_or(0.0),
            camera_interpolation_end: root
                .read_real("camera interpolation end")
                .unwrap_or(0.0),
            camera_forward_movement_scale: root
                .read_real("camera forward movement scale")
                .unwrap_or(0.0),
            camera_side_movement_scale: root
                .read_real("camera side movement scale")
                .unwrap_or(0.0),
            camera_vertical_movement_scale: root
                .read_real("camera vertical movement scale")
                .unwrap_or(0.0),
            camera_exclusion_distance: root
                .read_real("camera exclusion distance")
                .unwrap_or(0.0),
            autoaim_width: root.read_real("autoaim width").unwrap_or(0.0),
            lock_on_data,
            runtime_physics_control_node_index: root
                .read_int_any("runtime physics control node index")
                .unwrap_or(0) as i16,
            runtime_cosine_stationary_turning_threshold: root
                .read_real("runtime cosine stationary turning threshold")
                .unwrap_or(0.0),
            runtime_crouch_transition_velocity: root
                .read_real("runtime crouch transition velocity")
                .unwrap_or(0.0),
            runtime_pelvis_node_index: root
                .read_int_any("runtime pelvis node index")
                .unwrap_or(0) as i16,
            runtime_head_node_index: root
                .read_int_any("runtime head node index")
                .unwrap_or(0) as i16,
            head_shot_acc_scale: root.read_real("head shot acc scale").unwrap_or(0.0),
            area_damage_effect: root
                .read_tag_ref_path("area damage effect")
                .unwrap_or_default(),
            physics,
            contact_points,
            reanimation_character: root
                .read_tag_ref_path("reanimation character")
                .unwrap_or_default(),
            reanimation_morph_muffins: root
                .read_tag_ref_path("reanimation/morph muffins")
                .unwrap_or_default(),
            death_spawn_character: root
                .read_tag_ref_path("death spawn character")
                .unwrap_or_default(),
            death_spawn_count: root.read_int_any("death spawn count").unwrap_or(0) as i16,
            leaping_data,
            ground_fitting_data,
        })
    }
}

fn parse_lock_on(s: &TagStruct<'_>) -> BipedLockOnData {
    BipedLockOnData {
        flags: s.try_read_flags("flags").unwrap_or_default(),
        lock_on_distance: s.read_real("lock on distance").unwrap_or(0.0),
    }
}

fn parse_leaping(s: &TagStruct<'_>) -> BipedLeapingData {
    BipedLeapingData {
        leap_flags: s.try_read_flags("leap flags").unwrap_or_default(),
        dampening_scale: s.read_real("dampening scale").unwrap_or(0.0),
        roll_delay: s.read_real("roll delay").unwrap_or(0.0),
        cannonball_off_axis_scale: s.read_real("cannonball off-axis scale").unwrap_or(0.0),
        cannonball_off_track_scale: s.read_real("cannonball off-track scale").unwrap_or(0.0),
        cannonball_roll_bounds: s.read_angle_bounds("cannonball roll bounds"),
        anticipation_ratio_bounds: s.read_real_bounds("anticipation ratio bounds"),
        reaction_force_bounds: s.read_real_bounds("reaction force bounds"),
        lobbing_desire: s.read_real("lobbing desire").unwrap_or(0.0),
    }
}

fn parse_ground_fitting(s: &TagStruct<'_>) -> BipedGroundFittingData {
    BipedGroundFittingData {
        ground_normal_dampening: s.read_real("ground normal dampening").unwrap_or(0.0),
        root_offset_max_scale: s.read_real("root offset max scale").unwrap_or(0.0),
        root_offset_dampening: s.read_real("root offset dampening").unwrap_or(0.0),
        following_cam_scale: s.read_real("following cam scale").unwrap_or(0.0),
        root_leaning_scale: s.read_real("root leaning scale").unwrap_or(0.0),
        foot_roll_max: s.read_real("foot roll max").unwrap_or(0.0),
        foot_pitch_max: s.read_real("foot pitch max").unwrap_or(0.0),
        pivot_on_foot_scale: s.read_real("pivot-on-foot scale").unwrap_or(0.0),
        pivot_min_foot_delta: s.read_real("pivot min foot delta").unwrap_or(0.0),
        pivot_stride_length_scale: s.read_real("pivot stride length scale").unwrap_or(0.0),
        pivot_throttle_scale: s.read_real("pivot throttle scale").unwrap_or(0.0),
        pivot_offset_dampening: s.read_real("pivot offset dampening").unwrap_or(0.0),
    }
}

fn parse_physics(s: &TagStruct<'_>) -> BipedPhysics {
    BipedPhysics::from_struct(s)
}

impl BipedPhysics {
    /// Walk a `character_physics_struct` field. Public so
    /// `CreatureDefinition` can reuse — both bipeds and creatures
    /// hang the same engine struct off their root.
    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        let ground = s
            .field("ground physics")
            .and_then(|f| f.as_struct())
            .map(|sub| parse_ground(&sub))
            .unwrap_or_default();
        let flying = s
            .field("flying physics")
            .and_then(|f| f.as_struct())
            .map(|sub| parse_flying(&sub))
            .unwrap_or_default();
        let count = |name: &str| -> usize {
            s.field(name)
                .and_then(|f| f.as_block())
                .map(|b| b.len())
                .unwrap_or(0)
        };
        Self {
            flags: s.try_read_flags("flags").unwrap_or_default(),
            height_standing: s.read_real("height standing").unwrap_or(0.0),
            height_crouching: s.read_real("height crouching").unwrap_or(0.0),
            radius: s.read_real("radius").unwrap_or(0.0),
            mass: s.read_real("mass").unwrap_or(0.0),
            living_material_name: s.read_string_id("living material name").unwrap_or_default(),
            dead_material_name: s.read_string_id("dead material name").unwrap_or_default(),
            runtime_global_material_type: s
                .read_int_any("runtime global material type")
                .unwrap_or(0) as i16,
            runtime_dead_global_material_type: s
                .read_int_any("runtime dead global material type")
                .unwrap_or(0) as i16,
            dead_sphere_shape_count: count("dead sphere shapes"),
            pill_shape_count: count("pill shapes"),
            sphere_shape_count: count("sphere shapes"),
            ground,
            flying,
        }
    }
}

fn parse_ground(s: &TagStruct<'_>) -> BipedGroundPhysics {
    BipedGroundPhysics {
        maximum_slope_angle: s.read_real("maximum slope angle").unwrap_or(0.0),
        downhill_falloff_angle: s.read_real("downhill falloff angle").unwrap_or(0.0),
        downhill_cutoff_angle: s.read_real("downhill cutoff angle").unwrap_or(0.0),
        uphill_falloff_angle: s.read_real("uphill falloff angle").unwrap_or(0.0),
        uphill_cutoff_angle: s.read_real("uphill cutoff angle").unwrap_or(0.0),
        downhill_velocity_scale: s.read_real("downhill velocity scale").unwrap_or(0.0),
        uphill_velocity_scale: s.read_real("uphill velocity scale").unwrap_or(0.0),
        climb_inflection_angle: s.read_real("climb inflection angle").unwrap_or(0.0),
        scale_airborne_reaction_time: s
            .read_real("scale airborne reaction time")
            .unwrap_or(0.0),
        scale_ground_adhesion_velocity: s
            .read_real("scale ground adhesion velocity")
            .unwrap_or(0.0),
        gravity_scale: s.read_real("gravity scale").unwrap_or(0.0),
    }
}

fn parse_flying(s: &TagStruct<'_>) -> BipedFlyingPhysics {
    BipedFlyingPhysics {
        bank_angle: s.read_real("bank angle").unwrap_or(0.0),
        bank_apply_time: s.read_real("bank apply time").unwrap_or(0.0),
        bank_decay_time: s.read_real("bank decay time").unwrap_or(0.0),
        pitch_ratio: s.read_real("pitch ratio").unwrap_or(0.0),
        max_velocity: s.read_real("max velocity").unwrap_or(0.0),
        max_sidestep_velocity: s.read_real("max sidestep velocity").unwrap_or(0.0),
        acceleration: s.read_real("acceleration").unwrap_or(0.0),
        deceleration: s.read_real("deceleration").unwrap_or(0.0),
        angular_velocity_maximum: s.read_real("angular velocity maximum").unwrap_or(0.0),
        angular_acceleration_maximum: s.read_real("angular acceleration maximum").unwrap_or(0.0),
        crouch_velocity_modifier: s.read_real("crouch velocity modifier").unwrap_or(0.0),
        flags: s.try_read_flags("flags").unwrap_or_default(),
    }
}
