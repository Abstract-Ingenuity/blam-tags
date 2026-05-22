//! Sanity check for the runtime render_model extractor. Prints
//! totals so we can eyeball that crate_space.render_model decodes
//! to plausible numbers.

use blam_tags::{RenderModel, TagFile};

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump_render_model <path.render_model>");
    let tag = TagFile::read(&path).expect("failed to read tag");
    let rm = RenderModel::from_tag(&tag).expect("failed to extract render_model");
    println!("nodes: {}", rm.nodes.len());
    println!("materials: {}", rm.materials.len());
    for (i, m) in rm.materials.iter().enumerate() {
        println!("  [{i}] {} ({})", m.shader_name, m.shader_path);
    }
    println!("regions: {}", rm.regions.len());
    for r in &rm.regions {
        println!("  region '{}' ({} perms)", r.name, r.permutations.len());
        for p in &r.permutations {
            println!("    perm '{}' meshes [{}, {})", p.name, p.mesh_index, p.mesh_index + p.mesh_count);
        }
    }
    println!("meshes: {}", rm.meshes.len());
    let mut total_verts = 0;
    let mut total_tris = 0;
    for (i, m) in rm.meshes.iter().enumerate() {
        total_verts += m.vertices.len();
        total_tris += m.indices.len() / 3;
        println!(
            "  mesh[{i}]: {} verts, {} indices ({} tris), {} parts, rigid_node={:?}",
            m.vertices.len(), m.indices.len(), m.indices.len() / 3, m.parts.len(), m.rigid_node_index,
        );
    }
    println!("totals: {} verts, {} tris", total_verts, total_tris);
    println!(
        "default_node_orientations: {} (tag block, empty for extracted tags)",
        rm.default_node_orientations.len(),
    );
    println!("node_bind_pose: {} (derived if tag block empty)", rm.node_bind_pose().len());
    println!("markers: {}", rm.markers.len());
    if let Some(v) = rm.meshes.iter().flat_map(|m| m.vertices.iter()).next() {
        println!(
            "sample vertex: pos=({:.3},{:.3},{:.3}) uv=({:.3},{:.3}) normal=({:.3},{:.3},{:.3}) idx={:?} wts={:?}",
            v.position.x, v.position.y, v.position.z,
            v.texcoord.x, v.texcoord.y,
            v.normal.i, v.normal.j, v.normal.k,
            v.node_indices, v.node_weights,
        );
    }
}
