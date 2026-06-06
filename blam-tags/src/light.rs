//! `light` (`ligh`) tag walker — runtime light definition referenced
//! by scenario `lights[]` placements.
//!
//! Drives two engine paths protomorph cares about:
//!
//! 1. **`c_lights_view::submit_visibility_and_render @ 0x1806C6930`** —
//!    per-light shadow scheduler. Gates on `flags & shadow_casting`
//!    (bit 1) AND `type == frustum` AND `frustum_field_of_view < π`.
//!
//! 2. **`light_submit_lens_flares @ 0x18086A850`** — submits a lens
//!    flare entry to `g_lens_flare_globals.lens_flare_array` for each
//!    light tag whose `Lens Flare` reference is non-null.
//!
//! Schema reference: `definitions/halo3_mcc/light.json` (346 lines).
//! IDA cross-checks: light_definition struct guid
//! `f2b91e672d48afb6250f2d90a165b6ed`, size 148.

use crate::api::TagStruct;
use crate::fields::TagFieldType;
use crate::file::TagFile;
use crate::typed_enums::{Enum, Flags};
use crate::math::RealRgbColor;
use crate::tag_function::TagFunction;

/// Errors from `light` tag walking.
#[derive(Debug)]
pub enum LightError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
}

impl std::fmt::Display for LightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongGroup { expected, actual } => write!(
                f,
                "expected group '{}', got '{}'",
                std::str::from_utf8(expected).unwrap_or("?"),
                std::str::from_utf8(actual).unwrap_or("?"),
            ),
        }
    }
}

impl std::error::Error for LightError {}

const LIGHT_GROUP: [u8; 4] = *b"ligh";

/// `light_definition_flags` — one variant per bit. Discriminants are the
/// canonical bit indices from `definitions/halo3_mcc/light.json`. The
/// per-light shadow gate keys off [`Self::ShadowCasting`] (this is the
/// TAG bit; per-instance attenuation flags live on
/// `generic_light_instances`).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum LightDefinitionFlags {
    #[strum(serialize = "allow shadows and gels")] AllowShadowsAndGels = 0,
    #[strum(serialize = "shadow casting")] ShadowCasting = 1,
    #[strum(serialize = "render first person only")] RenderFirstPersonOnly = 2,
    #[strum(serialize = "render third person only")] RenderThirdPersonOnly = 3,
    #[strum(serialize = "dont render splitscreen")] DontRenderSplitscreen = 4,
    #[strum(serialize = "render while active camo")] RenderWhileActiveCamo = 5,
    #[strum(serialize = "render in multiplayer override")] RenderInMultiplayerOverride = 6,
    #[strum(serialize = "move to camera in first person")] MoveToCameraInFirstPerson = 7,
    #[strum(serialize = "never priority cull")] NeverPriorityCull = 8,
    #[strum(serialize = "affected by game_can_use_flashlights")] AffectedByGameCanUseFlashlights = 9,
}

/// `type` enum — engine `light_type_enum_definition` (`short_enum`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum LightType {
    /// Point/spherical light (radial falloff).
    #[default]
    #[strum(serialize = "sphere")] Sphere = 0,
    /// Spotlight cone (`frustum_field_of_view` defines the cone angle).
    #[strum(serialize = "frustum")] Frustum = 1,
}

/// Walked `light_struct_definition`. The `light_color_function_struct`
/// and `light_scalar_function_struct` sub-structs are reduced to their
/// authored constant values — the engine evaluates the curve at runtime
/// against time/age, but the vast majority of light tags use Constant
/// functions. Non-constant curves return the curve's clamp midpoint as
/// a stop-gap; revisit when an animated light shows up.
#[derive(Debug, Clone, Default)]
pub struct LightDefinition {
    /// `light_definition_flags`. Test with
    /// `.contains(LightDefinitionFlags::*)`.
    pub flags: Flags<LightDefinitionFlags, u32>,

    pub light_type: Enum<LightType, i16>,

    /// World-units distance at which the light is fully attenuated.
    pub maximum_distance: f32,
    /// Width of the frustum at the near plane (frustum lights only).
    pub frustum_near_width: f32,
    /// Vertical stretch of the gel (1.0 = aspect ratio matches gel).
    pub frustum_height_scale: f32,
    /// Horizontal cone angle in **degrees** as authored. Caller-side
    /// convert to radians before per-light projection math (engine
    /// shadow path: gate `< π` after deg→rad conversion).
    /// 0.0 = no spread (straight beam).
    pub frustum_field_of_view: f32,

    /// Authored RGB tint at evaluation time 0. Linear (engine gamma-
    /// corrects on submit per material).
    pub color: RealRgbColor,
    /// Authored intensity scalar at evaluation time 0.
    pub intensity: f32,

    /// `gel bitmap` — projected texture for spotlights / animated
    /// projectors. Tag-ref path; empty when unauthored.
    pub gel_bitmap: String,

    /// World units of effective light source size — small values
    /// produce hot near-field and rapid falloff.
    pub distance_diffusion: f32,
    /// `< 1.0` for sharp gel/cone edges, `> 1.0` for soft edges.
    pub angular_smoothness: f32,
    /// Fraction `[0, 1]` of light energy distributed spherically as
    /// ambient (vs directional).
    pub percent_spherical: f32,

