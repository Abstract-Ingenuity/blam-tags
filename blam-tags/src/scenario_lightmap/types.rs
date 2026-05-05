//! `scenario_lightmap_bsp_data` (.scenario_lightmap_bsp_data) — per-BSP
//! baked lightmap data: compression vectors, lightprobe atlas refs,
//! per-cluster + per-instance probe assignments, plus inline SH probes.
//!
//! ## Lighting paths supported (MCC)
//!
//! Each cluster / instance / scenery placement carries a policy via
//! `lightprobe_texture_array_index` + `pervertex_block_index` +
//! `probe_block_index` (mutually exclusive — only one is non-(-1) per
//! entry, except texture+anything which combines):
//!
//! | What | Resolves via |
//! |---|---|
//! | Per-pixel | `lightprobe texture reference` (atlas) sampled with mesh's lightmap UVs at the assigned `lightprobe_texture_array_index` slice. |
//! | Per-vertex | `bsp_per_vertex_data[pervertex_block_index]` — vertex-buffer-bound SH stream. |
//! | Single-probe | `probes[probe_block_index]` — order-3 SH (9 i16 coefs per channel) + dominant_light_direction + intensity. Used for instances + scenery placements. |
//!
//! Reference: `Ares/source/scenario/scenario_lightmap_definitions.h:90-104`.
//! Note that MCC stores **order-3** SH (9 coeffs / channel) for single
//! probes, while Ares older versions used order-2 (4 coefs / channel).

use crate::api::{TagBlock, TagStruct};
use crate::file::TagFile;
use crate::math::RealVector3d;

const SCNL_BSP_GROUP: [u8; 4] = *b"Lbsp";

#[derive(Debug)]
pub enum LightmapError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
}

impl std::fmt::Display for LightmapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongGroup { expected, actual } => write!(
                f,
                "scenario_lightmap_bsp_data: wrong tag group (expected {:?}, got {:?})",
                std::str::from_utf8(expected).unwrap_or("????"),
                std::str::from_utf8(actual).unwrap_or("????"),
            ),
        }
    }
}

impl std::error::Error for LightmapError {}

// =============================================================================
// Top-level
// =============================================================================

/// Per-BSP lightmap data — references the lightprobe atlas + per-cluster /
/// instance / scenery probes.
#[derive(Debug, Clone, Default)]
pub struct LightmapBspData {
    /// Bitmask: 0x1 compressed, 0x4 relightmapped, etc.
    pub flags: u16,
    /// Index of the BSP this lightmap applies to (matches scenario's
    /// structure_bsps[i]).
    pub bsp_reference_index: i16,
    /// Checksum from when the lightmap was baked — should match the
    /// BSP's `import info checksum` for valid pairing.
    pub structure_bsp_import_checksum: i32,

    /// 18 compression vectors used to dequantize SH coefficients in
    /// `probes[]` and per-vertex blocks. The runtime decoder multiplies
    /// each i16 coefficient by the matching compression vector.
    pub compression_vectors: [RealVector3d; 18],

    /// `.bitmap` of the lightprobe RGB SH atlas. Each texel holds 18
    /// half-float values (9 SH × 3 channels OR 4 SH × 3 channels at
    /// lower order; format documented in MCC tooling).
    pub lightprobe_texture: String,
    /// `.bitmap` of the dominant-light direction + intensity atlas.
    pub dominant_light_intensity_texture: String,

    /// Per-cluster lightmap policy. `clusters[i]` corresponds to
    /// `structure_bsp.clusters[i]`.
    pub clusters: Vec<LightmapClusterEntry>,

    /// Per-instance lightmap policy. `instances[i]` corresponds to
    /// `structure_bsp.instanced_geometry_instances[i]`.
    pub instances: Vec<LightmapInstanceEntry>,

    /// Single-probe SH coefficients — referenced by
    /// `instances[].probe_block_index`. Index = block index into here.
    pub probes: Vec<LightmapProbe>,

    /// Per-vertex SH blocks — referenced by
    /// `clusters[].pervertex_block_index` and
    /// `instances[].pervertex_block_index`. Each block is a flat list of
    /// per-vertex SH samples for the matching mesh's vertex buffer.
    pub bsp_per_vertex_data: Vec<LightmapPerVertexBlock>,

    /// Per-scenery-placement single probes (one per `scenario.scenery[i]`).
    /// Most scenery in maps relies on these for ambient lighting.
    pub scenery_probes: Vec<LightmapProbe>,

    /// Per-airprobe single probes (manually-placed lighting samples).
    pub airprobes: Vec<LightmapProbe>,

    /// Per-machine-placement single probes (one per device_machine).
    pub device_machine_probes: Vec<LightmapProbe>,
}

