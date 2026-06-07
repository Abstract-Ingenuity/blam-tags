//! `decorator_set` (`dctr`) tag walker — one decorator-foliage definition
//! (e.g. thistle, wildgrass, rocks). Each .decorator_set carries a
//! render_model + texture + shader-flavor selection + LOD/wind tuning,
//! plus a `decorator_types` block whose entries pick which mesh subparts
//! get instanced and how they vary (scale/tilt/wind/color/etc).
//!
//! Pointed at by `scenario.decorators[i].sets[j].decorator_set` (per
//! `scenario/types.rs::DecoratorSetEntry`). Scenario-level placements
//! reference one of these sets by block index; each placement's
//! `type_index` then picks one of this set's `decorator_types[k]`.
//!
//! Schema reference: `definitions/halo3_mcc/decorator_set.json` and
//! `Ares/source/decorators/decorator_tag_definitions.h`.

use crate::api::TagStruct;
use crate::file::TagFile;
use crate::math::RealRgbColor;
use crate::typed_enums::{Enum, Flags};

#[derive(Debug)]
pub enum DecoratorSetError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
}

impl std::fmt::Display for DecoratorSetError {
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

impl std::error::Error for DecoratorSetError {}

const DECORATOR_SET_GROUP: [u8; 4] = *b"dctr";

/// `s_decorator_set::e_render_flags` (decorator_tag_definitions.h:32-37).
/// `decorator_set_render_flags_definition`.
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u8)]
pub enum DecoratorSetRenderFlags {
    /// Render two-sided (no back-face culling). Set on most leafy foliage.
    #[strum(serialize = "render two sided")] RenderTwoSided = 0,
    /// Gate the 10-sample lightprobe bake with a visibility pre-cast
    /// (schema: "takes twice as long to light"). Consumed in
    /// `decorators::bake::bake_placement`.
    #[strum(serialize = "dont sample light through geometry")] DontSampleLightThroughGeometry = 1,
}

/// `decorator_type_flags_definition`.
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum DecoratorTypeFlags {
    #[strum(serialize = "only on ground")] OnlyOnGround = 0,
    #[strum(serialize = "random rotation")] RandomRotation = 1,
    #[strum(serialize = "rotate x axis down")] RotateXAxisDown = 2,
    #[strum(serialize = "align to normal")] AlignToNormal = 3,
    #[strum(serialize = "align random")] AlignRandom = 4,
}

/// One of 6 dedicated decorator shader variants. The runtime picks
/// `decorator_render_<variant>.pixel_shader` based on this enum.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i8)]
pub enum RenderShader {
    #[default]
    #[strum(serialize = "wind + dynamic lights")] WindDynamicLights = 0,
    #[strum(serialize = "still + dynamic lights")] DynamicLights = 1,
    #[strum(serialize = "still + no lights")] Static = 2,
    #[strum(serialize = "still + sun light only")] DominantLightOnly = 3,
    #[strum(serialize = "wavy + dynamic lights")] WavyDynamicLights = 4,
    /// "shaded + dynamic lights" — used by thistle.
    #[strum(serialize = "shaded + dynamic lights")] ShadedDynamicLights = 5,
}

/// `decorator_set_lighting_sample_pattern_enum_definition`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i8)]
pub enum LightingSamplePattern {
    /// Default — sample lighting from below the placement (grass-like).
    #[default]
    #[strum(serialize = "ground default")] Ground = 0,
    /// Hanging foliage — sample from above (mossy overhangs).
    #[strum(serialize = "hanging")] Hanging = 1,
}

/// One entry in `decorator_types` — picks a mesh subpart from the
/// referenced render_model and adds per-type variation parameters.
#[derive(Debug, Clone, Default)]
pub struct DecoratorType {
    pub name: String,
    pub index: i32,
    /// Block index into the render_model's mesh table.
    pub mesh: i32,
    pub flags: Flags<DecoratorTypeFlags, u32>,
    pub scale_min: f32,
    pub scale_max: f32,
    pub tilt_min: f32,
    pub tilt_max: f32,
    pub wind_min: f32,
    pub wind_max: f32,
    pub color_0: RealRgbColor,
    pub color_1: RealRgbColor,
    pub color_2: RealRgbColor,
    pub ground_tint_min: f32,
    pub ground_tint_max: f32,
    pub hover_min: f32,
    pub hover_max: f32,
    /// Per-instance authoring exclusion radius — placements closer than
    /// this get culled at paint time. No runtime effect.
    pub minimum_distance_between_decorators: f32,
}

