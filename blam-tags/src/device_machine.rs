//! `.device_machine` (`mach`) tag walker.
//!
//! Schema: `definitions/halo3_mcc/device_machine.json` →
//! `device_machine_struct_definition` (size 420, parent_tag `devi`).
//! Ares source: `source/devices/device_machines.h`.

use crate::device::DeviceDefinition;
use crate::file::TagFile;
use crate::object::ObjectDefinition;
use std::sync::Arc;

const MACHINE_GROUP: [u8; 4] = *b"mach";

#[derive(Debug)]
pub enum MachineError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
    MissingSubstruct { path: &'static str },
    ObjectDefinition(crate::object::ObjectDefinitionError),
}

impl std::fmt::Display for MachineError {
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

impl std::error::Error for MachineError {}

impl From<crate::object::ObjectDefinitionError> for MachineError {
    fn from(e: crate::object::ObjectDefinitionError) -> Self {
        Self::ObjectDefinition(e)
    }
}

/// Walked `device_machine_struct_definition` (size 420). Wraps a
/// `DeviceDefinition` parent + machine-specific authored fields.
#[derive(Debug, Clone, Default)]
pub struct MachineDefinition {
    pub device: Arc<DeviceDefinition>,
    /// `type` enum — engine `e_machine_type` (door, gear, platform, etc.).
    pub machine_type: i16,
    /// `flags` (word_flags) — machine-specific.
    pub flags: u16,
    /// `door open time:seconds`.
    pub door_open_time: f32,
    /// `door occlusion bounds` (fraction_bounds, lower/upper).
    pub door_occlusion_bounds_lower: f32,
    pub door_occlusion_bounds_upper: f32,
    pub elevator_node: i16,
    /// `pathfinding policy` enum.
    pub pathfinding_policy: i16,
}

impl MachineDefinition {
    pub fn from_tag(tag: &TagFile) -> Result<Self, MachineError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != MACHINE_GROUP {
            return Err(MachineError::WrongGroup {
                expected: MACHINE_GROUP,
                actual,
            });
        }
        let object = Arc::new(ObjectDefinition::from_tag(tag)?);
        let root = tag.root();
        let device_struct = root
            .descend("device")
            .ok_or(MachineError::MissingSubstruct { path: "device" })?;
        let device = Arc::new(DeviceDefinition::from_device_struct(object, &device_struct));

        // Schema declares `door occlusion bounds` as `fraction_bounds`
        // (two [0,1] floats), not real_bounds — caught at runtime by
        // deadlock.scenario load. Use the matching reader.
        let bounds = root.read_fraction_bounds("door occlusion bounds");
        Ok(Self {
            device,
            machine_type: root.read_int_any("type").unwrap_or(0) as i16,
            flags: root.read_int_any("flags").unwrap_or(0) as u16,
            door_open_time: root.read_real("door open time").unwrap_or(0.0),
            door_occlusion_bounds_lower: bounds.lower,
            door_occlusion_bounds_upper: bounds.upper,
            elevator_node: root.read_int_any("elevator node").unwrap_or(0) as i16,
            pathfinding_policy: root.read_int_any("pathfinding policy").unwrap_or(0) as i16,
        })
    }
}
