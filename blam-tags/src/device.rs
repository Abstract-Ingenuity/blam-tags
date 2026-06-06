//! `device_struct_definition` substruct — shared parent of
//! `.device_machine` (mach), `.device_control` (ctrl), and
//! `.device_terminal` (term).
//!
//! Schema: `definitions/halo3_mcc/device.json` →
//! `device_struct_definition` (parent_tag `obje`).
//! Ares source: `source/devices/devices.h` (device_struct_definition).
//!
//! Composition: each derived device tag's root holds a `device`
//! substruct, which holds an `object` substruct. The intermediate is
//! not a standalone instantiable tag — it's a layout-only parent.

use crate::api::TagStruct;
use crate::object::ObjectDefinition;
use crate::typed_enums::Flags;
use std::sync::Arc;

/// `device_definition_flags` (long_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum DeviceDefinitionFlags {
    #[strum(serialize = "position loops")] PositionLoops = 0,
    #[strum(serialize = "(unused)")] Unused = 1,
    #[strum(serialize = "allow interpolation")] AllowInterpolation = 2,
}

/// `device_lightmap_flags` (word_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u16)]
pub enum DeviceLightmapFlags {
    #[strum(serialize = "don't use in lightmap")] DontUseInLightmap = 0,
    #[strum(serialize = "don't use in lightprobe")] DontUseInLightprobe = 1,
}

/// Walked `device_struct_definition`. Field order matches schema
/// verbatim (`device.json:62-128`).
#[derive(Debug, Clone, Default)]
pub struct DeviceDefinition {
    /// Inherited `object_struct_definition` body, `Arc`-wrapped so
    /// derived `MachineDefinition` / `ControlDefinition` /
    /// `TerminalDefinition` share the same parsed copy.
    pub object: Arc<ObjectDefinition>,

    /// `device_struct_definition.flags` (long_flags).
    pub flags: Flags<DeviceDefinitionFlags, u32>,

    /// `power transition time:seconds` — divisor for the
    /// `change_in_power` compute case (engine `abs(power_velocity) /
    /// power_transition_time`).
    pub power_transition_time: f32,
    /// `power acceleration time:seconds`.
    pub power_acceleration_time: f32,
    /// `position transition time:seconds` — divisor for the
    /// `change_in_position` compute case (default-path).
    pub position_transition_time: f32,
    /// `position acceleration time:seconds`.
    pub position_acceleration_time: f32,
    /// `depowered position transition time:seconds`.
    pub depowered_position_transition_time: f32,
    /// `depowered position acceleration time:seconds`.
    pub depowered_position_acceleration_time: f32,

    /// `lightmap flags` (word_flags).
    pub lightmap_flags: Flags<DeviceLightmapFlags, u16>,

    // -- transition effect tag refs (Halo 3 had ~12 allowed effect groups) --
    /// `open (up)` (tag_reference).
    pub open_up: String,
    /// `close (down)` (tag_reference).
    pub close_down: String,
    /// `opened` (tag_reference).
    pub opened: String,
    /// `closed` (tag_reference).
    pub closed: String,
    /// `depowered` (tag_reference).
    pub depowered: String,
    /// `repowered` (tag_reference).
    pub repowered: String,

    /// `delay time:seconds` — divisor for the `delay` compute case
    /// (engine `game_ticks_to_seconds(delay_ticks) / delay_time`).
    pub delay_time: f32,
    /// `delay effect` (tag_reference).
    pub delay_effect: String,
    /// `automatic activation radius:world units`.
    pub automatic_activation_radius: f32,
}

impl DeviceDefinition {
    /// Parse from a tag's `device` substruct (descend by the caller
    /// — typically `tag.root().descend("device")`). Used by each
    /// derived definition reader (`MachineDefinition::from_tag`, etc.)
    /// after they descend into their own root.
    pub fn from_device_struct(
        object: Arc<ObjectDefinition>,
        s: &TagStruct<'_>,
    ) -> Self {
        Self {
            object,
            flags: s.try_read_flags("flags").unwrap_or_default(),
            power_transition_time: s.read_real("power transition time").unwrap_or(0.0),
            power_acceleration_time: s.read_real("power acceleration time").unwrap_or(0.0),
            position_transition_time: s.read_real("position transition time").unwrap_or(0.0),
            position_acceleration_time: s.read_real("position acceleration time").unwrap_or(0.0),
            depowered_position_transition_time: s
                .read_real("depowered position transition time")
                .unwrap_or(0.0),
            depowered_position_acceleration_time: s
                .read_real("depowered position acceleration time")
                .unwrap_or(0.0),
            lightmap_flags: s.try_read_flags("lightmap flags").unwrap_or_default(),
            open_up: s.read_tag_ref_path("open (up)").unwrap_or_default(),
            close_down: s.read_tag_ref_path("close (down)").unwrap_or_default(),
            opened: s.read_tag_ref_path("opened").unwrap_or_default(),
            closed: s.read_tag_ref_path("closed").unwrap_or_default(),
            depowered: s.read_tag_ref_path("depowered").unwrap_or_default(),
            repowered: s.read_tag_ref_path("repowered").unwrap_or_default(),
            delay_time: s.read_real("delay time").unwrap_or(0.0),
            delay_effect: s.read_tag_ref_path("delay effect").unwrap_or_default(),
            automatic_activation_radius: s
                .read_real("automatic activation radius")
                .unwrap_or(0.0),
        }
    }
}
