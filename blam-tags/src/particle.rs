//! `particle` (`prt3`) tag walker — per-particle definition that the
//! effect particle_systems reference. Engine `c_particle_definition`
//! (404 bytes) per Ares `source/effects/particle.h` +
//! `particle_definitions.h`.
//!
//! Layout summary (offsets are runtime engine layout, not authored):
//! - main flags + appearance flags + billboard style + sequence range
//! - center_offset, curvature, angle_fade, motion_blur scales
//! - shader (embedded `c_render_method` — handled via existing
//!   [`crate::render_method::RenderMethod`] walker)
//! - 7 property slots (aspect / color / intensity / alpha / frame_index
//!   / animation_rate / palette_animation) — each 32B
//!   `c_particle_property`. P1 captures constants + state inputs;
//!   per-frame curve evaluation lives in the protomorph particle
//!   subsystem (P3.T3+).
//! - model reference (`pmdf` for mesh particles)
//! - GPU sprite/frame UV corners (s_gpu_data)
//! - 4 attachment slots (effe/snd!/material_effect on birth/collision/death)
//!
//! Riverworld coverage: 3 prt3 tags exercised by the spine —
//! `rolling_mist`, `mist`, `water_spray` (all under
//! `levels/multi/riverworld/fx/waterfall/particles/`). Each uses the
//! `shaders\particle` render-method template family with different
//! option tuples (`_1_3_0_0_1_1_1_0_0` / `_1_3_0_0_1_1_1_0_1` /
//! `_3_3_0_0_1_0_1_0_0`).

use std::sync::Arc;

use crate::api::TagStruct;
use crate::file::TagFile;
use crate::math::{RealPoint2d, RealVector3d};
use crate::render_method::{RenderMethod, RenderMethodError};
use crate::typed_enums::{Enum, Flags};

pub const PARTICLE_GROUP: [u8; 4] = *b"prt3";

#[derive(Debug)]
pub enum ParticleError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
    RenderMethod(RenderMethodError),
}

impl std::fmt::Display for ParticleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongGroup { expected, actual } => write!(
                f,
                "expected group '{}', got '{}'",
                std::str::from_utf8(expected).unwrap_or("?"),
                std::str::from_utf8(actual).unwrap_or("?"),
            ),
            Self::RenderMethod(e) => write!(f, "shader: {e}"),
        }
    }
}

impl std::error::Error for ParticleError {}

impl From<RenderMethodError> for ParticleError {
    fn from(e: RenderMethodError) -> Self {
        Self::RenderMethod(e)
    }
}

// ---------------------------------------------------------------------------
// `particle_main_flags` (long_flags) — per `particle_main_flags` enum
// in particle.json. P1 captures the bits we know cases for; engine
// adds more at higher bits.
// ---------------------------------------------------------------------------

pub const PARTICLE_MAIN_FLAG_DIES_AT_REST: u32 = 1 << 0;
pub const PARTICLE_MAIN_FLAG_DIES_ON_STRUCTURE_COLLISION: u32 = 1 << 1;
pub const PARTICLE_MAIN_FLAG_DIES_IN_MEDIA: u32 = 1 << 2;
pub const PARTICLE_MAIN_FLAG_DIES_IN_AIR: u32 = 1 << 3;
pub const PARTICLE_MAIN_FLAG_HAS_SWEETENER: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// `particle_appearance_flags` (long_flags) — visual control bits per
// `particle_appearance_flags` enum.
// ---------------------------------------------------------------------------

pub const PARTICLE_APPEARANCE_RANDOM_U_MIRROR: u32 = 1 << 0;
pub const PARTICLE_APPEARANCE_RANDOM_V_MIRROR: u32 = 1 << 1;
pub const PARTICLE_APPEARANCE_RANDOM_ROTATION: u32 = 1 << 2;
pub const PARTICLE_APPEARANCE_TINT_FROM_LIGHTMAP: u32 = 1 << 3;
pub const PARTICLE_APPEARANCE_TINT_FROM_DIFFUSE: u32 = 1 << 4;
pub const PARTICLE_APPEARANCE_MOTION_BLUR: u32 = 1 << 5;
pub const PARTICLE_APPEARANCE_DOUBLE_SIDED: u32 = 1 << 6;
pub const PARTICLE_APPEARANCE_EDGE_FADE: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// `particle_animation_flags`.
// ---------------------------------------------------------------------------

