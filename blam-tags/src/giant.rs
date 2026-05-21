//! `.giant` (`gint`) tag walker.
//!
//! Schema: `definitions/halo3_mcc/giant.json` →
//! `giant_struct_definition` (size 740, parent_tag `unit`).
//! Ares source: `source/units/giants.h`.

use crate::api::TagStruct;
use crate::file::TagFile;
use crate::math::AngleBounds;
use crate::object::ObjectDefinition;
use crate::unit::UnitDefinition;
use std::sync::Arc;

const GIANT_GROUP: [u8; 4] = *b"gint";

#[derive(Debug)]
pub enum GiantError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
    MissingSubstruct { path: &'static str },
    ObjectDefinition(crate::object::ObjectDefinitionError),
}

impl std::fmt::Display for GiantError {
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

impl std::error::Error for GiantError {}

impl From<crate::object::ObjectDefinitionError> for GiantError {
    fn from(e: crate::object::ObjectDefinitionError) -> Self {
        Self::ObjectDefinition(e)
    }
}

/// Walked `giant_buckle_parameters_block` (size 92). Each entry
/// drives one buckle/recovery pose used when a giant is grappled.
#[derive(Debug, Clone, Default)]
pub struct GiantBuckleParameters {
    pub lower_time: f32,
    pub lower_curve: i32,
    pub raise_time: f32,
    pub raise_curve: i32,
    pub pause_time_easy: f32,
    pub pause_time_normal: f32,
    pub pause_time_heroic: f32,
    pub pause_time_legendary: f32,
    pub buckle_gravity_scale: f32,
    pub buckling_marker: String,
    pub forward_rear_scan: f32,
    pub left_right_scan: f32,
    pub forward_rear_steps: i32,
    pub left_right_steps: i32,
    pub pitch_bounds: AngleBounds,
    pub roll_bounds: AngleBounds,
    pub buckle_animation: String,
    pub descent_overlay: String,
    pub paused_overlay: String,
    pub descent_overlay_scale: f32,
    pub paused_overlay_scale: f32,
}

impl GiantBuckleParameters {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            lower_time: s.read_real("lower time").unwrap_or(0.0),
            lower_curve: s.read_int_any("lower curve").unwrap_or(0) as i32,
            raise_time: s.read_real("raise time").unwrap_or(0.0),
            raise_curve: s.read_int_any("raise curve").unwrap_or(0) as i32,
            pause_time_easy: s.read_real("pause time (easy)").unwrap_or(0.0),
            pause_time_normal: s.read_real("pause time (normal)").unwrap_or(0.0),
            pause_time_heroic: s.read_real("pause time (heroic)").unwrap_or(0.0),
            pause_time_legendary: s.read_real("pause time (legendary)").unwrap_or(0.0),
            buckle_gravity_scale: s.read_real("buckle gravity scale").unwrap_or(0.0),
            buckling_marker: s.read_string_id("buckling marker").unwrap_or_default(),
            forward_rear_scan: s.read_real("forward-rear scan").unwrap_or(0.0),
            left_right_scan: s.read_real("left-right scan").unwrap_or(0.0),
            forward_rear_steps: s.read_int_any("forward-rear steps").unwrap_or(0) as i32,
            left_right_steps: s.read_int_any("left-right steps").unwrap_or(0) as i32,
            pitch_bounds: s.read_angle_bounds("pitch bounds"),
            roll_bounds: s.read_angle_bounds("roll bounds"),
            buckle_animation: s.read_string_id("buckle animation").unwrap_or_default(),
            descent_overlay: s.read_string_id("descent overlay").unwrap_or_default(),
            paused_overlay: s.read_string_id("paused overlay").unwrap_or_default(),
            descent_overlay_scale: s.read_real("descent overlay scale").unwrap_or(0.0),
            paused_overlay_scale: s.read_real("paused overlay scale").unwrap_or(0.0),
        }
    }
}

/// Walked `giant_struct_definition` (size 740). Field order matches
/// schema verbatim.
#[derive(Debug, Clone, Default)]
pub struct GiantDefinition {
    pub unit: Arc<UnitDefinition>,
    pub flags: u32,
    /// `accel_time:acceleration time in seconds`.
    pub accel_time: f32,
    /// `decel_time:deceleration time in seconds`.
    pub decel_time: f32,
    /// `minimum speed scale:as slow as we get` (real_fraction).
    pub minimum_speed_scale: f32,
    /// `elevation change rate:scale per update` (real_fraction).
    pub elevation_change_rate: f32,
    /// `max_vertical_reach` (world units).
    pub max_vertical_reach: f32,
    /// `buckle-settings` block.
    pub buckle_settings: Vec<GiantBuckleParameters>,
    /// `ankle ik scale` (real).
    pub ankle_ik_scale: f32,
}

impl GiantDefinition {
    pub fn from_tag(tag: &TagFile) -> Result<Self, GiantError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != GIANT_GROUP {
            return Err(GiantError::WrongGroup { expected: GIANT_GROUP, actual });
        }
        let object = Arc::new(ObjectDefinition::from_tag(tag)?);
        let root = tag.root();
        let unit_struct = root
            .descend("unit")
            .ok_or(GiantError::MissingSubstruct { path: "unit" })?;
        let unit = Arc::new(UnitDefinition::from_unit_struct(object, &unit_struct));
        let buckle_settings = root
            .field("buckle-settings")
            .and_then(|f| f.as_block())
            .map(|b| {
                b.iter()
                    .map(|e| GiantBuckleParameters::from_struct(&e))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(Self {
            unit,
            flags: root.read_int_any("flags").unwrap_or(0) as u32,
            accel_time: root.read_real("accel_time").unwrap_or(0.0),
            decel_time: root.read_real("decel_time").unwrap_or(0.0),
            minimum_speed_scale: root.read_real("minimum speed scale").unwrap_or(0.0),
            elevation_change_rate: root.read_real("elevation change rate").unwrap_or(0.0),
            max_vertical_reach: root.read_real("max_vertical_reach").unwrap_or(0.0),
            buckle_settings,
            ankle_ik_scale: root.read_real("ankle ik scale").unwrap_or(0.0),
        })
    }
}
