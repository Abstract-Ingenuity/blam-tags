//! Sweep every render_model / sbsp / instance_imposter / particle_model
//! / render_model_lightmap_atlas in a monolithic cache; resolve each
//! mesh's `vertex type` enum option name plus its `PRT vertex type`
//! and tally how often each appears. Output drives which decoders we
//! need to port from TagTool's `VertexCompressor` plus the engine
//! source.
//!
//! Usage:
//!
//! ```text
//! cargo run --release --example vertex_type_survey -- <cache_dir>
//! ```

use std::collections::BTreeMap;
use std::error::Error;

use blam_tags::api::TagStruct;
use blam_tags::monolithic::MonolithicCache;

/// `(group_tag, list of paths to a `meshes` block within that tag's
/// root struct)`. Different groups expose meshes at different paths.
const MESH_PATHS: &[(&[u8; 4], &[&str])] = &[
    (b"mode", &["render geometry/meshes"]),
    (b"sbsp", &["render geometry/meshes"]),
    (b"pmdf", &["render geometry/meshes"]),
    (b"impo", &["geometry/meshes"]),
    (b"iimz", &["render geometry/meshes"]),
    (b"Lbsp", &[
        "imported geometry/meshes",
        "shadow geometry/meshes",
        "Dynamic Light Shadow Geometry/meshes",
    ]),
    (b"rmla", &["atlas geometry/meshes"]),
];

fn main() -> Result<(), Box<dyn Error>> {
    let cache_dir = std::env::args().nth(1).ok_or("usage: <cache_dir>")?;
    let cache = MonolithicCache::open(&cache_dir)?;

    let mut vt_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut prt_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut pair_counts: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut by_group: BTreeMap<String, u64> = BTreeMap::new();
    let mut tags_seen: u64 = 0;
    let mut tags_failed: u64 = 0;
    let mut meshes_total: u64 = 0;

    for entry in cache.iter_tags() {
        let group_bytes = entry.group_tag.to_be_bytes();
        let Some(paths) = MESH_PATHS
            .iter()
            .find(|(g, _)| **g == group_bytes)
            .map(|(_, paths)| *paths)
        else {
            continue;
        };
        tags_seen += 1;
        let tag = match cache.read_tag(entry) {
            Ok(t) => t,
            Err(_) => {
                tags_failed += 1;
                continue;
            }
        };
        let root = tag.root();
        let group_label = std::str::from_utf8(&group_bytes)
            .unwrap_or("????")
            .trim_end_matches(['\0', ' '])
            .to_string();

        for path in paths {
            let Some(field) = root.field_path(path) else {
                continue;
            };
            let Some(meshes) = field.as_block() else {
                continue;
            };
            for mesh in meshes.iter() {
                meshes_total += 1;
                let vt = read_enum_name(&mesh, "vertex type").unwrap_or_else(|| "<missing>".into());
                let prt = read_enum_name(&mesh, "PRT vertex type").unwrap_or_else(|| "<missing>".into());
                *vt_counts.entry(vt.clone()).or_default() += 1;
                *prt_counts.entry(prt.clone()).or_default() += 1;
                *pair_counts.entry((vt, prt)).or_default() += 1;
                *by_group.entry(group_label.clone()).or_default() += 1;
            }
        }
    }

    println!("tags scanned: {tags_seen} ({tags_failed} parse-failed)");
    println!("meshes total: {meshes_total}");
    println!("\nmeshes by tag group:");
    for (g, n) in &by_group {
        println!("  {n:>7}  {g}");
    }
    println!("\nvertex type histogram:");
    for (name, n) in &vt_counts {
        println!("  {n:>7}  {name}");
    }
    println!("\nPRT vertex type histogram:");
    for (name, n) in &prt_counts {
        println!("  {n:>7}  {name}");
    }
    println!("\n(vertex type, PRT vertex type) pairs:");
    for ((vt, prt), n) in &pair_counts {
        println!("  {n:>7}  {vt:<32}  +  {prt}");
    }
    Ok(())
}

fn read_enum_name(s: &TagStruct<'_>, name: &str) -> Option<String> {
    s.read_enum_name(name)
}