pub const PARTICLE_ANIM_FRAME_ANIMATION_ONE_SHOT: u32 = 1 << 0;
pub const PARTICLE_ANIM_CAN_ANIMATE_BACKWARDS: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// `particle_billboard_type_enum` — controls how the VS expands a
// particle into a quad. Per particle.json:
// ---------------------------------------------------------------------------

/// `particle_billboard_type_enum` (short_enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i16)]
pub enum ParticleBillboardStyle {
    #[default]
    #[strum(serialize = "screen facing")] ScreenFacing = 0,
    #[strum(serialize = "camera facing")] CameraFacing = 1,
    #[strum(serialize = "parallel to direction")] ParallelToDirection = 2,
    #[strum(serialize = "perpendicular to direction")] Perpendicular = 3,
    #[strum(serialize = "vertical")] Vertical = 4,
    #[strum(serialize = "horizontal")] Horizontal = 5,
    #[strum(serialize = "local vertical")] LocalVertical = 6,
    #[strum(serialize = "local horizontal")] LocalHorizontal = 7,
    #[strum(serialize = "world (particle models)")] WorldModel = 8,
    #[strum(serialize = "velocity horizontal (particle models)")] VelocityHorizontal = 9,
}

/// `attachment_type_enum` — when the attachment fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i8)]
pub enum ParticleAttachmentTrigger {
    #[default]
    #[strum(serialize = "birth")] Birth = 0,
    #[strum(serialize = "collision")] Collision = 1,
    #[strum(serialize = "death")] Death = 2,
}

/// `output_mod_enum` — `c_particle_property.m_output_modifier`. Owned
/// here; imported by `lens_flare`. Variant 0 is the blank `" "` option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i8)]
pub enum ParticlePropertyOutputModifier {
    #[default]
    #[strum(serialize = " ")] None = 0,
    #[strum(serialize = "Plus")] Plus = 1,
    #[strum(serialize = "Times")] Times = 2,
}

/// `game_state_type_enum` — the per-property input/range/modifier state
/// source the particle evaluator samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i8)]
pub enum GameStateType {
    #[default]
    #[strum(serialize = "particle age")] ParticleAge = 0,
    #[strum(serialize = "system age")] SystemAge = 1,
    #[strum(serialize = "particle random")] ParticleRandom = 2,
    #[strum(serialize = "system random")] SystemRandom = 3,
    #[strum(serialize = "particle correlation 1")] ParticleCorrelation1 = 4,
    #[strum(serialize = "particle correlation 2")] ParticleCorrelation2 = 5,
    #[strum(serialize = "particle correlation 3")] ParticleCorrelation3 = 6,
    #[strum(serialize = "particle correlation 4")] ParticleCorrelation4 = 7,
    #[strum(serialize = "system correlation 1")] SystemCorrelation1 = 8,
    #[strum(serialize = "system correlation 2")] SystemCorrelation2 = 9,
    #[strum(serialize = "particle emission time")] ParticleEmissionTime = 10,
    #[strum(serialize = "location lod")] LocationLod = 11,
    #[strum(serialize = "game time")] GameTime = 12,
    #[strum(serialize = "effect a scale")] EffectAScale = 13,
    #[strum(serialize = "effect b scale")] EffectBScale = 14,
    #[strum(serialize = "particle rotation")] ParticleRotation = 15,
    #[strum(serialize = "location random")] LocationRandom = 16,
    #[strum(serialize = "distance from emitter")] DistanceFromEmitter = 17,
    #[strum(serialize = "explosion animation")] ExplosionAnimation = 18,
    #[strum(serialize = "explosion rotation")] ExplosionRotation = 19,
    #[strum(serialize = "invalid state --- please set again")] InvalidState = 20,
    /// SUPERSET (2007-era effect tags, e.g. `s3d_avalanche`). The shipped
    /// 2007 `game_state_type_enum` carried an extra "inactive (zero)" option
    /// (a property explicitly pinned to zero) that MCC dropped. Decode is
    /// by-name, so the discriminant is irrelevant — this variant only needs
    /// to exist so the 2007 layout's name resolves instead of fail-loud
    /// panicking. Evaluates to a constant-zero input source.
    #[strum(serialize = "inactive (zero)")] InactiveZero = 21,
}

/// `particle_main_flags` (long_flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum ParticleMainFlags {
    #[strum(serialize = "dies at rest")] DiesAtRest = 0,
    #[strum(serialize = "dies on structure collision")] DiesOnStructureCollision = 1,
    #[strum(serialize = "dies in media")] DiesInMedia = 2,
    #[strum(serialize = "dies in air")] DiesInAir = 3,
    #[strum(serialize = "has sweetener")] HasSweetener = 4,
}

