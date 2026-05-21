//! `.device_terminal` (`term`) tag walker.
//!
//! Schema: `definitions/halo3_mcc/device_terminal.json` →
//! `device_terminal_struct_definition` (size 720, parent_tag `devi`).
//! Ares source: `source/devices/device_terminals.h`.

use crate::api::TagStruct;
use crate::device::DeviceDefinition;
use crate::file::TagFile;
use crate::object::ObjectDefinition;
use std::sync::Arc;

const TERMINAL_GROUP: [u8; 4] = *b"term";

#[derive(Debug)]
pub enum TerminalError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
    MissingSubstruct { path: &'static str },
    ObjectDefinition(crate::object::ObjectDefinitionError),
}

impl std::fmt::Display for TerminalError {
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

impl std::error::Error for TerminalError {}

impl From<crate::object::ObjectDefinitionError> for TerminalError {
    fn from(e: crate::object::ObjectDefinitionError) -> Self {
        Self::ObjectDefinition(e)
    }
}

/// Per-difficulty terminal content. Same shape repeated 4× in the
/// schema (easy/normal/hard/legendary).
#[derive(Debug, Clone, Default)]
pub struct TerminalDifficultyContent {
    /// `dummy strings` tag-ref path (group `unic`).
    pub dummy_strings: String,
    /// `story strings` tag-ref path.
    pub story_strings: String,
    /// `dummy content` (word_flags) — bitmap selector.
    pub dummy_content: u16,
    /// `story content` (word_flags) — bitmap selector.
    pub story_content: u16,
    /// `dummy image frame` (short_integer).
    pub dummy_image_frame: i16,
    /// `story image frame` (short_integer).
    pub story_image_frame: i16,
    /// `error strings` tag-ref path.
    pub error_strings: String,
}

impl TerminalDifficultyContent {
    fn read(s: &TagStruct<'_>, suffix: &str) -> Self {
        let dummy_strings = s
            .read_tag_ref_path(&format!("dummy strings ({suffix})"))
            .unwrap_or_default();
        let story_strings = s
            .read_tag_ref_path(&format!("story strings ({suffix})"))
            .unwrap_or_default();
        let dummy_content = s
            .read_int_any(&format!("dummy content ({suffix})"))
            .unwrap_or(0) as u16;
        let story_content = s
            .read_int_any(&format!("story content ({suffix})"))
            .unwrap_or(0) as u16;
        let dummy_image_frame = s
            .read_int_any(&format!("dummy image frame ({suffix})"))
            .unwrap_or(0) as i16;
        let story_image_frame = s
            .read_int_any(&format!("story image frame ({suffix})"))
            .unwrap_or(0) as i16;
        let error_strings = s
            .read_tag_ref_path(&format!("error strings ({suffix})"))
            .unwrap_or_default();
        Self {
            dummy_strings,
            story_strings,
            dummy_content,
            story_content,
            dummy_image_frame,
            story_image_frame,
            error_strings,
        }
    }
}

/// Walked `device_terminal_struct_definition` (size 720). Wraps a
/// `DeviceDefinition` parent + terminal-specific authored fields and
/// per-difficulty content blocks.
#[derive(Debug, Clone, Default)]
pub struct TerminalDefinition {
    pub device: Arc<DeviceDefinition>,

    /// `bah bah` (long_integer) — engine internal name.
    pub bah_bah: i32,
    /// `action string` string_id.
    pub action_string: String,
    /// `terminal number` short_integer.
    pub terminal_number: i16,
    /// `exposure` (real) — exposure override while viewing terminal.
    pub exposure: f32,

    /// `activation sound` (tag_ref → snd!).
    pub activation_sound: String,
    /// `deactivation sound` (tag_ref → snd!).
    pub deactivation_sound: String,
    /// `translation sound`.
    pub translation_sound: String,
    /// `untranslation sound`.
    pub untranslation_sound: String,
    /// `error_sound`.
    pub error_sound: String,

    pub easy: TerminalDifficultyContent,
    pub normal: TerminalDifficultyContent,
    pub hard: TerminalDifficultyContent,
    pub legendary: TerminalDifficultyContent,
}

impl TerminalDefinition {
    pub fn from_tag(tag: &TagFile) -> Result<Self, TerminalError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != TERMINAL_GROUP {
            return Err(TerminalError::WrongGroup {
                expected: TERMINAL_GROUP,
                actual,
            });
        }
        let object = Arc::new(ObjectDefinition::from_tag(tag)?);
        let root = tag.root();
        let device_struct = root
            .descend("device")
            .ok_or(TerminalError::MissingSubstruct { path: "device" })?;
        let device = Arc::new(DeviceDefinition::from_device_struct(object, &device_struct));

        Ok(Self {
            device,
            bah_bah: root.read_int_any("bah bah").unwrap_or(0) as i32,
            action_string: root.read_string_id("action string").unwrap_or_default(),
            terminal_number: root.read_int_any("terminal number").unwrap_or(0) as i16,
            exposure: root.read_real("exposure").unwrap_or(0.0),
            activation_sound: root.read_tag_ref_path("activation sound").unwrap_or_default(),
            deactivation_sound: root.read_tag_ref_path("deactivation sound").unwrap_or_default(),
            translation_sound: root.read_tag_ref_path("translation sound").unwrap_or_default(),
            untranslation_sound: root.read_tag_ref_path("untranslation sound").unwrap_or_default(),
            error_sound: root.read_tag_ref_path("error_sound").unwrap_or_default(),
            easy: TerminalDifficultyContent::read(&root, "easy"),
            normal: TerminalDifficultyContent::read(&root, "normal"),
            hard: TerminalDifficultyContent::read(&root, "hard"),
            legendary: TerminalDifficultyContent::read(&root, "legendary"),
        })
    }
}
