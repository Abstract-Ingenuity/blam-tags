//! `light_volume_system` (`ltvl`) tag walker — collection of light
//! volume definitions referenced by `effect_part_definition` with
//! `runtime_tag_reference_base_class_tag = b"ltvl"`.
//!
//! ## Runtime hookup
//!
//! - `c_light_volume_system::create @ 0x1804976C0` allocates a runtime
//!   `c_light_volume_system` (44B) bound to the parent effect.
//! - `c_light_volume_system::submit @ 0x180497EF0` adds the system to
//!   the transparency renderer during opaque + transparent passes.
//! - `c_light_volume::render @ 0x180499110` invokes the per-volume
//!   draw call using light_volume_fx.hlsl with sin_view_angle
//!   overdraw compensation (Tier 7 of effects port).
//!
//! ## Schema shape
//!
//! Root carries a single `light_volumes[]` block (up to 16 entries).
//! Each `c_light_volume_definition` (380B) has:
//! - `light_volume name^` (string_id)
//! - Embedded `c_render_method_shader_light_volume` (40B inline)
//! - `appearance flags` (word_flags)
//! - `brightness ratio` — avg brightness head-on vs side-view
//! - 8 `EditableProperty` curves: length, offset, profile_density,
//!   profile_length, profile_thickness, profile_color, profile_alpha,
//!   profile_intensity. All authored against `c_light_volume_states`.
//! - Runtime gpu_data block (resolved by tool.exe).

use crate::api::TagStruct;
use crate::effects_properties::EditableProperty;
use crate::file::TagFile;
use crate::render_method::{RenderMethod, RenderMethodError};

const LTVL_GROUP: [u8; 4] = *b"ltvl";

#[derive(Debug)]
pub enum LightVolumeSystemError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
    RenderMethod(RenderMethodError),
}

impl std::fmt::Display for LightVolumeSystemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongGroup { expected, actual } => write!(
                f,
                "light_volume_system: wrong tag group (expected {:?}, got {:?})",
                std::str::from_utf8(expected).unwrap_or("????"),
                std::str::from_utf8(actual).unwrap_or("????"),
            ),
            Self::RenderMethod(e) => write!(f, "light_volume_system: actual shader: {e}"),
        }
    }
}

impl std::error::Error for LightVolumeSystemError {}

impl From<RenderMethodError> for LightVolumeSystemError {
    fn from(e: RenderMethodError) -> Self {
        Self::RenderMethod(e)
    }
}

/// One light_volume definition entry — 380B `c_light_volume_definition`.
#[derive(Debug, Clone, Default)]
pub struct LightVolumeDefinition {
    pub name: String,
    /// Embedded `c_render_method_shader_light_volume` — group_tag
    /// stamped to `b"rmlv"` after walk.
    pub shader: Option<RenderMethod>,
    /// `appearance flags` (word_flags) — engine-side bits for fog,
    /// double-sided, etc.
    pub appearance_flags: u16,
    /// Average head-on vs side-view brightness ratio. Drives
    /// sin_view_angle overdraw compensation in light_volume_fx.hlsl
    /// (per HLSL inventory line 165-169).
    pub brightness_ratio: f32,
    // ---- 8 property curves ----
    pub length: EditableProperty,
    pub offset: EditableProperty,
    pub profile_density: EditableProperty,
    pub profile_length: EditableProperty,
    pub profile_thickness: EditableProperty,
    pub profile_color: EditableProperty,
    pub profile_alpha: EditableProperty,
    pub profile_intensity: EditableProperty,
    // ---- runtime-resolved ----
    /// tool.exe-resolved bitmask of which properties are constant per profile.
    pub runtime_constant_per_profile_properties: i32,
    /// tool.exe-resolved bitmask of which state inputs are referenced.
    pub runtime_used_states: i32,
    /// tool.exe-resolved cap on profile count for GPU buffer sizing.
    pub runtime_max_profile_count: i32,
}

/// Walked `light_volume_system` (ltvl) tag.
#[derive(Debug, Clone, Default)]
pub struct LightVolumeSystem {
    /// Up to 16 light_volume definitions per system.
    pub definitions: Vec<LightVolumeDefinition>,
}

impl LightVolumeSystem {
    pub fn from_tag(tag: &TagFile) -> Result<Self, LightVolumeSystemError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != LTVL_GROUP {
            return Err(LightVolumeSystemError::WrongGroup { expected: LTVL_GROUP, actual });
        }
        Self::from_struct(&tag.root())
    }

    pub fn from_struct(s: &TagStruct<'_>) -> Result<Self, LightVolumeSystemError> {
        let block = match s.field("light_volumes").and_then(|f| f.as_block()) {
            Some(b) => b,
            None => return Ok(Self::default()),
        };
        let mut definitions = Vec::with_capacity(block.len());
        for i in 0..block.len() {
            if let Some(elem) = block.element(i) {
                definitions.push(LightVolumeDefinition::from_struct(&elem)?);
            }
        }
        Ok(Self { definitions })
    }
}

impl LightVolumeDefinition {
    pub fn from_struct(s: &TagStruct<'_>) -> Result<Self, LightVolumeSystemError> {
        // Embedded shader sub-struct. Walker stamps the group_tag
        // explicitly since the outer tag context can't infer rmlv.
        let shader_struct = s
            .descend("actual shader")
            .or_else(|| s.descend("actual shader?"));
        let shader = match shader_struct {
            Some(view) => {
                let mut rm = RenderMethod::from_struct(&view)?;
                rm.group_tag = u32::from_be_bytes(*b"rmlv");
                Some(rm)
            }
            None => None,
        };

        Ok(Self {
            name: s
                .read_string_id("light_volume name")
                .or_else(|| s.read_string_id("light_volume name^"))
                .unwrap_or_default(),
            shader,
            appearance_flags: s.read_int_any("appearance flags").unwrap_or(0) as u16,
            brightness_ratio: s.read_real("brightness ratio").unwrap_or(0.0),
            length: read_property(s, "length"),
            offset: read_property(s, "offset"),
            profile_density: read_property(s, "profile_density"),
            profile_length: read_property(s, "profile_length"),
            profile_thickness: read_property(s, "profile_thickness"),
            profile_color: read_property(s, "profile_color"),
            profile_alpha: read_property(s, "profile_alpha"),
            profile_intensity: read_property(s, "profile_intensity"),
            runtime_constant_per_profile_properties: s
                .read_int_any("runtime m_constant_per_profile_properties")
                .unwrap_or(0) as i32,
            runtime_used_states: s.read_int_any("runtime m_used_states").unwrap_or(0) as i32,
            runtime_max_profile_count: s
                .read_int_any("runtime m_max_profile_count")
                .unwrap_or(0) as i32,
        })
    }
}

fn read_property(parent: &TagStruct<'_>, name: &str) -> EditableProperty {
    parent
        .field(name)
        .and_then(|f| f.as_struct())
        .map(|inner| EditableProperty::from_struct(&inner))
        .unwrap_or_default()
}