/// `particle_appearance_flags` (long_flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum ParticleAppearanceFlags {
    #[strum(serialize = "random u mirror")] RandomUMirror = 0,
    #[strum(serialize = "random v mirror")] RandomVMirror = 1,
    #[strum(serialize = "random starting rotation")] RandomStartingRotation = 2,
    #[strum(serialize = "tint from lightmap")] TintFromLightmap = 3,
    #[strum(serialize = "tint from diffuse texture")] TintFromDiffuseTexture = 4,
    #[strum(serialize = "bitmap authored vertically")] BitmapAuthoredVertically = 5,
    #[strum(serialize = "intensity affects alpha")] IntensityAffectsAlpha = 6,
    #[strum(serialize = "fade when viewed edge-on")] FadeWhenViewedEdgeOn = 7,
    #[strum(serialize = "motion blur")] MotionBlur = 8,
    #[strum(serialize = "double-sided")] DoubleSided = 9,
    /// SUPERSET (2007-era tags, e.g. cyberdyne's `spark_ember`,
    /// `fireball_*`). The shipped 2007 particle schema INSERTED "fade near
    /// camera" at bit 7 (the near-fade appearance bit), shifting
    /// "fade when viewed edge-on" to bit 8; MCC dropped it and renumbered.
    /// Decode is by-name, so the bit position is irrelevant — this variant
    /// only needs to exist so the 2007 layout's bit-7 name resolves instead
    /// of fail-loud panicking. Verified: all 50 corpus tags carrying it
    /// share the single layout `…6:intensity affects alpha · 7:fade near
    /// camera · 8:fade when viewed edge-on`.
    #[strum(serialize = "fade near camera")] FadeNearCamera = 10,
    /// SUPERSET (2007-era tags, e.g. `s3d_turf`, `s3d_reactor`). The 2007
    /// particle appearance schema carried a "tint colors are self-illum"
    /// bit (route the particle's tint into self-illumination rather than
    /// diffuse) that MCC dropped. Decode is by-name, so the bit position is
    /// irrelevant — this variant only needs to exist so the 2007 layout's
    /// name resolves instead of fail-loud panicking.
    #[strum(serialize = "tint colors are self-illum")] TintColorsAreSelfIllum = 11,
}

/// `particle_animation_flags` (long_flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum ParticleAnimationFlags {
    #[strum(serialize = "frame animation one shot")] FrameAnimationOneShot = 0,
    #[strum(serialize = "can animate backwards")] CanAnimateBackwards = 1,
    /// SUPERSET (2007-era tags, e.g. `s3d_avalanche`). The 2007 particle
    /// animation schema carried a "select random sequence" bit (pick a
    /// random animation sequence per particle) that MCC dropped. Decode is
    /// by-name, so the bit position is irrelevant — this variant only needs
    /// to exist so the 2007 layout's name resolves instead of fail-loud
    /// panicking.
    #[strum(serialize = "select random sequence")] SelectRandomSequence = 2,
}

// ---------------------------------------------------------------------------
// `s_particle_attachment` (20B, max 4 per particle).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ParticleAttachment {
    /// Tag path of the attachment target (effe / snd! / foot etc.).
    pub type_ref: String,
    /// Group fourcc of the target tag (effe / snd! / foot / etc.).
    pub type_group: [u8; 4],
    pub trigger: Enum<ParticleAttachmentTrigger, i8>,
    /// `attachment_flags` — empty option list in the schema, kept raw.
    pub flags: u8,
    /// `game_state_type_enum` — drives scale at attachment fire time.
    pub primary_scale: Enum<GameStateType, i8>,
    pub secondary_scale: Enum<GameStateType, i8>,
}

impl ParticleAttachment {
    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        let (type_group_u32, type_ref) =
            s.read_tag_ref_with_group("type").unwrap_or((0, String::new()));
        Self {
            type_ref,
            type_group: type_group_u32.to_be_bytes(),
            trigger: s.try_read_enum("trigger").unwrap_or_default(),
            flags: s.read_int_any("flags").unwrap_or(0) as u8,
            primary_scale: s.try_read_enum("primary scale").unwrap_or_default(),
            secondary_scale: s.try_read_enum("secondary scale").unwrap_or_default(),
        }
    }
}