impl DecoratorType {
    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            name: s.read_string_id("name").unwrap_or_default(),
            index: s.read_int_any("index").unwrap_or(0) as i32,
            mesh: s.read_block_index("mesh") as i32,
            flags: s.try_read_flags("flags").unwrap_or_default(),
            scale_min: s.read_real("scale min").unwrap_or(1.0),
            scale_max: s.read_real("scale max").unwrap_or(1.0),
            tilt_min: s.read_real("tilt min").unwrap_or(0.0),
            tilt_max: s.read_real("tilt max").unwrap_or(0.0),
            wind_min: s.read_real("wind min").unwrap_or(0.0),
            wind_max: s.read_real("wind max").unwrap_or(0.0),
            color_0: s.read_rgb("color 0"),
            color_1: s.read_rgb("color 1"),
            color_2: s.read_rgb("color 2"),
            ground_tint_min: s.read_real("ground tint min").unwrap_or(0.0),
            ground_tint_max: s.read_real("ground tint max").unwrap_or(0.0),
            hover_min: s.read_real("hover min").unwrap_or(0.0),
            hover_max: s.read_real("hover max").unwrap_or(0.0),
            minimum_distance_between_decorators: s
                .read_real("minimum distance between decorators")
                .unwrap_or(0.0),
        }
    }
}

/// Decoded `.decorator_set` tag.
#[derive(Debug, Clone, Default)]
pub struct DecoratorSet {
    /// Tag-ref path to the `.render_model` whose meshes get instanced.
    pub render_model_path: String,
    /// Optional named instances within the render_model (block of
    /// `name: string_id` records). Picks specific subparts.
    pub render_model_instance_names: Vec<String>,
    pub render_model_instance_name_valid_count: i32,
    /// Tag-ref path to the `.bitmap` rendered onto the foliage.
    pub texture_path: String,

    pub render_flags: Flags<DecoratorSetRenderFlags, u8>,
    pub render_shader: Enum<RenderShader, i8>,
    pub lighting_sample_pattern: Enum<LightingSamplePattern, i8>,

    /// `translucency` — 0 = opaque, 1 = both sides equal intensity. Only
    /// affects dynamic-light shaders. `_a/_b/_c` are post-processed
    /// derived values (don't touch).
    pub translucency: f32,
    pub translucency_a: f32,
    pub translucency_b: f32,
    pub translucency_c: f32,

    /// Wind / wave animation params (used by Wavy / Wind shader variants).
    pub wavelength_x: f32,
    pub wavelength_y: f32,
    pub wave_speed: f32,
    pub wave_frequency: f32,

    /// Shaded-variant tuning. Dark side intensity / bright side bonus.
    pub shaded_dark: f32,
    pub shaded_bright: f32,

    /// LOD fade — start_fade < end_fade in world units; placements past
    /// end_fade get skipped entirely.
    pub start_fade_distance: f32,
    pub end_fade_distance: f32,
    /// `early_cull` is a [0,1] percentage — vertices fade out this much
    /// sooner than end_fade.
    pub early_cull: f32,
    /// LOD chunking grid size — placements get bucketed into block_x/y
    /// cells of this size for batched culling.
    pub cull_block_size: f32,

    /// Per-decorator-type records — selected by
    /// `ScenarioDecoratorPlacement::type_index`.
    pub decorator_types: Vec<DecoratorType>,
}