    /// `Lens Flare` attachment — tag-ref path to a `.lens_flare`
    /// (group `lens`) tag. Empty when unauthored. `light_submit_lens_flares`
    /// walks every active light with a non-empty value here.
    pub lens_flare: String,
}

impl LightDefinition {
    pub fn from_tag(tag: &TagFile) -> Result<Self, LightError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != LIGHT_GROUP {
            return Err(LightError::WrongGroup { expected: LIGHT_GROUP, actual });
        }
        Ok(Self::from_struct(&tag.root()))
    }

    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        let flags: Flags<LightDefinitionFlags, u32> =
            s.try_read_flags("flags").unwrap_or_default();
        let light_type: Enum<LightType, i16> = s.read_enum("type");
        let maximum_distance = s.read_real("maximum distance").unwrap_or(0.0);
        let frustum_near_width = s.read_real("frustum near width").unwrap_or(0.0);
        let frustum_height_scale = s.read_real("frustum height scale").unwrap_or(1.0);
        let frustum_field_of_view = s.read_real("frustum field of view").unwrap_or(0.0);

        let color = read_light_color(s, "color");
        let intensity = read_light_scalar(s, "intensity");

        let gel_bitmap = s.read_tag_ref_path("gel bitmap").unwrap_or_default();

        let distance_diffusion = s.read_real("distance diffusion").unwrap_or(1.0);
        let angular_smoothness = s.read_real("angular smoothness").unwrap_or(1.0);
        let percent_spherical = s.read_real("percent spherical").unwrap_or(0.0);

        let lens_flare = s.read_tag_ref_path("Lens Flare").unwrap_or_default();

        Self {
            flags,
            light_type,
            maximum_distance,
            frustum_near_width,
            frustum_height_scale,
            frustum_field_of_view,
            color,
            intensity,
            gel_bitmap,
            distance_diffusion,
            angular_smoothness,
            percent_spherical,
            lens_flare,
        }
    }

    /// True if `flags & shadow_casting`. The engine's per-light shadow
    /// gate at `c_lights_view::submit_visibility_and_render` predicates
    /// off this bit (NOT the per-instance flags on `generic_light_instances`).
    pub fn casts_shadows(&self) -> bool {
        self.flags.contains(LightDefinitionFlags::ShadowCasting)
    }

    /// True if the light has a non-empty lens flare attachment.
    /// `light_submit_lens_flares` skips lights with empty references.
    pub fn has_lens_flare(&self) -> bool {
        !self.lens_flare.is_empty()
    }

    /// True if this is a frustum-shaped light (cone). The engine's
    /// per-light shadow path requires frustum AND `fov < π`.
    pub fn is_frustum(&self) -> bool {
        self.light_type == LightType::Frustum
    }
}

/// Walk a `light_color_function_struct` field and return the authored
/// constant RGB. Non-constant functions return the gradient's first
/// stop (`m_colors[0]`) as a reasonable default.
fn read_light_color(parent: &TagStruct<'_>, name: &str) -> RealRgbColor {
    parent
        .field(name)
        .and_then(|f| f.as_struct())
        .and_then(|color_struct| inner_mapping_function(&color_struct))
        .map(|func| color_from_function(&func))
        .unwrap_or(RealRgbColor { red: 1.0, green: 1.0, blue: 1.0 })
}

/// Walk a `light_scalar_function_struct` field and return the authored
/// constant scalar. Falls back to 1.0 if the function blob is missing.
fn read_light_scalar(parent: &TagStruct<'_>, name: &str) -> f32 {
    parent
        .field(name)
        .and_then(|f| f.as_struct())
        .and_then(|scalar_struct| inner_mapping_function(&scalar_struct))
        .map(|func| func.as_constant().unwrap_or_else(|| {
            // Range-mapped curve — return clamp midpoint. Engine
            // evaluates against runtime time, but most light tags are
            // constant; this only fires for animated lights.
            let h = func.header();
            0.5 * (h.clamp_range_min + h.clamp_range_max)
        }))
        .unwrap_or(1.0)
}

/// Reach into a `light_*_function_struct` and pull the
/// `mapping_function::data` blob as a parsed [`TagFunction`].
///
/// The schema declares TWO same-named "Mapping" fields inside the
/// outer function struct — a `custom` marker (group_tag `fned`) and
/// the real `mapping_function` struct that follows it. `field("Mapping")`
/// returns the marker first, so we walk by type instead.
fn inner_mapping_function(outer: &TagStruct<'_>) -> Option<TagFunction> {
    let mapping = outer
        .fields()
        .find(|f| f.field_type() == TagFieldType::Struct)?
        .as_struct()?;
    mapping.field("data").and_then(|f| f.as_function())
}

/// Decode a [`TagFunction`]'s `colors[0]` slot as ARGB-packed RGB.
/// `m_colors[0]` carries the first authored gradient stop; for
/// constant-color lights this is the single authored value. Engine
/// pixel32 layout is `0xAARRGGBB`.
fn color_from_function(func: &TagFunction) -> RealRgbColor {
    let packed = func.header().colors[0];
    if packed == 0 {
        return RealRgbColor { red: 1.0, green: 1.0, blue: 1.0 };
    }
    let r = ((packed >> 16) & 0xff) as f32 / 255.0;
    let g = ((packed >> 8) & 0xff) as f32 / 255.0;
    let b = (packed & 0xff) as f32 / 255.0;
    RealRgbColor { red: r, green: g, blue: b }
}