// ---------------------------------------------------------------------------
// `c_particle_property` (32B) — scalar variant. Captures state inputs +
// output modifier + constant value + runtime flags + the authored
// `Mapping` curve (identical to `c_editable_property_base`); the GPU
// property evaluator compiles the curve per-emitter and runs it against
// game_state_type_enum values each frame.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ParticlePropertyScalar {
    /// `game_state_type_enum` — what feeds the function input.
    pub input_variable: Enum<GameStateType, i8>,
    /// Second `game_state_type_enum` — feeds the range axis for ranged
    /// interpolation curves.
    pub range_variable: Enum<GameStateType, i8>,
    pub output_modifier: Enum<ParticlePropertyOutputModifier, i8>,
    /// `game_state_type_enum` — feeds the modifier's input.
    pub output_modifier_input: Enum<GameStateType, i8>,
    /// Authored curve / function blob (the `Mapping` field — IDENTICAL
    /// two-stage layout to `c_editable_property_base`). `None` when the
    /// property is constant (engine reads `constant_value`). Present in
    /// the schema; the GPU property evaluator compiles it per-emitter.
    pub function: Option<crate::tag_function::TagFunction>,
    /// Fallback constant when the curve is the identity / not authored.
    /// Engine reads this at evaluate time when `m_flags & is_constant`.
    pub constant_value: f32,
    /// Runtime flag byte. Bits aren't fully decoded by P1; the
    /// particle subsystem owns the bit interpretation.
    pub runtime_flags: u8,
}

impl ParticlePropertyScalar {
    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            input_variable: s.try_read_enum("Input Variable").unwrap_or_default(),
            range_variable: s.try_read_enum("Range Variable").unwrap_or_default(),
            output_modifier: s.try_read_enum("Output Modifier").unwrap_or_default(),
            output_modifier_input: s.try_read_enum("Output Modifier Input").unwrap_or_default(),
            function: crate::effects_properties::read_mapping_function(s, "Mapping"),
            constant_value: s.read_real("runtime m_constant_value").unwrap_or(0.0),
            runtime_flags: s.read_int_any("runtime m_flags").unwrap_or(0) as u8,
        }
    }
}

/// `c_particle_property` (32B) — color variant. Same layout as scalar
/// at the engine level (the underlying struct IS the same) but the
/// constant value spans 3 floats (RGB) in the function blob. P1 keeps
/// it conservative: same field set as scalar, leave the RGB constant
/// for the particle subsystem to extract from the curve blob.
pub type ParticlePropertyColor = ParticlePropertyScalar;

// ---------------------------------------------------------------------------
// GPU sprite + frame UV corners — runtime engine bakes these per-tag.
// ---------------------------------------------------------------------------

/// One sprite — a `real_vector4d` UV rect (x, y, width, height OR
/// min/max corners — engine picks per particle's billboard style).
#[derive(Debug, Clone, Copy, Default)]
pub struct ParticleGpuSprite {
    pub corner: [f32; 4],
}

/// Up to 16 frame UVs (for sprite-sheet animation). Engine slot 15 is
/// padding (`m_frames[15]` per Ares).
#[derive(Debug, Clone, Default)]
pub struct ParticleGpuFrames {
    /// Authored frame count (engine stores as float in slot 0).
    pub count: f32,
    /// Per-frame UV corner (up to 15 valid + 1 pad).
    pub frames: Vec<[f32; 4]>,
}

/// GPU sprite/frames metadata. Runtime-baked at tag postprocess time.
#[derive(Debug, Clone, Default)]
pub struct ParticleGpuData {
    pub sprite: Option<ParticleGpuSprite>,
    pub frames: Option<ParticleGpuFrames>,
}

impl ParticleGpuData {
    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        let sprite = s
            .field("runtime m_sprite")
            .and_then(|f| f.as_block())
            .and_then(|b| b.element(0))
            .map(|sprite_block| {
                // sprite_block contains a `gpu_single_constant_register_array`
                // which is an inline array of 4 reals — the corner vec4.
                let mut corner = [0.0f32; 4];
                if let Some(arr_struct) = sprite_block
                    .fields_all()
                    .find_map(|f| f.as_struct())
                {
                    // Each array element is a single real. Walk in order.
                    for (i, field) in arr_struct.fields_all().enumerate().take(4) {
                        if let Some(crate::fields::TagFieldData::Real(v)) =
                            field.value()
                        {
                            corner[i] = v;
                        }
                    }
                }
                ParticleGpuSprite { corner }
            });

