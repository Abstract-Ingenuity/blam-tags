//! `.vehicle` (`vehi`) tag walker.
//!
//! Schema: `definitions/halo3_mcc/vehicle.json` → `vehicle_group`
//! (size 1020, parent_tag `unit`).
//! Ares source: `source/units/vehicles.h`.

use crate::api::TagStruct;
use crate::file::TagFile;
use crate::object::ObjectDefinition;
use crate::typed_enums::{Enum, Flags};
use crate::unit::UnitDefinition;
use std::sync::Arc;

const VEHICLE_GROUP: [u8; 4] = *b"vehi";

/// `vehicle_flags` (long_flags) — vehicle_group variant.
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum VehicleFlags {
    #[strum(serialize = "passengers adopt original squad")] PassengersAdoptOriginalSquad = 0,
    #[strum(serialize = "snap facing to forward (ghosts)")] SnapFacingToForward = 1,
    #[strum(serialize = "throttle to target (hornets)")] ThrottleToTarget = 2,
    #[strum(serialize = "stationary fight (tanks)")] StationaryFight = 3,
    #[strum(serialize = "keep moving")] KeepMoving = 4,
}

/// `havok_vehicle_physics_definition_flags` (long_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum HavokVehiclePhysicsFlags {
    #[strum(serialize = "invalid")] Invalid = 0,
}

/// `player_training_vehicle_type_enum` (char_enum).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i8)]
pub enum PlayerTrainingVehicleType {
    #[default]
    #[strum(serialize = "none")] None = 0,
    #[strum(serialize = "warthog")] Warthog = 1,
    #[strum(serialize = "warthog turret")] WarthogTurret = 2,
    #[strum(serialize = "ghost")] Ghost = 3,
    #[strum(serialize = "banshee")] Banshee = 4,
    #[strum(serialize = "tank")] Tank = 5,
    #[strum(serialize = "wraith")] Wraith = 6,
}

/// `vehicle_size_enum` (char_enum). Determines seat-size eligibility.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i8)]
pub enum VehicleSize {
    #[default]
    #[strum(serialize = "small")] Small = 0,
    #[strum(serialize = "large")] Large = 1,
}

#[derive(Debug)]
pub enum VehicleError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
    MissingSubstruct { path: &'static str },
    ObjectDefinition(crate::object::ObjectDefinitionError),
}

impl std::fmt::Display for VehicleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongGroup { expected, actual } => write!(
                f,
                "expected group '{}', got '{}'",
                std::str::from_utf8(expected).unwrap_or("?"),
                std::str::from_utf8(actual).unwrap_or("?"),
            ),
            Self::MissingSubstruct { path } => write!(f, "tag missing substruct '{path}'"),
            Self::ObjectDefinition(e) => write!(f, "object substruct: {e}"),
        }
    }
}

impl std::error::Error for VehicleError {}

impl From<crate::object::ObjectDefinitionError> for VehicleError {
    fn from(e: crate::object::ObjectDefinitionError) -> Self {
        Self::ObjectDefinition(e)
    }
}

// ---------------------------------------------------------------------------
// Physics substructs
// ---------------------------------------------------------------------------

/// `vehicle_physics_types_struct` (size 120). Each authored type
/// surfaces as a count — Ares's `physics_*_block` bodies are deep
/// tuning blocks per vehicle archetype (chopper/banshee/warthog
/// /etc.) and aren't read by `vehicle_compute_function_value`. We
/// keep them as counts so the consumer can detect which archetype a
/// tag selected (engine sets exactly one block to count==1).
#[derive(Debug, Clone, Default)]
pub struct VehiclePhysicsTypes {
    pub human_tank_count: usize,
    pub human_jeep_count: usize,
    pub human_plane_count: usize,
    pub alien_scout_count: usize,
    pub alien_fighter_count: usize,
    pub turret_count: usize,
    pub mantis_count: usize,
    pub vtol_count: usize,
    pub chopper_count: usize,
    pub guardian_count: usize,
}