impl LightmapBspData {
    pub fn from_tag(tag: &TagFile) -> Result<Self, LightmapError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != SCNL_BSP_GROUP {
            return Err(LightmapError::WrongGroup {
                expected: SCNL_BSP_GROUP,
                actual,
            });
        }
        Ok(Self::from_struct(&tag.root()))
    }

    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        let mut compression_vectors = [RealVector3d::default(); 18];
        if let Some(arr) = s.field("compression vectors").and_then(|f| f.as_array()) {
            for i in 0..arr.len().min(18) {
                if let Some(elem) = arr.element(i) {
                    compression_vectors[i] = elem.read_vec3("vector");
                }
            }
        } else if let Some(b) = s.field("compression vectors").and_then(|f| f.as_block()) {
            for i in 0..b.len().min(18) {
                if let Some(e) = b.element(i) {
                    compression_vectors[i] = e.read_vec3("vector");
                }
            }
        }

        Self {
            flags: s.read_int_any("flags").unwrap_or(0) as u16,
            bsp_reference_index: s.read_int_any("bsp reference index").unwrap_or(-1) as i16,
            structure_bsp_import_checksum: s
                .read_int_any("structure BSP import checksum")
                .unwrap_or(0) as i32,
            compression_vectors,

            lightprobe_texture: s.read_tag_ref_path("lightprobe texture reference").unwrap_or_default(),
            dominant_light_intensity_texture: s
                .read_tag_ref_path("dominant light intensity texture reference")
                .unwrap_or_default(),

            clusters: read_block(s, "clusters", LightmapClusterEntry::from_struct),
            instances: read_block(s, "instances", LightmapInstanceEntry::from_struct),
            probes: read_block(s, "probes", LightmapProbe::from_struct),
            bsp_per_vertex_data: read_block(
                s,
                "bsp per-vertex data",
                LightmapPerVertexBlock::from_struct,
            ),
            scenery_probes: read_block(s, "scenery probes", LightmapProbe::from_struct),
            airprobes: read_block(s, "airprobes", LightmapProbe::from_struct),
            device_machine_probes: read_block(
                s,
                "device machine probes",
                LightmapProbe::from_struct,
            ),
        }
    }
}

// =============================================================================
// Sub-blocks
// =============================================================================

/// One cluster's lightmap policy. Texture-array mode and per-vertex
/// mode are mutually exclusive: only one of the two indices is non-(-1).
#[derive(Debug, Clone, Copy, Default)]
pub struct LightmapClusterEntry {
    /// Slice index into `lightprobe_texture` (-1 = no per-pixel lightmap).
    pub lightprobe_texture_array_index: i16,
    /// Block index into `bsp_per_vertex_data` (-1 = no per-vertex SH).
    pub pervertex_block_index: i16,
}

impl LightmapClusterEntry {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            lightprobe_texture_array_index: s
                .read_int_any("lightprobe texture array index")
                .unwrap_or(-1) as i16,
            pervertex_block_index: s.read_int_any("pervertex block index").unwrap_or(-1) as i16,
        }
    }

    /// Selected lighting policy for this entry (in order of precedence).
    pub fn policy(&self) -> LightmapPolicy {
        if self.lightprobe_texture_array_index >= 0 {
            LightmapPolicy::PerPixel
        } else if self.pervertex_block_index >= 0 {
            LightmapPolicy::PerVertex
        } else {
            LightmapPolicy::Fallback
        }
    }
}

/// One instance's lightmap policy. Like cluster, but instances also
/// support a single-probe path via `probe_block_index`.
#[derive(Debug, Clone, Copy, Default)]
pub struct LightmapInstanceEntry {
    pub lightprobe_texture_array_index: i16,
    pub pervertex_block_index: i16,
    /// Block index into `probes[]` (-1 = no single probe assignment).
    pub probe_block_index: i16,
}

impl LightmapInstanceEntry {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            lightprobe_texture_array_index: s
                .read_int_any("lightprobe texture array index")
                .unwrap_or(-1) as i16,
            pervertex_block_index: s.read_int_any("pervertex block index").unwrap_or(-1) as i16,
            probe_block_index: s.read_int_any("probe block index").unwrap_or(-1) as i16,
        }
    }

    pub fn policy(&self) -> LightmapPolicy {
        if self.lightprobe_texture_array_index >= 0 {
            LightmapPolicy::PerPixel
        } else if self.pervertex_block_index >= 0 {
            LightmapPolicy::PerVertex
        } else if self.probe_block_index >= 0 {
            LightmapPolicy::SingleProbe
        } else {
            LightmapPolicy::Fallback
        }
    }
}

/// Lighting evaluation path, selected per cluster / instance / object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightmapPolicy {
    /// Sample from lightprobe atlas with per-pixel UVs.
    PerPixel,
    /// Per-vertex SH stream interpolated to fragment.
    PerVertex,
    /// One pre-baked SH probe (instance / scenery / airprobe).
    SingleProbe,
    /// No lightmap data — engine uses sky-default lightprobe fallback.
    Fallback,
}

