//! `particle_physics` (`pmov`) tag walker — movement template
//! referenced by `c_particle_emitter_definition` at the per-emitter
//! `particle_movement` field. Drives per-particle physics simulation
//! (velocity, friction, gravity), collision response, swarm AI, and
//! wind interaction.
//!
//! ## Runtime hookup
//!
//! - Authored on `c_particle_emitter_definition.particle_movement` and
//!   on `c_particle_definition.particle_movement` (sub-emitter
//!   movements). Engine resolves at emitter init.
//! - `c_particle_movement_definition::get_property_by_index @
//!   0x180579230` looks up a controller property by composite ID.
//! - `c_particle_controller_parameter::get_property @ 0x18057B370`
//!   returns the editable property for a controller parameter.
//! - GPU side: properties feed into `particle_update.wgsl` (Tier 4)
//!   evaluation kernels per-particle per-frame.
//!
//! ## Authoring shape
//!
//! Tag carries:
//! - `template` tag_reference (optional fallback to another pmov)
//! - `flags` (`particle_movement_flags`, 8 bits) — physics +
//!   collision-with-{structure, media, scenery, vehicles, bipeds} +
//!   swarm + wind
//! - `movements[]` block — one entry per active controller
//!   (`particle_movement_type` enum: physics / collider / swarm / wind)
//! - Each movement: `parameters[]` block — parameter_id + property
//!
//! Each property mirrors `c_editable_property_base` (32B) shape but
//! the per-tag struct stores only authoring metadata; runtime fields
//! (`runtime m_constant_parameters!`, etc.) are tool.exe-resolved.

use crate::api::TagStruct;
use crate::effects_properties::EditableProperty;
use crate::file::TagFile;

const PMOV_GROUP: [u8; 4] = *b"pmov";

#[derive(Debug)]
pub enum ParticlePhysicsError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
}

impl std::fmt::Display for ParticlePhysicsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongGroup { expected, actual } => write!(
                f,
                "particle_physics: wrong tag group (expected {:?}, got {:?})",
                std::str::from_utf8(expected).unwrap_or("????"),
                std::str::from_utf8(actual).unwrap_or("????"),
            ),
        }
    }
}

impl std::error::Error for ParticlePhysicsError {}

/// `particle_movement_flags` (8 bits) — composite of physics-enable +
/// collision-target classes + swarm + wind. NOT a movement-type
/// dispatch (that's `particle_movement_type` per controller).
pub const PMOV_FLAG_PHYSICS: u32 = 1 << 0;
pub const PMOV_FLAG_COLLIDE_WITH_STRUCTURE: u32 = 1 << 1;
pub const PMOV_FLAG_COLLIDE_WITH_MEDIA: u32 = 1 << 2;
pub const PMOV_FLAG_COLLIDE_WITH_SCENERY: u32 = 1 << 3;
pub const PMOV_FLAG_COLLIDE_WITH_VEHICLES: u32 = 1 << 4;
pub const PMOV_FLAG_COLLIDE_WITH_BIPEDS: u32 = 1 << 5;
pub const PMOV_FLAG_SWARM: u32 = 1 << 6;
pub const PMOV_FLAG_WIND: u32 = 1 << 7;

/// `particle_movement_type` — per-controller dispatch. Selects which
/// inner physics integrator the engine runs against the controller's
/// parameter set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ControllerType {
    Physics = 0,
    Collider = 1,
    Swarm = 2,
    Wind = 3,
}

impl ControllerType {
    pub fn from_index(i: i64) -> Option<Self> {
        Some(match i {
            0 => Self::Physics,
            1 => Self::Collider,
            2 => Self::Swarm,
            3 => Self::Wind,
            _ => return None,
        })
    }
}

/// One `particle_controller_parameters` entry — a parameter slot on
/// a controller. The `parameter_id` is a composite (controller type
/// in high bits + parameter index in low bits) — `get_property_by_index
/// @ 0x180579230` is the runtime lookup.
#[derive(Debug, Clone, Default)]
pub struct ControllerParameter {
    pub parameter_id: i32,
    pub property: EditableProperty,
}

/// One `particle_controller` entry — a single integrator instance
/// authored with a specific type + parameter set.
#[derive(Debug, Clone, Default)]
pub struct ParticleController {
    /// Authored controller type (`particle_movement_type` enum). `None`
    /// when out of range.
    pub controller_type: Option<ControllerType>,
    pub parameters: Vec<ControllerParameter>,
    pub runtime_constant_parameters: i32,
    pub runtime_used_particle_states: i32,
}

/// Walked `particle_physics` tag.
#[derive(Debug, Clone, Default)]
pub struct ParticlePhysics {
    /// Optional template tag — engine merges its movements with this
    /// tag's authoring layer (template wins on conflict, AFAICT).
    pub template: Option<String>,
    /// `particle_movement_flags` (8 bits).
    pub flags: u32,
    pub movements: Vec<ParticleController>,
}

impl ParticlePhysics {
    pub fn from_tag(tag: &TagFile) -> Result<Self, ParticlePhysicsError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != PMOV_GROUP {
            return Err(ParticlePhysicsError::WrongGroup { expected: PMOV_GROUP, actual });
        }
        Ok(Self::from_struct(&tag.root()))
    }

    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        let movements = s
            .field("movements")
            .and_then(|f| f.as_block())
            .map(|b| {
                let mut out = Vec::with_capacity(b.len());
                for i in 0..b.len() {
                    if let Some(elem) = b.element(i) {
                        out.push(ParticleController::from_struct(&elem));
                    }
                }
                out
            })
            .unwrap_or_default();
        Self {
            template: s.read_tag_ref_path("template"),
            flags: s.read_int_any("flags").unwrap_or(0) as u32,
            movements,
        }
    }
}

impl ParticleController {
    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        let parameters = s
            .field("parameters")
            .and_then(|f| f.as_block())
            .map(|b| {
                let mut out = Vec::with_capacity(b.len());
                for i in 0..b.len() {
                    if let Some(elem) = b.element(i) {
                        out.push(ControllerParameter::from_struct(&elem));
                    }
                }
                out
            })
            .unwrap_or_default();
        Self {
            controller_type: s
                .read_int_any("type")
                .and_then(|i| ControllerType::from_index(i as i64)),
            parameters,
            runtime_constant_parameters: s
                .read_int_any("runtime m_constant_parameters")
                .unwrap_or(0) as i32,
            runtime_used_particle_states: s
                .read_int_any("runtime m_used_particle_states")
                .unwrap_or(0) as i32,
        }
    }
}

impl ControllerParameter {
    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        let property = s
            .field("property")
            .and_then(|f| f.as_struct())
            .map(|inner| EditableProperty::from_struct(&inner))
            .unwrap_or_default();
        Self {
            parameter_id: s.read_int_any("parameter id").unwrap_or(0) as i32,
            property,
        }
    }
}