impl DecoratorSet {
    pub fn from_tag(tag: &TagFile) -> Result<Self, DecoratorSetError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != DECORATOR_SET_GROUP {
            return Err(DecoratorSetError::WrongGroup {
                expected: DECORATOR_SET_GROUP,
                actual,
            });
        }
        let mut out = Self::from_struct(&tag.root());
        out.apply_engine_postprocess();
        Ok(out)
    }

    /// Mirror tool.exe `sub_140637DB0`'s tag-group postprocess: derive
    /// `translucency_a/b/c` at top-level struct offsets +52/+56/+60 from
    /// the authored `translucency` at +64. The engine writes these
    /// **once at cache-bake time**; source-format tags (TagTool-extracted)
    /// leave them as zero, so protomorph would run decorator dynamic-light
    /// shaders with zero coefficients without this.
    ///
    /// Engine source:
    /// ```c
    /// s = translucency;                                  // +64
    /// v15 = ((1-s) + 1.0) * 0.5 / ((1-s) + 1.0 + (1-s)*(1-s)*0.25);
    /// translucency_a = v15;                              // +52
    /// translucency_b = v15 * (1-s);                      // +56
    /// translucency_c = v15 * (1-s)*(1-s)*0.25 + s*0.5;   // +60
    /// ```
    ///
    /// Verified against `grass_thick.decorator_set` which authors
    /// `translucency=0.8` and stores A=B=C=0 in source format — engine
    /// post-derived values are A≈0.4959, B≈0.0992, C≈0.4050.
    ///
    /// The engine function also runs render-model-instance-index
    /// validation and palette renumbering — those are diagnostics /
    /// runtime-asset binding and don't transform tag data; we skip them.
    pub fn apply_engine_postprocess(&mut self) {
        let s = self.translucency;
        let one_minus_s = 1.0 - s;
        let denom = one_minus_s + 1.0 + one_minus_s * one_minus_s * 0.25;
        let v15 = (one_minus_s + 1.0) * 0.5 / denom;
        self.translucency_a = v15;
        self.translucency_b = v15 * one_minus_s;
        self.translucency_c = v15 * one_minus_s * one_minus_s * 0.25 + s * 0.5;
    }

    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        let render_model_instance_names = s
            .field("render model instance names")
            .and_then(|f| f.as_block())
            .map(|b| {
                let mut out = Vec::with_capacity(b.len());
                for i in 0..b.len() {
                    if let Some(elem) = b.element(i) {
                        out.push(elem.read_string_id("name").unwrap_or_default());
                    }
                }
                out
            })
            .unwrap_or_default();

        let decorator_types = s
            .field("decorator types")
            .and_then(|f| f.as_block())
            .map(|b| {
                let mut out = Vec::with_capacity(b.len());
                for i in 0..b.len() {
                    if let Some(elem) = b.element(i) {
                        out.push(DecoratorType::from_struct(&elem));
                    }
                }
                out
            })
            .unwrap_or_default();

        Self {
            render_model_path: s.read_tag_ref_path("render model").unwrap_or_default(),
            render_model_instance_names,
            render_model_instance_name_valid_count: s
                .read_int_any("render model instance name valid count")
                .unwrap_or(0) as i32,
            texture_path: s.read_tag_ref_path("texture").unwrap_or_default(),
            render_flags: s.try_read_flags("render flags").unwrap_or_default(),
            render_shader: s.read_enum("render shader"),
            // Added after the 2007 schema rev — absent in launch-era
            // decorator_sets (e.g. riverworld's wildgrass/thistle), so read
            // optionally and default to `ground default` (engine's behavior
            // for the missing field).
            lighting_sample_pattern: s
                .try_read_enum("light sampling pattern")
                .unwrap_or_default(),
            translucency: s.read_real("translucency").unwrap_or(0.5),
            translucency_a: s.read_real("translucency A").unwrap_or(0.0),
            translucency_b: s.read_real("translucency B").unwrap_or(0.0),
            translucency_c: s.read_real("translucency C").unwrap_or(0.0),
            wavelength_x: s.read_real("wavelength X").unwrap_or(0.0),
            wavelength_y: s.read_real("wavelength Y").unwrap_or(0.0),
            wave_speed: s.read_real("wave speed").unwrap_or(0.0),
            wave_frequency: s.read_real("wave frequency").unwrap_or(0.0),
            shaded_dark: s.read_real("shaded dark").unwrap_or(0.5),
            shaded_bright: s.read_real("shaded bright").unwrap_or(1.0),
            start_fade_distance: s.read_real("start fade").unwrap_or(7.0),
            end_fade_distance: s.read_real("end fade").unwrap_or(12.0),
            early_cull: s.read_real("early cull").unwrap_or(0.0),
            cull_block_size: s.read_real("cull block size").unwrap_or(5.0),
            decorator_types,
        }
    }

    pub fn is_two_sided(&self) -> bool {
        self.render_flags.contains(DecoratorSetRenderFlags::RenderTwoSided)
    }

    pub fn dont_sample_lighting_through_geometry(&self) -> bool {
        self.render_flags
            .contains(DecoratorSetRenderFlags::DontSampleLightThroughGeometry)
    }
}
