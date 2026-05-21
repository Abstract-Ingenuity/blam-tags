//! `.projectile` (`proj`) tag walker.
//!
//! Schema: `definitions/halo3_mcc/projectile.json` → `projectile_group`
//! (size 672, parent_tag `obje`).
//! Ares source: `source/items/projectiles.h`.
//!
//! Surfaces every authored sub-block + the runtime-relevant fields
//! that `projectile_compute_function_value` reads.

use crate::api::TagStruct;
use crate::file::TagFile;
use crate::math::{AngleBounds, Bounds};
use crate::object::ObjectDefinition;
use std::sync::Arc;

const PROJECTILE_GROUP: [u8; 4] = *b"proj";

#[derive(Debug)]
pub enum ProjectileError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
    ObjectDefinition(crate::object::ObjectDefinitionError),
}

impl std::fmt::Display for ProjectileError {
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

impl std::error::Error for ProjectileError {}

impl From<crate::object::ObjectDefinitionError> for ProjectileError {
    fn from(e: crate::object::ObjectDefinitionError) -> Self {
        Self::ObjectDefinition(e)
    }
}

// ---------------------------------------------------------------------------
// Sub-structs
// ---------------------------------------------------------------------------

/// `super_detonation_damage_struct` (size 16) — single tag_reference.
#[derive(Debug, Clone, Default)]
pub struct ProjectileSuperDetonationDamage {
    pub super_detonation_damage: String,
}

impl ProjectileSuperDetonationDamage {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            super_detonation_damage: s
                .read_tag_ref_path("super detonation damage")
                .unwrap_or_default(),
        }
    }
}

/// `angular_velocity_lower_bound_struct` (size 4) — single angle.
#[derive(Debug, Clone, Default)]
pub struct ProjectileAngularVelocityLowerBound {
    pub guided_angular_velocity_lower: f32,
}

impl ProjectileAngularVelocityLowerBound {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            guided_angular_velocity_lower: s
                .read_real("guided angular velocity (lower)")
                .unwrap_or(0.0),
        }
    }
}

/// `projectile_material_response_block` (size 64). One entry per
/// material the projectile can hit.
#[derive(Debug, Clone, Default)]
pub struct ProjectileMaterialResponse {
    pub flags: u16,
    pub default_response: i16,
    pub material_name: String,
    /// `runtime material index!` — index into global_material_data
    /// populated at load.
    pub runtime_material_index: i16,
    pub potential_response: i16,
    pub response_flags: u16,
    /// `chance fraction:[0,1]`.
    pub chance_fraction: f32,
    /// `between:degrees` (angle_bounds).
    pub between: AngleBounds,
    /// `and:world units per second` (real_bounds).
    pub and: Bounds<f32>,
    pub scale_effects_by: i16,
    pub angular_noise: f32,
    pub velocity_noise: f32,
    pub initial_friction: f32,
    pub maximum_distance: f32,
    pub parallel_friction: f32,
    pub perpendicular_friction: f32,
}

impl ProjectileMaterialResponse {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            flags: s.read_int_any("flags").unwrap_or(0) as u16,
            default_response: s.read_int_any("default response").unwrap_or(0) as i16,
            material_name: s.read_string_id("material name").unwrap_or_default(),
            runtime_material_index: s.read_int_any("runtime material index").unwrap_or(0) as i16,
            potential_response: s.read_int_any("potential response").unwrap_or(0) as i16,
            response_flags: s.read_int_any("response flags").unwrap_or(0) as u16,
            chance_fraction: s.read_real("chance fraction").unwrap_or(0.0),
            between: s.read_angle_bounds("between"),
            and: s.read_real_bounds("and"),
            scale_effects_by: s.read_int_any("scale effects by").unwrap_or(0) as i16,
            angular_noise: s.read_real("angular noise").unwrap_or(0.0),
            velocity_noise: s.read_real("velocity noise").unwrap_or(0.0),
            initial_friction: s.read_real("initial friction").unwrap_or(0.0),
            maximum_distance: s.read_real("maximum distance").unwrap_or(0.0),
            parallel_friction: s.read_real("parallel friction").unwrap_or(0.0),
            perpendicular_friction: s.read_real("perpendicular friction").unwrap_or(0.0),
        }
    }
}

/// `brute_grenade_block` (size 48). Brute-grenade-specific airborne
/// physics tuning.
#[derive(Debug, Clone, Default)]
pub struct ProjectileBruteGrenade {
    pub minimum_angular_vel: f32,
    pub maximum_angular_vel: f32,
    pub spin_angular_vel: f32,
    pub angular_damping: f32,
    pub drag_angle_k: f32,
    pub drag_speed_k: f32,
    pub drag_exponent: f32,
    pub attach_sample_radius: f32,
    pub attach_acc_k: f32,
    pub attach_acc_s: f32,
    pub attach_acc_e: f32,
    pub attach_acc_damping: f32,
}

impl ProjectileBruteGrenade {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            minimum_angular_vel: s.read_real("minimum angular vel").unwrap_or(0.0),
            maximum_angular_vel: s.read_real("maximum angular vel").unwrap_or(0.0),
            spin_angular_vel: s.read_real("spin angular vel").unwrap_or(0.0),
            angular_damping: s.read_real("angular damping").unwrap_or(0.0),
            drag_angle_k: s.read_real("drag angle k").unwrap_or(0.0),
            drag_speed_k: s.read_real("drag speed k").unwrap_or(0.0),
            drag_exponent: s.read_real("drag exponent").unwrap_or(0.0),
            attach_sample_radius: s.read_real("attach sample radius").unwrap_or(0.0),
            attach_acc_k: s.read_real("attach acc k").unwrap_or(0.0),
            attach_acc_s: s.read_real("attach acc s").unwrap_or(0.0),
            attach_acc_e: s.read_real("attach acc e").unwrap_or(0.0),
            attach_acc_damping: s.read_real("attach acc damping").unwrap_or(0.0),
        }
    }
}