impl VehiclePhysicsTypes {
    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        let count = |name: &str| -> usize {
            s.field(name)
                .and_then(|f| f.as_block())
                .map(|b| b.len())
                .unwrap_or(0)
        };
        Self {
            human_tank_count: count("type-human_tank"),
            human_jeep_count: count("type-human_jeep"),
            human_plane_count: count("type-human_plane"),
            alien_scout_count: count("type-alien_scout"),
            alien_fighter_count: count("type-alien_fighter"),
            turret_count: count("type-turret"),
            mantis_count: count("type-mantis"),
            vtol_count: count("type-vtol"),
            chopper_count: count("type-chopper"),
            guardian_count: count("type-guardian"),
        }
    }
}

/// `havok_vehicle_physics_struct` (size 96). Top-level havok tuning
/// scalars + 3 sub-blocks (anti-gravity / friction points / phantom
/// shapes). We surface the scalars plus block counts. The 3 block
/// bodies hold per-marker placement data which the runtime walks
/// directly on the engine side, not via compute_function_value.
#[derive(Debug, Clone, Default)]
pub struct HavokVehiclePhysics {
    pub flags: Flags<HavokVehiclePhysicsFlags, u32>,
    pub ground_friction: f32,
    pub ground_depth: f32,
    pub ground_damp_factor: f32,
    pub ground_moving_friction: f32,
    /// `ground maximum slope 0:degrees 0-90`.
    pub ground_maximum_slope_0: f32,
    /// `ground maximum slope 1:degrees 0-90` (must be > slope 0).
    pub ground_maximum_slope_1: f32,
    /// `maximum normal force contribution`. 0 = default 3.
    pub maximum_normal_force_contribution: f32,
    pub anti_gravity_bank_lift: f32,
    pub steering_bank_reaction_scale: f32,
    /// `gravity scale`. 0 = default 1.
    pub gravity_scale: f32,
    /// `radius` — runtime-generated from hkConvexShape, kept here.
    pub radius: f32,
    pub maximum_update_distance: f32,
    pub maximum_update_period: f32,
    pub maximum_remote_update_period: f32,

    pub anti_gravity_point_count: usize,
    pub friction_point_count: usize,
    pub phantom_shape_count: usize,
}

impl HavokVehiclePhysics {
    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        let count = |name: &str| -> usize {
            s.field(name)
                .and_then(|f| f.as_block())
                .map(|b| b.len())
                .unwrap_or(0)
        };
        Self {
            flags: s.try_read_flags("flags").unwrap_or_default(),
            ground_friction: s.read_real("ground friction").unwrap_or(0.0),
            ground_depth: s.read_real("ground depth").unwrap_or(0.0),
            ground_damp_factor: s.read_real("ground damp factor").unwrap_or(0.0),
            ground_moving_friction: s.read_real("ground moving friction").unwrap_or(0.0),
            ground_maximum_slope_0: s.read_real("ground maximum slope 0").unwrap_or(0.0),
            ground_maximum_slope_1: s.read_real("ground maximum slope 1").unwrap_or(0.0),
            maximum_normal_force_contribution: s
                .read_real("maximum normal force contribution")
                .unwrap_or(0.0),
            anti_gravity_bank_lift: s.read_real("anti_gravity_bank_lift").unwrap_or(0.0),
            steering_bank_reaction_scale: s
                .read_real("steering_bank_reaction_scale")
                .unwrap_or(0.0),
            gravity_scale: s.read_real("gravity scale").unwrap_or(0.0),
            radius: s.read_real("radius").unwrap_or(0.0),
            maximum_update_distance: s.read_real("maximum update distance").unwrap_or(0.0),
            maximum_update_period: s.read_real("maximum update period").unwrap_or(0.0),
            maximum_remote_update_period: s
                .read_real("maximum remote update period")
                .unwrap_or(0.0),
            anti_gravity_point_count: count("anti gravity points"),
            friction_point_count: count("friction points"),
            phantom_shape_count: count("shape phantom shape"),
        }
    }
}