        let frames = s
            .field("runtime m_frames")
            .and_then(|f| f.as_block())
            .map(|b| {
                let mut out = ParticleGpuFrames::default();
                if let Some(head) = b.element(0)
                    && let Some(count_field) = head.field("runtime gpu_variants_count")
                    && let Some(crate::fields::TagFieldData::Real(c)) =
                        count_field.value()
                {
                    out.count = c;
                }
                for i in 0..b.len() {
                    let Some(elem) = b.element(i) else { continue };
                    // Each element holds an array of 4 reals (one frame's UV).
                    let mut corner = [0.0f32; 4];
                    if let Some(arr_struct) = elem
                        .fields_all()
                        .find_map(|f| f.as_struct())
                    {
                        for (j, field) in arr_struct.fields_all().enumerate().take(4) {
                            if let Some(crate::fields::TagFieldData::Real(v)) =
                                field.value()
                            {
                                corner[j] = v;
                            }
                        }
                    }
                    out.frames.push(corner);
                }
                out
            });

        Self { sprite, frames }
    }
}

// ---------------------------------------------------------------------------
// `c_particle_definition` (404B root).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ParticleDefinition {
    pub main_flags: Flags<ParticleMainFlags, u32>,
    pub attachments: Vec<ParticleAttachment>,
    pub appearance_flags: Flags<ParticleAppearanceFlags, u32>,
    pub billboard_style: Enum<ParticleBillboardStyle, i16>,
    pub first_sequence_index: i16,
    pub sequence_count: i16,
    pub center_offset: RealPoint2d,
    pub curvature: f32,
    pub angle_fade_range_degrees: f32,
    pub angle_fade_cutoff_degrees: f32,
    pub motion_blur_translation_scale: f32,
    pub motion_blur_rotation_scale: f32,
    pub motion_blur_aspect_scale: f32,
    /// Render method (shader_particle_struct_definition). Carries the
    /// `rmdf` reference, options, parameters, postprocess. Reuses the
    /// existing [`RenderMethod`] walker — protomorph already knows how
    /// to bind these at draw time.
    pub shader: Option<Arc<RenderMethod>>,

    // Properties — each 32B `c_particle_property`.
    pub aspect_ratio: ParticlePropertyScalar,
    pub color: ParticlePropertyColor,
    pub intensity: ParticlePropertyScalar,
    pub alpha: ParticlePropertyScalar,

    pub animation_flags: Flags<ParticleAnimationFlags, u32>,
    pub frame_index: ParticlePropertyScalar,
    pub animation_rate: ParticlePropertyScalar,
    pub palette_animation: ParticlePropertyScalar,

    /// pmdf model reference for mesh-based particles. Empty for the
    /// common case (billboards).
    pub model: String,

    /// Runtime bitmask: which `game_state_type_enum` inputs any of
    /// the property curves reference.
    pub runtime_used_particle_states: u32,
    pub runtime_constant_per_particle_properties: u32,
    pub runtime_constant_over_time_properties: u32,

    pub gpu_data: ParticleGpuData,

    /// `_arbitrary_vector3d` of the particle's sample axis (used by
    /// VS for parallel/perpendicular billboard styles). Not authored
    /// at the prt3 level — derived per-emitter at spawn time. Kept
    /// here for diagnostic purposes; default = +X.
    pub diagnostic_axis: RealVector3d,
}