/// `fire_bomb_grenade_block` (size 4).
#[derive(Debug, Clone, Default)]
pub struct ProjectileFireBombGrenade {
    pub projection_offset: f32,
}

impl ProjectileFireBombGrenade {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            projection_offset: s.read_real("projection offset").unwrap_or(0.0),
        }
    }
}

/// `conical_projection_block` (size 12). Cone-spread distribution
/// for area-of-effect projectiles.
#[derive(Debug, Clone, Default)]
pub struct ProjectileConicalProjection {
    pub yaw_count: i16,
    pub pitch_count: i16,
    /// `distribution exponent`. exp==.5 even, exp==1 H2, exp>1 weighted center.
    pub distribution_exponent: f32,
    /// `spread:degrees`.
    pub spread: f32,
}

impl ProjectileConicalProjection {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            yaw_count: s.read_int_any("yaw count").unwrap_or(0) as i16,
            pitch_count: s.read_int_any("pitch count").unwrap_or(0) as i16,
            distribution_exponent: s.read_real("distribution exponent").unwrap_or(0.0),
            spread: s.read_real("spread").unwrap_or(0.0),
        }
    }
}

// ---------------------------------------------------------------------------
// ProjectileDefinition
// ---------------------------------------------------------------------------

/// Walked `projectile_group`. Field set covers what
/// `projectile_compute_function_value` reads (range/velocity/
/// acceleration math) plus the authored sub-blocks for
/// super_detonation / brute_grenade / fire_bomb / material_response
/// / conical_projection / angular_velocity_lower_bound.
#[derive(Debug, Clone, Default)]
pub struct ProjectileDefinition {
    pub object: Arc<ObjectDefinition>,

    /// `maximum range:world units` — divisor for compute case
    /// `range_remaining` (sid 509).
    pub detonation_maximum_range: f32,
    /// `initial velocity:world units per second`.
    pub initial_velocity: f32,
    /// `final velocity:world units per second`.
    pub final_velocity: f32,
    /// `acceleration range` lower bound.
    pub initial_velocity_end_distance: f32,
    /// `acceleration range` upper bound.
    pub acceleration_range_upper: f32,
    /// `runtime acceleration bound inverse!` (engine field
    /// `runtime_oo_acceleration_distance`).
    pub runtime_oo_acceleration_distance: f32,

    /// `super detonation damage` substruct.
    pub super_detonation_damage: ProjectileSuperDetonationDamage,
    /// `angular velocity lower bound` substruct.
    pub angular_velocity_lower_bound: ProjectileAngularVelocityLowerBound,
    /// `material responses` block.
    pub material_responses: Vec<ProjectileMaterialResponse>,
    /// `brute grenade properties` block (max 1).
    pub brute_grenade_properties: Vec<ProjectileBruteGrenade>,
    /// `fire bomb grenade properties` block (max 1).
    pub fire_bomb_grenade_properties: Vec<ProjectileFireBombGrenade>,
    /// `conical projection` block.
    pub conical_projection: Vec<ProjectileConicalProjection>,
}

impl ProjectileDefinition {
    pub fn from_tag(tag: &TagFile) -> Result<Self, ProjectileError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != PROJECTILE_GROUP {
            return Err(ProjectileError::WrongGroup {
                expected: PROJECTILE_GROUP,
                actual,
            });
        }
        let object = Arc::new(ObjectDefinition::from_tag(tag)?);
        let root = tag.root();

        let accel_range = root.read_real_bounds("acceleration range");

        // Field-NAME based substruct lookups — schema names vary
        // slightly from the parsed struct names. The walker descends
        // by field name into the relevant sub-struct.
        let super_detonation_damage = root
            .field("super detonation damage")
            .and_then(|f| f.as_struct())
            .map(|sub| ProjectileSuperDetonationDamage::from_struct(&sub))
            .unwrap_or_default();
        let angular_velocity_lower_bound = root
            .field("angular velocity lower bound")
            .and_then(|f| f.as_struct())
            .map(|sub| ProjectileAngularVelocityLowerBound::from_struct(&sub))
            .unwrap_or_default();

        Ok(Self {
            object,
            detonation_maximum_range: root.read_real("maximum range").unwrap_or(0.0),
            initial_velocity: root.read_real("initial velocity").unwrap_or(0.0),
            final_velocity: root.read_real("final velocity").unwrap_or(0.0),
            initial_velocity_end_distance: accel_range.lower,
            acceleration_range_upper: accel_range.upper,
            runtime_oo_acceleration_distance: root
                .read_real("runtime acceleration bound inverse")
                .unwrap_or(0.0),
            super_detonation_damage,
            angular_velocity_lower_bound,
            material_responses: read_block_vec(
                &root,
                "material responses",
                ProjectileMaterialResponse::from_struct,
            ),
            brute_grenade_properties: read_block_vec(
                &root,
                "brute grenade properties",
                ProjectileBruteGrenade::from_struct,
            ),
            fire_bomb_grenade_properties: read_block_vec(
                &root,
                "fire bomb grenade properties",
                ProjectileFireBombGrenade::from_struct,
            ),
            conical_projection: read_block_vec(
                &root,
                "conical projection",
                ProjectileConicalProjection::from_struct,
            ),
        })
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