/// Walked `vehicle_group` (size 1020). Field order matches schema
/// verbatim. The two physics substructs are surfaced at archetype
/// granularity — `vehicle_physics_types_struct` reports which of
/// the 10 archetype blocks were authored (engine sets exactly one),
/// `havok_vehicle_physics_struct` carries the top-level havok tuning
/// scalars and block counts. The deep per-archetype tuning bodies
/// aren't read by `vehicle_compute_function_value` and remain
/// unsurfaced.
#[derive(Debug, Clone, Default)]
pub struct VehicleDefinition {
    pub unit: Arc<UnitDefinition>,
    /// `flags` (long_flags) — vehicle-specific.
    pub flags: Flags<VehicleFlags, u32>,
    /// `physics types` substruct.
    pub physics_types: VehiclePhysicsTypes,
    /// `havok vehicle physics` substruct.
    pub havok_vehicle_physics: HavokVehiclePhysics,
    /// `player training vehicle type` (char_enum).
    pub player_training_vehicle_type: Enum<PlayerTrainingVehicleType, i8>,
    /// `vehicle size` (char_enum). Determines seat-size eligibility.
    pub vehicle_size: Enum<VehicleSize, i8>,
    /// `minimum flipping angular velocity`.
    pub minimum_flipping_angular_velocity: f32,
    /// `maximum flipping angular velocity`.
    pub maximum_flipping_angular_velocity: f32,
    pub crouch_transition_time: f32,
    /// `HOOJYTSU!` (real) — engine internal field, kept for fidelity.
    pub hoojytsu: f32,
    pub seat_entrance_acceleration_scale: f32,
    pub seat_exit_acceleration_scale: f32,
    pub blur_speed: f32,
    pub flip_message: String,
    pub suspension_sound: String,
    pub special_effect: String,
    pub driver_boost_damage_effect_or_response: String,
    pub rider_boost_damage_effect_or_response: String,
}

impl VehicleDefinition {
    pub fn from_tag(tag: &TagFile) -> Result<Self, VehicleError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != VEHICLE_GROUP {
            return Err(VehicleError::WrongGroup { expected: VEHICLE_GROUP, actual });
        }
        let object = Arc::new(ObjectDefinition::from_tag(tag)?);
        let root = tag.root();
        let unit_struct = root
            .descend("unit")
            .ok_or(VehicleError::MissingSubstruct { path: "unit" })?;
        let unit = Arc::new(UnitDefinition::from_unit_struct(object, &unit_struct));
        let physics_types = root
            .field("physics types")
            .and_then(|f| f.as_struct())
            .map(|s| VehiclePhysicsTypes::from_struct(&s))
            .unwrap_or_default();
        let havok_vehicle_physics = root
            .field("havok vehicle physics")
            .and_then(|f| f.as_struct())
            .map(|s| HavokVehiclePhysics::from_struct(&s))
            .unwrap_or_default();
        Ok(Self {
            unit,
            flags: root.try_read_flags("flags").unwrap_or_default(),
            physics_types,
            havok_vehicle_physics,
            player_training_vehicle_type: root.read_enum("player training vehicle type"),
            vehicle_size: root.read_enum("vehicle size"),
            minimum_flipping_angular_velocity: root
                .read_real("minimum flipping angular velocity")
                .unwrap_or(0.0),
            maximum_flipping_angular_velocity: root
                .read_real("maximum flipping angular velocity")
                .unwrap_or(0.0),
            crouch_transition_time: root.read_real("crouch transition time").unwrap_or(0.0),
            hoojytsu: root.read_real("HOOJYTSU").unwrap_or(0.0),
            seat_entrance_acceleration_scale: root
                .read_real("seat enterance acceleration scale")
                .unwrap_or(0.0),
            seat_exit_acceleration_scale: root
                .read_real("seat exit accelersation scale")
                .unwrap_or(0.0),
            blur_speed: root.read_real("blur speed").unwrap_or(0.0),
            flip_message: root.read_string_id("flip message").unwrap_or_default(),
            suspension_sound: root.read_tag_ref_path("suspension sound").unwrap_or_default(),
            special_effect: root.read_tag_ref_path("special effect").unwrap_or_default(),
            driver_boost_damage_effect_or_response: root
                .read_tag_ref_path("driver boost damage effect or response")
                .unwrap_or_default(),
            rider_boost_damage_effect_or_response: root
                .read_tag_ref_path("rider boost damage effect or response")
                .unwrap_or_default(),
        })
    }
}