impl ParticleDefinition {
    pub fn from_tag(tag: &TagFile) -> Result<Self, ParticleError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != PARTICLE_GROUP {
            return Err(ParticleError::WrongGroup {
                expected: PARTICLE_GROUP,
                actual,
            });
        }
        Self::from_struct(&tag.root())
    }

    pub fn from_struct(s: &TagStruct<'_>) -> Result<Self, ParticleError> {
        let main_flags = s.try_read_flags("main flags").unwrap_or_default();
        let attachments = s
            .field("attachments")
            .and_then(|f| f.as_block())
            .map(|b| {
                let mut out = Vec::with_capacity(b.len());
                for i in 0..b.len() {
                    if let Some(entry) = b.element(i) {
                        out.push(ParticleAttachment::from_struct(&entry));
                    }
                }
                out
            })
            .unwrap_or_default();
        let appearance_flags = s.try_read_flags("appearance flags").unwrap_or_default();
        let billboard_style = s.try_read_enum("particle billboard style").unwrap_or_default();
        let first_sequence_index =
            s.read_int_any("first sequence index").unwrap_or(0) as i16;
        let sequence_count =
            s.read_int_any("sequence count").unwrap_or(0) as i16;
        let center_offset = s.read_point2d("center offset");
        let curvature = s.read_real("curvature").unwrap_or(0.0);
        let angle_fade_range_degrees = s.read_real("angle fade range").unwrap_or(0.0);
        let angle_fade_cutoff_degrees =
            s.read_real("angle fade cutoff").unwrap_or(0.0);
        let motion_blur_translation_scale =
            s.read_real("motion blur translation scale").unwrap_or(0.0);
        let motion_blur_rotation_scale =
            s.read_real("motion blur rotation scale").unwrap_or(0.0);
        let motion_blur_aspect_scale =
            s.read_real("motion blur aspect scale").unwrap_or(0.0);

        // Shader is a struct field — descend + walk via existing
        // RenderMethod::from_struct. Schema field name is "actual shader?"
        // (yes, with the `?`); the embedded layout name might differ —
        // we walk via the struct field iterator to find the first
        // child struct that's the render_method (typical position).
        // Particles embed a `c_render_method` sub-struct without an outer
        // tag-group declaration (the parent prt3 tag carries the context).
        // `RenderMethod::from_struct` defaults `group_tag = 0` for that
        // reason — patch it to `b"rmp "` here so the renderer's
        // subclass dispatch (`render_methods::assemble`) routes through
        // the particle WGSL path. Engine equivalent: the prt3's runtime
        // c_render_method has its subclass field stamped to `rmp ` at
        // tag-build time; we restore that semantic post-walk.
        let shader = s
            .field("actual shader?")
            .and_then(|f| f.as_struct())
            .and_then(|sub| RenderMethod::from_struct(&sub).ok())
            .map(|mut rm| {
                // Embedded particle shader — set the typed subclass and a
                // readable stand-in group_tag. The real schema slot is the
                // embedded-only nominal `?rmp` (never on disk); `rmp ` is a
                // collision-free dispatch key for the renderer.
                rm.class = crate::render_method::RenderMethodClass::Particle;
                rm.group_tag = u32::from_be_bytes(*b"rmp ");
                rm
            })
            .map(Arc::new);

        let aspect_ratio = read_property_scalar(s, "aspect ratio");
        let color = read_property_scalar(s, "color");
        let intensity = read_property_scalar(s, "intensity");
        let alpha = read_property_scalar(s, "alpha");

        let animation_flags = s.try_read_flags("animation flags").unwrap_or_default();
        let frame_index = read_property_scalar(s, "frame index");
        let animation_rate = read_property_scalar(s, "animation rate");
        let palette_animation = read_property_scalar(s, "palette animation");

        let model = s.read_tag_ref_path("Model").unwrap_or_default();

        let runtime_used_particle_states =
            s.read_int_any("runtime m_used_particle_states").unwrap_or(0) as u32;
        let runtime_constant_per_particle_properties = s
            .read_int_any("runtime m_constant_per_particle_properties")
            .unwrap_or(0) as u32;
        let runtime_constant_over_time_properties = s
            .read_int_any("runtime m_constant_over_time_properties")
            .unwrap_or(0) as u32;

        let gpu_data = s
            .field("runtime m_gpu_data")
            .and_then(|f| f.as_struct())
            .map(|sub| ParticleGpuData::from_struct(&sub))
            .unwrap_or_default();

        Ok(Self {
            main_flags,
            attachments,
            appearance_flags,
            billboard_style,
            first_sequence_index,
            sequence_count,
            center_offset,
            curvature,
            angle_fade_range_degrees,
            angle_fade_cutoff_degrees,
            motion_blur_translation_scale,
            motion_blur_rotation_scale,
            motion_blur_aspect_scale,
            shader,
            aspect_ratio,
            color,
            intensity,
            alpha,
            animation_flags,
            frame_index,
            animation_rate,
            palette_animation,
            model,
            runtime_used_particle_states,
            runtime_constant_per_particle_properties,
            runtime_constant_over_time_properties,
            gpu_data,
            diagnostic_axis: RealVector3d { i: 1.0, j: 0.0, k: 0.0 },
        })
    }
}

/// Walk a 32B `c_particle_property` substruct by field name. Returns
/// [`ParticlePropertyScalar::default()`] if the field is absent.
fn read_property_scalar(parent: &TagStruct<'_>, name: &str) -> ParticlePropertyScalar {
    parent
        .field(name)
        .and_then(|f| f.as_struct())
        .map(|sub| ParticlePropertyScalar::from_struct(&sub))
        .unwrap_or_default()
}