/// One single-probe SH sample: order-3 RGB SH (9 coefs / channel) +
/// dominant-light direction + intensity. Quantized as i16 — multiply
/// by the appropriate `compression_vectors[i]` to dequantize.
#[derive(Debug, Clone, Copy, Default)]
pub struct LightmapProbe {
    /// `dominant light direction i/j/k` — quantized direction.
    pub dominant_light_direction: [i16; 3],
    /// `dominant light intensity r/g/b` — quantized RGB intensity.
    pub dominant_light_intensity: [i16; 3],
    /// `red/green/blue lightprobe terms[0..9]` — quantized SH order-3
    /// coefficients per channel.
    pub red_terms: [i16; 9],
    pub green_terms: [i16; 9],
    pub blue_terms: [i16; 9],
}

impl LightmapProbe {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        let mut probe = Self::default();
        probe.dominant_light_direction = [
            s.read_int_any("dominant light direction i").unwrap_or(0) as i16,
            s.read_int_any("dominant light direction j").unwrap_or(0) as i16,
            s.read_int_any("dominant light direction k").unwrap_or(0) as i16,
        ];
        probe.dominant_light_intensity = [
            s.read_int_any("dominant light intensity r").unwrap_or(0) as i16,
            s.read_int_any("dominant light intensity g").unwrap_or(0) as i16,
            s.read_int_any("dominant light intensity b").unwrap_or(0) as i16,
        ];
        read_short_array(s, "red lightprobe terms", &mut probe.red_terms);
        read_short_array(s, "green lightprobe terms", &mut probe.green_terms);
        read_short_array(s, "blue lightprobe terms", &mut probe.blue_terms);
        probe
    }
}

/// One per-vertex SH block — a flat list of vertex-aligned probes.
/// Index in [`LightmapBspData::bsp_per_vertex_data`] is what
/// cluster/instance entries reference.
///
/// Each entry in `lightprobe_data[]` is the per-vertex equivalent of
/// [`LightmapProbe`] but typically with order-2 SH (4 coefs / channel)
/// for memory efficiency.
#[derive(Debug, Clone, Default)]
pub struct LightmapPerVertexBlock {
    pub lightprobe_data: Vec<LightmapPerVertexProbe>,
}

impl LightmapPerVertexBlock {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            lightprobe_data: read_block(
                s,
                "lightprobe data",
                LightmapPerVertexProbe::from_struct,
            ),
        }
    }
}

/// One per-vertex SH probe entry. Order-2 SH: 4 coefs per channel.
#[derive(Debug, Clone, Copy, Default)]
pub struct LightmapPerVertexProbe {
    pub dominant_light_intensity: [i16; 3],
    pub red_terms: [i16; 4],
    pub green_terms: [i16; 4],
    pub blue_terms: [i16; 4],
}

impl LightmapPerVertexProbe {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        let mut p = Self::default();
        p.dominant_light_intensity = [
            s.read_int_any("dominant light intensity r").unwrap_or(0) as i16,
            s.read_int_any("dominant light intensity g").unwrap_or(0) as i16,
            s.read_int_any("dominant light intensity b").unwrap_or(0) as i16,
        ];
        read_short_array(s, "red lightprobe terms", &mut p.red_terms);
        read_short_array(s, "green lightprobe terms", &mut p.green_terms);
        read_short_array(s, "blue lightprobe terms", &mut p.blue_terms);
        p
    }
}

// =============================================================================
// Helpers
// =============================================================================

fn read_block<T, F>(s: &TagStruct<'_>, name: &str, f: F) -> Vec<T>
where
    F: Fn(&TagStruct<'_>) -> T,
{
    s.field(name)
        .and_then(|fld| fld.as_block())
        .map(|b| read_block_vec(&b, f))
        .unwrap_or_default()
}

fn read_block_vec<T, F>(block: &TagBlock<'_>, f: F) -> Vec<T>
where
    F: Fn(&TagStruct<'_>) -> T,
{
    let mut out = Vec::with_capacity(block.len());
    for i in 0..block.len() {
        if let Some(elem) = block.element(i) {
            out.push(f(&elem));
        }
    }
    out
}

/// Read a fixed-size array-of-i16-coefficient field (the per-channel
/// SH "lightprobe terms" arrays). Each element is a struct with a
/// single `coefficient: short integer` field.
fn read_short_array<const N: usize>(s: &TagStruct<'_>, name: &str, out: &mut [i16; N]) {
    if let Some(arr) = s.field(name).and_then(|f| f.as_array()) {
        for i in 0..arr.len().min(N) {
            if let Some(elem) = arr.element(i) {
                out[i] = elem.read_int_any("coefficient").unwrap_or(0) as i16;
            }
        }
    } else if let Some(b) = s.field(name).and_then(|f| f.as_block()) {
        for i in 0..b.len().min(N) {
            if let Some(e) = b.element(i) {
                out[i] = e.read_int_any("coefficient").unwrap_or(0) as i16;
            }
        }
    }
}
