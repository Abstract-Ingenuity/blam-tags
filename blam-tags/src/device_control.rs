//! `.device_control` (`ctrl`) tag walker.
//!
//! Schema: `definitions/halo3_mcc/device_control.json` →
//! `device_control_struct_definition` (size 460, parent_tag `devi`).
//! Ares source: `source/devices/device_controls.h`.

use crate::device::DeviceDefinition;
use crate::file::TagFile;
use crate::object::ObjectDefinition;
use crate::typed_enums::Enum;
use std::sync::Arc;

const CONTROL_GROUP: [u8; 4] = *b"ctrl";

/// `control_types`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum ControlType {
    #[default]
    #[strum(serialize = "toggle switch")] ToggleSwitch = 0,
    #[strum(serialize = "on button")] OnButton = 1,
    #[strum(serialize = "off button")] OffButton = 2,
    #[strum(serialize = "call button")] CallButton = 3,
}

/// `control_triggers`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum ControlTriggers {
    #[default]
    #[strum(serialize = "touched by player")] TouchedByPlayer = 0,
    #[strum(serialize = "destroyed")] Destroyed = 1,
}

#[derive(Debug)]
pub enum ControlError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
    MissingSubstruct { path: &'static str },
    ObjectDefinition(crate::object::ObjectDefinitionError),
}

impl std::fmt::Display for ControlError {
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

impl std::error::Error for ControlError {}

impl From<crate::object::ObjectDefinitionError> for ControlError {
    fn from(e: crate::object::ObjectDefinitionError) -> Self {
        Self::ObjectDefinition(e)
    }
}

/// Walked `device_control_struct_definition` (size 460). Field order
/// matches `device_control.json` verbatim.
#[derive(Debug, Clone, Default)]
pub struct ControlDefinition {
    pub device: Arc<DeviceDefinition>,
    /// `type` enum (toggle/momentary/etc.).
    pub control_type: Enum<ControlType, i16>,
    /// `triggers when` enum.
    pub triggers_when: Enum<ControlTriggers, i16>,
    /// `call value:[0,1]`.
    pub call_value: f32,
    /// `action string` string_id.
    pub action_string: String,
    /// `on` (tag_reference) — fired on activation.
    pub on: String,
    /// `off` (tag_reference) — fired on deactivation.
    pub off: String,
    /// `deny` (tag_reference) — fired on denied activation.
    pub deny: String,
}

impl ControlDefinition {
    pub fn from_tag(tag: &TagFile) -> Result<Self, ControlError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != CONTROL_GROUP {
            return Err(ControlError::WrongGroup {
                expected: CONTROL_GROUP,
                actual,
            });
        }
        let object = Arc::new(ObjectDefinition::from_tag(tag)?);
        let root = tag.root();
        let device_struct = root
            .descend("device")
            .ok_or(ControlError::MissingSubstruct { path: "device" })?;
        let device = Arc::new(DeviceDefinition::from_device_struct(object, &device_struct));

        Ok(Self {
            device,
            control_type: root.read_enum("type"),
            triggers_when: root.read_enum("triggers when"),
            call_value: root.read_real("call value").unwrap_or(0.0),
            action_string: root.read_string_id("action string").unwrap_or_default(),
            on: root.read_tag_ref_path("on").unwrap_or_default(),
            off: root.read_tag_ref_path("off").unwrap_or_default(),
            deny: root.read_tag_ref_path("deny").unwrap_or_default(),
        })
    }
}
