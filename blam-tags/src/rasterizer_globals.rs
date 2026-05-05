//! `rasterizer_globals` (`rasg`) — engine resource registry.
//! Pointed at by the global `globals` tag's rasterizer reference.
//!
//! Holds the per-engine fallback bitmaps, Cook-Torrance integral LUTs,
//! built-in vertex/pixel shader pairs (postprocess + debug), and the
//! Active Camo distortion texture. Halo's runtime references these via
//! `e_render_method_extern` enum values + a small index-into-block.
//!
//! For protomorph: load this once at startup so engine externs
//! (`TextureCookTorranceCc0236/Dd0236/C78d78`,
//! `TextureDynamicEnvironmentMap0/1` fallback, etc.) resolve to the
//! authored bitmap path — no name-based or index-based hacks.

use crate::api::{TagBlock, TagStruct};
use crate::file::TagFile;

const RASG_GROUP: [u8; 4] = *b"rasg";

#[derive(Debug)]
pub enum RasterizerGlobalsError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
}

impl std::fmt::Display for RasterizerGlobalsError {
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
impl std::error::Error for RasterizerGlobalsError {}

/// `rasterizer_globals.default_bitmaps[i]` slot identity. Indices match
/// the on-disk block order. Naming mirrors what each slot stores in
/// the `shaders\default_bitmaps\bitmaps\` library; runtime extern
/// fallbacks pick a slot by this enum.
///
/// Verified against `inspect --full rasterizer_globals.rasterizer_globals`
/// 2026-05-04.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DefaultBitmap {
    ColorWhite              = 0,
    DefaultVector           = 1,
    DefaultDynamicCubeMap   = 2,
    Colorbars               = 3,
    ColorBlack              = 4,
    ColorBlackAlphaBlack    = 5,
    Gray50Percent           = 6,
    AutoExposureWeight      = 7,
    AutoExposureWeight2     = 8,
    DitherPattern2          = 9,
    Random4Warp             = 10,
    WaterRipples            = 11,
}

/// `rasterizer_globals.material_textures[i]` slot identity. The
/// 3 entries are the Cook-Torrance pre-integrated BRDF LUTs runtime
/// binds via `_render_method_extern_texture_cook_torrance_*`.
///
/// **Note ordering:** the rasterizer_globals tag stores them as
/// `[cc0236, c78d78, dd0236]` (verified at inspection time), but the
/// HLSL extern enum is `cc0236 (24), dd0236 (25), c78d78 (26)`. The
/// runtime mapping accounts for this — we expose accessors by name.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MaterialTexture {
    CookTorranceCc0236 = 0,
    CookTorranceC78d78 = 1,
    CookTorranceDd0236 = 2,
}

/// Decoded `rasterizer_globals` — subset relevant to material/shader
/// binding. Other fields (motion blur, shield impact, performance
/// throttles) skipped until those features land.
#[derive(Debug, Clone, Default)]
pub struct RasterizerGlobals {
    /// 12 default fallback bitmaps. Index by [`DefaultBitmap`].
    pub default_bitmaps: Vec<String>,
    /// 3 Cook-Torrance LUTs. Index by [`MaterialTexture`].
    pub material_textures: Vec<String>,
    /// `default vertex shader` tag-ref path. The fallback simple VS
    /// most postprocess passes use as a base.
    pub default_vertex_shader: String,
    /// `default pixel shader` tag-ref path.
    pub default_pixel_shader: String,
    /// `Active Camo Distortion Texture` — bound to the
    /// `ActiveCamoDistortionTexture` extern.
    pub active_camo_distortion_texture: String,
    /// `explicit shaders[N]` — pre-baked vertex/pixel shader pairs
    /// for built-in passes (debug, copy, blur, downsample, cubemap
    /// ops, bloom, etc.). Empty PS fields kept as None for unused
    /// slots so the slot index lines up with Halo's
    /// `e_explicit_shader` enum.
    pub explicit_shaders: Vec<ExplicitShader>,
}

#[derive(Debug, Clone, Default)]
pub struct ExplicitShader {
    pub vertex_shader: String,
    pub pixel_shader: String,
    pub compute_shader: String,
}

impl RasterizerGlobals {
    pub fn from_tag(tag: &TagFile) -> Result<Self, RasterizerGlobalsError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != RASG_GROUP {
            return Err(RasterizerGlobalsError::WrongGroup { expected: RASG_GROUP, actual });
        }
        Ok(Self::from_struct(&tag.root()))
    }

    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            default_bitmaps: read_block_paths(s, "default bitmaps", "default bitmaps"),
            material_textures: read_block_paths(s, "material textures", "material textures"),
            default_vertex_shader: s
                .read_tag_ref_path("default vertex shader")
                .unwrap_or_default(),
            default_pixel_shader: s
                .read_tag_ref_path("default pixel shader")
                .unwrap_or_default(),
            active_camo_distortion_texture: s
                .read_tag_ref_path("Active Camo Distortion Texture")
                .unwrap_or_default(),
            explicit_shaders: read_explicit_shaders(s),
        }
    }

    /// Resolve a [`DefaultBitmap`] slot to its tag-relative bitmap path.
    pub fn default_bitmap(&self, slot: DefaultBitmap) -> Option<&str> {
        self.default_bitmaps
            .get(slot as usize)
            .map(|s| s.as_str())
            .filter(|s| !s.is_empty())
    }

    /// Resolve a [`MaterialTexture`] slot (Cook-Torrance LUT) to its
    /// tag-relative bitmap path.
    pub fn material_texture(&self, slot: MaterialTexture) -> Option<&str> {
        self.material_textures
            .get(slot as usize)
            .map(|s| s.as_str())
            .filter(|s| !s.is_empty())
    }
}

fn read_block_paths(s: &TagStruct<'_>, block_name: &str, ref_field: &str) -> Vec<String> {
    s.field(block_name)
        .and_then(|f| f.as_block())
        .map(|b| read_paths(&b, ref_field))
        .unwrap_or_default()
}

fn read_paths(block: &TagBlock<'_>, ref_field: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(block.len());
    for i in 0..block.len() {
        if let Some(elem) = block.element(i) {
            out.push(elem.read_tag_ref_path(ref_field).unwrap_or_default());
        }
    }
    out
}

fn read_explicit_shaders(s: &TagStruct<'_>) -> Vec<ExplicitShader> {
    s.field("explicit shaders")
        .and_then(|f| f.as_block())
        .map(|b| {
            let mut out = Vec::with_capacity(b.len());
            for i in 0..b.len() {
                if let Some(elem) = b.element(i) {
                    out.push(ExplicitShader {
                        vertex_shader: elem
                            .read_tag_ref_path("explicit vertex shader")
                            .unwrap_or_default(),
                        pixel_shader: elem
                            .read_tag_ref_path("explicit pixel shader")
                            .unwrap_or_default(),
                        compute_shader: elem
                            .read_tag_ref_path("explicit compute shader")
                            .unwrap_or_default(),
                    });
                }
            }
            out
        })
        .unwrap_or_default()
}
