//! Probe a single render-geometry-carrying tag in a monolithic
//! cache: hydrate it, find the xsync-backed `api resource`, apply
//! control fixups, walk the resource definition struct, and dump
//! every vertex / index buffer descriptor we found. Used to
//! validate the resource walker against a known good test case
//! (`cinematics\tac_pad\tac_pad_test.mode`).

use std::error::Error;

use blam_tags::monolithic::{MonolithicCache, XSyncState};
use blam_tags::render_geometry::RenderGeometryResource;
use blam_tags::{TagField, TagFieldType, TagStruct};

fn main() -> Result<(), Box<dyn Error>> {
    let cache_dir = std::env::args().nth(1).ok_or("usage: <cache_dir> <group> <name>")?;
    let group_arg = std::env::args().nth(2).ok_or("missing group arg")?;
    let name = std::env::args().nth(3).ok_or("missing name arg")?;

    let cache = MonolithicCache::open(&cache_dir)?;
    let group_tag = blam_tags::parse_group_tag(&group_arg).ok_or("bad group")?;

    let entry = cache.find_tag(group_tag, &name).ok_or("tag not found")?;
    let tag = cache.read_tag(entry)?;

    let Some(state) = find_first_xsync_state(&tag.root()) else {
        eprintln!("no hydrated xsync resource found in tag");
        return Ok(());
    };

    println!("--- xsync state (control_data={} B) ---", state.header.control_data_size);
    run_with_xsync(state)?;

    // Confirm the hydrator filled author-format `per mesh temporary`
    // alongside the GPU descriptors above.
    dump_per_mesh_temporary(&tag.root());
    Ok(())
}

fn dump_per_mesh_temporary(root: &TagStruct<'_>) {
    let Some(pmt) = root
        .field_path("render geometry/per mesh temporary")
        .and_then(|f| f.as_block())
    else {
        println!("\nno `render geometry/per mesh temporary` block on this tag");
        return;
    };
    println!("\nper mesh temporary[]: {} elements", pmt.len());
    for i in 0..pmt.len() {
        let elem = pmt.element(i).unwrap();
        let raw_v = elem.field("raw vertices").and_then(|f| f.as_block()).map(|b| b.len()).unwrap_or(0);
        let raw_i = elem.field("raw indices").and_then(|f| f.as_block()).map(|b| b.len()).unwrap_or(0);
        let raw_i32 = elem.field("raw indices32").and_then(|f| f.as_block()).map(|b| b.len()).unwrap_or(0);
        println!("  [{i}] raw_vertices={raw_v}  raw_indices={raw_i}  raw_indices32={raw_i32}");
        if raw_v > 0 {
            let v0 = elem.field("raw vertices").and_then(|f| f.as_block()).and_then(|b| b.element(0)).unwrap();
            let p = v0.read_point3d("position");
            let n = v0.read_point3d("normal");
            let uv = v0.read_point2d("texcoord");
            println!("      vertex[0] pos=({:.4},{:.4},{:.4}) normal=({:.3},{:.3},{:.3}) uv=({:.4},{:.4})",
                p.x, p.y, p.z, n.x, n.y, n.z, uv.x, uv.y);
        }
    }
}

/// Walk every reachable struct under `root` and return the first
/// `pageable_resource` field whose hydrated form carries an
/// [`XSyncState`]. Returns `None` if there is no resource (or none
/// have an attached state — e.g. an MCC-native tag).
fn find_first_xsync_state<'a>(root: &TagStruct<'a>) -> Option<&'a XSyncState> {
    for field in root.fields() {
        if let Some(state) = check_field(&field) {
            return Some(state);
        }
    }
    None
}

fn check_field<'a>(field: &TagField<'a>) -> Option<&'a XSyncState> {
    match field.field_type() {
        TagFieldType::PageableResource => {
            let res = field.as_resource()?;
            res.xsync_state()
        }
        TagFieldType::Struct => find_first_xsync_state(&field.as_struct()?),
        TagFieldType::Block => {
            let block = field.as_block()?;
            for i in 0..block.len() {
                if let Some(state) = find_first_xsync_state(&block.element(i)?) {
                    return Some(state);
                }
            }
            None
        }
        TagFieldType::Array => {
            let arr = field.as_array()?;
            for i in 0..arr.len() {
                if let Some(state) = find_first_xsync_state(&arr.element(i)?) {
                    return Some(state);
                }
            }
            None
        }
        _ => None,
    }
}

fn run_with_xsync(state: &XSyncState) -> Result<(), Box<dyn Error>> {
    println!(
        "xsync header: cache_loc=(0x{:x},{} B)  optional=(0x{:x},{} B)  control_data={} B  fixups={}",
        state.header.cache_location_offset,
        state.header.cache_location_size,
        state.header.optional_location_offset,
        state.header.optional_location_size,
        state.header.control_data_size,
        state.header.control_fixup_count,
    );

    let fixed = state.apply_control_fixups();
    let root_addr = blam_tags::monolithic::FixupAddress(state.header.root_address);
    println!("root address: tier={:?} offset=0x{:x}", root_addr.tier(), root_addr.offset());

    let Some(geom) = RenderGeometryResource::parse(&fixed, root_addr) else {
        eprintln!("failed to parse resource definition");
        return Ok(());
    };
    println!(
        "parsed: pc_vbs={} pc_ibs={} xenon_vbs={} xenon_ibs={}",
        geom.pc_vertex_buffers.len(),
        geom.pc_index_buffers.len(),
        geom.xenon_vertex_buffers.len(),
        geom.xenon_index_buffers.len(),
    );

    println!("\nxenon vertex buffers:");
    for (i, vb) in geom.xenon_vertex_buffers.iter().enumerate() {
        println!(
            "  [{i}] count={} decl={} stride={} data=({:?},0x{:x}) size={}",
            vb.vertex_count,
            vb.declaration,
            vb.stride,
            vb.data_address.tier(),
            vb.data_address.offset(),
            vb.data_size,
        );
    }
    println!("\nxenon index buffers:");
    for (i, ib) in geom.xenon_index_buffers.iter().enumerate() {
        println!(
            "  [{i}] primitive_type={} is_index32={} data=({:?},0x{:x}) size={}",
            ib.primitive_type,
            ib.is_index32,
            ib.data_address.tier(),
            ib.data_address.offset(),
            ib.data_size,
        );
    }
    Ok(())
}
