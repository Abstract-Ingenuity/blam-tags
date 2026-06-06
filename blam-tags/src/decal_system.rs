//! Decal system tag (`decs`) walker.
//!
//! Each scenario palette entry (`scenario.decal_palette[i]`) references
//! one of these tags. A `.decal_system` wraps 1+ `c_decal_definition`
//! entries; runtime decal placements (`scenario.decals[]`) cite a
//! palette index → tag → definition pair when projecting onto BSP
//! surfaces.
//!
//! Schema: `definitions/halo3_mcc/decal_system.json`
//! Engine: `c_decal_system_definition` (sizeof=36),
//! `c_decal_definition` (sizeof=116). Source:
//! `effects/decal_definitions.cpp`.
//!
//! Authoring source of truth — runtime per-decal mesh build
//! (`c_decal_system::build_mesh`) is handled separately at load time.
//! See umbrella `project_decals_port_plan_2026_05_10.md`.
//!
//! ## Pass enum convention
//!
//! Schema names the two values "pre-lighting" / "post-lighting"; the
//! engine internally aliases them as `_pass_post_albedo` (0) and
//! `_pass_post_static_lighting` (1) in `c_decal_system::render_all`.
//! We keep the schema names; the renderer can translate.

use crate::api::{TagBlock, TagStruct};
use crate::file::TagFile;
use crate::render_method::{RenderMethod, RenderMethodError};
use crate::typed_enums::{Enum, Flags};

const DECS_GROUP: [u8; 4] = *b"decs";

/// `decal_system_flags` (long_flags). NB: protomorph's `collide` path
/// (decal/collide.rs) interprets bits 3/4 as "skip instance/structure
/// secondary" — same wire bits, a different runtime reading of the
/// authoring names `force quad` / `force planar`.
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum DecalSystemFlags {
    #[strum(serialize = "random rotation")] RandomRotation = 0,
    #[strum(serialize = "random u mirror")] RandomUMirror = 1,
    #[strum(serialize = "random v mirror")] RandomVMirror = 2,
    #[strum(serialize = "force quad (use with care)")] ForceQuad = 3,
    #[strum(serialize = "force planar")] ForcePlanar = 4,
    #[strum(serialize = "restrict to single material")] RestrictToSingleMaterial = 5,
    #[strum(serialize = "use primary collision only")] UsePrimaryCollisionOnly = 6,
    #[strum(serialize = "don't collide with structure")] DontCollideWithStructure = 7,
    #[strum(serialize = "don't collide with instances")] DontCollideWithInstances = 8,
}

#[derive(Debug)]
pub enum DecalSystemError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
    RenderMethod(RenderMethodError),
}

impl std::fmt::Display for DecalSystemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongGroup { expected, actual } => write!(
                f,
                "decal_system: wrong tag group (expected {:?}, got {:?})",
                std::str::from_utf8(expected).unwrap_or("????"),
                std::str::from_utf8(actual).unwrap_or("????"),
            ),
            Self::RenderMethod(e) => write!(f, "decal_system: actual shader: {e}"),
        }
    }
}

impl std::error::Error for DecalSystemError {}

impl From<RenderMethodError> for DecalSystemError {
    fn from(e: RenderMethodError) -> Self {
        Self::RenderMethod(e)
    }
}

/// `_pass_post_albedo` (0) / `_pass_post_static_lighting` (1) — which
/// render pass `c_decal_system::render_all` should draw this definition
/// during. See umbrella plan for state-setup deltas between the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(i32)]
pub enum DecalPass {
    /// Schema "pre-lighting"; engine `_pass_post_albedo`. Writes to
    /// `_surface_post_HDR` (RT1) during the albedo pass — used for
    /// surface stains, posters, weathering that participate in the
    /// downstream lighting pass like the BSP underneath.
    #[default]
    #[strum(serialize = "pre-lighting")]
    PreLighting = 0,
    /// Schema "post-lighting"; engine `_pass_post_static_lighting`.
    /// Writes to RT0 (final lit color) — used for additive decals
    /// (laser scorches, blood) that don't want re-lighting.
    #[strum(serialize = "post-lighting")]
    PostLighting = 1,
}

/// Decal system tag (`decs`) — one palette entry.
#[derive(Debug, Clone, Default)]
pub struct DecalSystem {
    /// `decal_system_flags` — 9 authoring bits (random rotation,
    /// random u/v mirror, force quad, force planar, restrict single
    /// material, primary collision only, don't collide with
    /// structure/instances). Consumed by `c_decal_system::create` +
    /// `collide` + `build_mesh`.
    pub flags: Flags<DecalSystemFlags, u32>,
    /// `max overlapping` — 0 means no limit. Drives the per-cluster
    /// LRU eviction at `add_decal_to_cluster` time.
    pub max_overlapping: i32,
    /// `overlapping threshold` (world units) — distance at which two
    /// decals count as "overlapping" for the eviction check.
    pub overlapping_threshold: f32,
    /// `distance fade range` (start, end) in world units — feeds
    /// `c_decal_system::get_distance_fade`.
    pub distance_fade_range: (f32, f32),
    /// `runtime max radius!` — populated by the cache compiler from
    /// the max of all `decals[i].radius.max`.
    pub runtime_max_radius: f32,
    /// `decals` block — up to 16 per system.
    pub definitions: Vec<DecalDefinition>,
}

