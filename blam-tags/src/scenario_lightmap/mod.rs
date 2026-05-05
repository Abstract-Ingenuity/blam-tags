//! Halo `scenario_lightmap_bsp_data` walker — per-BSP baked SH probes,
//! lightprobe atlas refs, cluster / instance / scenery probe assignments.
//!
//! Reference: `Ares/source/scenario/scenario_lightmap_definitions.h:90`.

mod types;

pub use types::{
    LightmapBspData, LightmapClusterEntry, LightmapError, LightmapInstanceEntry,
    LightmapPerVertexBlock, LightmapPerVertexProbe, LightmapPolicy, LightmapProbe,
};
