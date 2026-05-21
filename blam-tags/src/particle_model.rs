//! `particle_model` (`pmdf`) tag walker — mesh-particle geometry
//! source. A `particle` (prt3) tag with the
//! `_particle_is_particle_model_bit` set on `main_flags` references
//! one of these instead of using sprite-billboard rendering.
//!
//! ## Engine consumption
//!
//! - `c_particle_definition::get_particle_model_definition` resolves
//!   the model reference at particle-emitter init.
//! - At submit time `c_particle_emitter_definition::initialize_particle
//!   @ 0x1805673F0` reads the m_variants block (via gpu_data) and
//!   picks a random variant index per particle.
//! - GPU side: the spawn/update shaders read
//!   `mesh_vertices` from structured buffer slot 17 (see HLSL
//!   inventory) and use the variant's index range to slice the source
//!   mesh into one mesh-particle instance.
//!
//! ## Schema shape
//!
//! `c_particle_model_definition` (144B): `s_render_geometry` (132B) +
//! `s_gpu_data` variants meta (12B). The render_geometry uses the
//! same `global_render_geometry_struct` as render_model — consumers
//! who need actual mesh data should call
//! [`crate::render_model::extract_render_geometry_meshes`] on
//! [`ParticleModel::render_geometry_root`].
//!
//! ## m_gpu_data is runtime-resolved
//!
//! The m_variants block carries one element per variant, each holding
//! a `runtime m_count!` array filled by tool.exe at cache-compile
//! time. Source tags read here have `variant_count` (block length) but
//! the inner `m_count` arrays are empty.

use crate::api::TagStruct;
use crate::file::TagFile;

const PMDF_GROUP: [u8; 4] = *b"pmdf";

#[derive(Debug)]
pub enum ParticleModelError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
}

impl std::fmt::Display for ParticleModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongGroup { expected, actual } => write!(
                f,
                "particle_model: wrong tag group (expected {:?}, got {:?})",
                std::str::from_utf8(expected).unwrap_or("????"),
                std::str::from_utf8(actual).unwrap_or("????"),
            ),
        }
    }
}

impl std::error::Error for ParticleModelError {}

/// Walked `particle_model` tag.
#[derive(Debug, Clone, Default)]
pub struct ParticleModel {
    /// Authored variant count — number of distinct mesh-slice
    /// permutations the particle emitter can pick from. Zero on a
    /// freshly-imported tag (no `Import model` step performed).
    pub variant_count: usize,
    /// `runtime flags*!` from the inner render_geometry — bitfield
    /// of geometry-level engine flags (compression hints etc.).
    pub render_geometry_runtime_flags: u32,
    /// Mesh count from `render geometry.meshes[]` — exposed for
    /// quick sanity checks without re-walking the geometry struct.
    pub mesh_count: usize,
}

impl ParticleModel {
    pub fn from_tag(tag: &TagFile) -> Result<Self, ParticleModelError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != PMDF_GROUP {
            return Err(ParticleModelError::WrongGroup { expected: PMDF_GROUP, actual });
        }
        Ok(Self::from_struct(&tag.root()))
    }

    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        let (rg_flags, mesh_count) = match s.field("render geometry").and_then(|f| f.as_struct()) {
            Some(rg) => {
                let flags = rg.read_int_any("runtime flags").unwrap_or(0) as u32;
                let mc = rg
                    .field("meshes")
                    .and_then(|f| f.as_block())
                    .map(|b| b.len())
                    .unwrap_or(0);
                (flags, mc)
            }
            None => (0, 0),
        };
        let variant_count = s
            .field("m_gpu_data")
            .and_then(|f| f.as_struct())
            .and_then(|gpu| gpu.field("m_variants").and_then(|f| f.as_block()))
            .map(|b| b.len())
            .unwrap_or(0);
        Self {
            variant_count,
            render_geometry_runtime_flags: rg_flags,
            mesh_count,
        }
    }
}