impl DecalSystem {
    pub fn from_tag(tag: &TagFile) -> Result<Self, DecalSystemError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != DECS_GROUP {
            return Err(DecalSystemError::WrongGroup { expected: DECS_GROUP, actual });
        }
        let root = tag.root();
        Self::from_struct(&root)
    }

    pub fn from_struct(s: &TagStruct<'_>) -> Result<Self, DecalSystemError> {
        let (fade_start, fade_end) = read_real_bounds(s, "distance fade range");
        let definitions = if let Some(block) = s.field("decals").and_then(|f| f.as_block()) {
            read_definitions(&block)?
        } else {
            Vec::new()
        };
        Ok(Self {
            flags: s.try_read_flags("flags").unwrap_or_default(),
            max_overlapping: s.read_int_any("max overlapping").unwrap_or(0) as i32,
            overlapping_threshold: s.read_real("overlapping threshold").unwrap_or(0.0),
            distance_fade_range: (fade_start, fade_end),
            runtime_max_radius: s
                .read_real("runtime max radius")
                .or_else(|| s.read_real("runtime max radius!"))
                .unwrap_or(0.0),
            definitions,
        })
    }
}

/// One entry of `decal_system.decals[]` — `c_decal_definition`.
#[derive(Debug, Clone, Default)]
pub struct DecalDefinition {
    /// `decal name^` (string_id).
    pub name: String,
    /// `decal_flags` (long_flags). Per the umbrella plan the active
    /// bits are: 0 = specular_modulate, 1 = bump_modulate, 2 =
    /// has_sprite, 3 = debug_border_color_white. (Schema ships an
    /// empty options list — bit interpretation is engine-side.)
    pub flags: u32,
    /// `actual shader?` (inline `c_render_method_shader_decal` struct).
    /// Same shape as rmsh: definition rmdf + options + parameters +
    /// postprocess + sort_layer + custom_fog. `None` if the slot is
    /// empty or unparseable.
    pub shader: Option<RenderMethod>,
    /// `radius` (world units, start/end) — projection sphere bounds.
    pub radius: (f32, f32),
    /// `decay time` (seconds, start/end).
    pub decay_time: (f32, f32),
    /// `lifespan` (seconds, start/end).
    pub lifespan: (f32, f32),
    /// `clamp angle` (degrees) — projections beyond this surface
    /// normal angle are clamped to it.
    pub clamp_angle_degrees: f32,
    /// `cull angle` (degrees) — projections beyond this are dropped.
    pub cull_angle_degrees: f32,
    /// `runtime pass!` — which c_player_view sub-pass to draw in.
    pub pass: Enum<DecalPass, i32>,
    /// `runtime specular_multiplier!`.
    pub specular_multiplier: f32,
    /// `runtime bitmap aspect!` — width/height ratio precomputed by
    /// the cache builder for sprite atlases.
    pub bitmap_aspect: f32,
}

fn read_definitions(block: &TagBlock<'_>) -> Result<Vec<DecalDefinition>, DecalSystemError> {
    let mut out = Vec::with_capacity(block.len());
    for i in 0..block.len() {
        if let Some(elem) = block.element(i) {
            out.push(DecalDefinition::from_struct(&elem)?);
        }
    }
    Ok(out)
}

impl DecalDefinition {
    pub fn from_struct(s: &TagStruct<'_>) -> Result<Self, DecalSystemError> {
        let shader_struct = s
            .descend("actual shader")
            .or_else(|| s.descend("actual shader?"));
        let shader = match shader_struct {
            Some(view) => {
                let mut rm = RenderMethod::from_struct(&view)?;
                // `actual shader` is by construction a
                // `c_render_method_shader_decal` — set the group_tag
                // explicitly since `from_struct` can't infer it (no
                // outer tag context). Without this, downstream
                // dispatchers keyed off `group_tag.to_be_bytes()` see
                // 0x00000000 and miss the rmd arm.
                rm.group_tag = u32::from_be_bytes(*b"rmd ");
                Some(rm)
            }
            None => None,
        };
        let radius = read_real_bounds(s, "radius");
        let decay_time = read_real_bounds(s, "decay time");
        let lifespan = read_real_bounds(s, "lifespan");
        let pass = s
            .try_read_enum("runtime pass")
            .or_else(|| s.try_read_enum("runtime pass!"))
            .unwrap_or_default();
        Ok(Self {
            name: s
                .read_string_id("decal name")
                .or_else(|| s.read_string_id("decal name^"))
                .unwrap_or_default(),
            flags: s.read_int_any("flags").unwrap_or(0) as u32,
            shader,
            radius,
            decay_time,
            lifespan,
            clamp_angle_degrees: s.read_real("clamp angle").unwrap_or(90.0),
            cull_angle_degrees: s.read_real("cull angle").unwrap_or(90.0),
            pass,
            specular_multiplier: s
                .read_real("runtime specular_multiplier")
                .or_else(|| s.read_real("runtime specular_multiplier!"))
                .unwrap_or(1.0),
            bitmap_aspect: s
                .read_real("runtime bitmap aspect")
                .or_else(|| s.read_real("runtime bitmap aspect!"))
                .unwrap_or(1.0),
        })
    }
}

fn read_real_bounds(s: &TagStruct<'_>, name: &str) -> (f32, f32) {
    use crate::fields::TagFieldData;
    match s.field(name).and_then(|f| f.value()) {
        Some(TagFieldData::RealBounds(b)) => (b.lower, b.upper),
        _ => (0.0, 0.0),
    }
}

