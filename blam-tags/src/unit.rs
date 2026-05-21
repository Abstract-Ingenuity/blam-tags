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
use std::sync::Arc;

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
    pub flags: u16,
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
            flags: s.read_int_any("flags").unwrap_or(0) as u16,
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
    pub flags: u32,
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
    pub ai_seat_type: i16,
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
            flags: s.read_int_any("flags").unwrap_or(0) as u32,
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
            ai_seat_type: s.read_int_any("ai seat type").unwrap_or(0) as i16,
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
    pub flags: u8,
    pub bucket_type: i8,
    pub bucket_class: i8,
    pub point_count: i16,
}

impl CampaignMetagameBucket {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            flags: s.read_int_any("flags").unwrap_or(0) as u8,
            bucket_type: s.read_int_any("type").unwrap_or(0) as i8,
            bucket_class: s.read_int_any("class").unwrap_or(0) as i8,
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
    pub flags: u32,
    pub default_team: i16,
    pub constant_sound_volume: i16,
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
    pub motion_sensor_blip_size: i16,
    pub item_owner_size: i16,
    pub new_hud_interfaces: Vec<UnitHudReference>,
    pub grenade_velocity: f32,
    pub grenade_type: i16,
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
            flags: s.read_int_any("flags").unwrap_or(0) as u32,
            default_team: s.read_int_any("default team").unwrap_or(0) as i16,
            constant_sound_volume: s.read_int_any("constant sound volume").unwrap_or(0) as i16,
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
            motion_sensor_blip_size: s.read_int_any("motion sensor blip size").unwrap_or(0) as i16,
            item_owner_size: s.read_int_any("item owner size").unwrap_or(0) as i16,
            new_hud_interfaces: read_block_vec(
                s,
                "NEW HUD INTERFACES",
                UnitHudReference::from_struct,
            ),
            grenade_velocity: s.read_real("grenade velocity").unwrap_or(0.0),
            grenade_type: s.read_int_any("grenade type").unwrap_or(0) as i16,
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
