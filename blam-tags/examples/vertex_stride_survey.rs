//! For every render-geometry-carrying tag in a monolithic cache,
//! resolve the api-resource's vertex-buffer table and tally
//! `(vertex_type, primary_buffer_stride)` pairs. Used to confirm
//! the actual on-disk stride per `mesh_vertex_type_definition`
//! variant (which is the source of truth — the schema's enum
//! variant doesn't dictate stride directly).
//!
//! Usage:
//! ```text
//! cargo run --release --example vertex_stride_survey -- <cache_dir>
//! ```
//!
//! Output is sorted by frequency.

use std::collections::BTreeMap;
use std::error::Error;

use blam_tags::api::TagStruct;
use blam_tags::monolithic::{FixupAddress, MonolithicCache};
use blam_tags::render_geometry::RenderGeometryResource;

const MESH_PATHS: &[(&[u8; 4], &[&str])] = &[
    (b"mode", &["render geometry"]),
    (b"sbsp", &["render geometry"]),
    (b"pmdf", &["render geometry"]),
    (b"impo", &["geometry"]),
    (b"iimz", &["render geometry"]),
    (b"Lbsp", &[
        "imported geometry",
        "shadow geometry",
        "Dynamic Light Shadow Geometry",
    ]),
    (b"rmla", &["atlas geometry"]),
];

fn main() -> Result<(), Box<dyn Error>> {
    let cache_dir = std::env::args().nth(1).ok_or("usage: <cache_dir>")?;
    let cache = MonolithicCache::open(&cache_dir)?;

    // `(vertex_type_schema_name, stride_bytes) -> mesh_count`.
    let mut pair_counts: BTreeMap<(String, u32), u64> = BTreeMap::new();
    // `(vertex_type_schema_name, stride_bytes, declaration_index) -> count`,
    // because `decl` might disambiguate sub-formats with the same name.
    let mut triple_counts: BTreeMap<(String, u32, u32), u64> = BTreeMap::new();

    let mut tags_seen: u64 = 0;
    let mut tags_skipped: u64 = 0;
    let mut tags_no_xsync: u64 = 0;
    let mut meshes_total: u64 = 0;

    for entry in cache.iter_tags() {
        let group_bytes = entry.group_tag.to_be_bytes();
        let Some(paths) = MESH_PATHS.iter().find(|(g, _)| **g == group_bytes).map(|(_, p)| *p)
        else {
            continue;
        };
        tags_seen += 1;
        let tag = match cache.read_tag(entry) {
            Ok(t) => t,
            Err(_) => { tags_skipped += 1; continue; }
        };
        let root = tag.root();

        for path in paths {
            let Some(rg) = root.field_path(path).and_then(|f| f.as_struct()) else { continue; };
            let xsync = rg.field("api resource")
                .and_then(|f| f.as_resource())
                .and_then(|r| r.xsync_state());
            let Some(state) = xsync else {
                tags_no_xsync += 1;
                continue;
            };
            let fixed = state.apply_control_fixups();
            let Some(resource) = RenderGeometryResource::parse(&fixed, FixupAddress(state.header.root_address))
            else { continue; };
            let vbs = &resource.xenon_vertex_buffers;

            let Some(meshes) = rg.field("meshes").and_then(|f| f.as_block()) else { continue; };
            for mesh in meshes.iter() {
                meshes_total += 1;
                let vt = mesh.read_enum_name("vertex type").unwrap_or_else(|| "<missing>".into());
                let vbi0 = read_vbi(&mesh, 0);
                let stride = if vbi0 >= 0 {
                    vbs.get(vbi0 as usize).map(|vb| vb.stride as u32).unwrap_or(0)
                } else {
                    0
                };
                let decl: u32 = if vbi0 >= 0 {
                    vbs.get(vbi0 as usize).map(|vb| vb.declaration as u32).unwrap_or(0)
                } else {
                    0
                };
                *pair_counts.entry((vt.clone(), stride)).or_default() += 1;
                *triple_counts.entry((vt, stride, decl)).or_default() += 1;
            }
        }
    }

    println!("tags scanned: {tags_seen}  ({tags_skipped} parse-failed)  ({tags_no_xsync} no-xsync)");
    println!("meshes total: {meshes_total}\n");
    println!("(vertex type, primary buffer stride) — sorted by count:");
    let mut sorted: Vec<_> = pair_counts.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));
    for ((vt, stride), n) in sorted {
        println!("  {n:>7}  stride={stride:<4}  {vt}");
    }
    println!("\n(vertex type, stride, decl) tuples:");
    let mut sorted: Vec<_> = triple_counts.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));
    for ((vt, stride, decl), n) in sorted {
        println!("  {n:>7}  stride={stride:<4}  decl={decl:<4}  {vt}");
    }
    Ok(())
}

fn read_vbi(mesh: &TagStruct<'_>, slot: usize) -> i64 {
    mesh.field("vertex buffer indices")
        .and_then(|f| f.as_array())
        .and_then(|a| a.element(slot))
        .and_then(|e| e.read_int_any("vertex buffer index"))
        .map(|v| v as i64)
        .unwrap_or(-1)
}
