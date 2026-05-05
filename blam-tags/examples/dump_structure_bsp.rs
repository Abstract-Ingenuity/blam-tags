//! Dump a Halo structure_bsp tag's rendering-relevant fields.
//!
//! Usage:
//!   cargo run --example dump_structure_bsp -- <path/to/level.scenario_structure_bsp>

use std::path::PathBuf;

use blam_tags::TagFile;
use blam_tags::structure_bsp::StructureBsp;

fn main() {
    let Some(path_str) = std::env::args().nth(1) else {
        eprintln!("usage: dump_structure_bsp <path/to/level.scenario_structure_bsp>");
        std::process::exit(2);
    };
    let path = PathBuf::from(&path_str);
    let tag = TagFile::read(&path).unwrap_or_else(|e| {
        eprintln!("failed to read {}: {e}", path.display());
        std::process::exit(1);
    });
    let bsp = StructureBsp::from_tag(&tag).expect("StructureBsp::from_tag failed");

    println!("structure_bsp: {}", path.display());
    println!("  flags:        0x{:08x}", bsp.flags);
    println!(
        "  world_bounds: x[{:.2}..{:.2}] y[{:.2}..{:.2}] z[{:.2}..{:.2}]",
        bsp.world_bounds_x.lower,
        bsp.world_bounds_x.upper,
        bsp.world_bounds_y.lower,
        bsp.world_bounds_y.upper,
        bsp.world_bounds_z.lower,
        bsp.world_bounds_z.upper,
    );
    println!();

    println!("materials: [{}]", bsp.materials.len());
    for (i, m) in bsp.materials.iter().take(8).enumerate() {
        println!("  [{i}] {}  (imported_index={})", short(&m.render_method), m.imported_material_index);
    }
    if bsp.materials.len() > 8 {
        println!("  ... +{} more", bsp.materials.len() - 8);
    }
    println!();

    println!("collision_materials: [{}]", bsp.collision_materials.len());
    for (i, m) in bsp.collision_materials.iter().take(4).enumerate() {
        println!("  [{i}] {}", short(&m.render_method));
    }
    if bsp.collision_materials.len() > 4 {
        println!("  ... +{} more", bsp.collision_materials.len() - 4);
    }
    println!();

    println!("clusters: [{}]", bsp.clusters.len());
    for (i, c) in bsp.clusters.iter().enumerate() {
        println!(
            "  [{i:2}] mesh={} sky={} portals=[{}] flags=0x{:04x}",
            c.mesh_index,
            c.scenario_sky_index,
            c.portals.len(),
            c.flags,
        );
    }
    println!();

    println!("instanced_geometry_instances: [{}]", bsp.instanced_geometry_instances.len());
    for (i, inst) in bsp.instanced_geometry_instances.iter().take(8).enumerate() {
        println!(
            "  [{i}] def={:3} pos=({:.2},{:.2},{:.2}) scale={:.3} lm_policy={} flags=0x{:04x} name={}",
            inst.definition_index,
            inst.position.x, inst.position.y, inst.position.z,
            inst.scale,
            inst.lightmapping_policy,
            inst.flags,
            short_str(&inst.name),
        );
    }
    if bsp.instanced_geometry_instances.len() > 8 {
        println!("  ... +{} more", bsp.instanced_geometry_instances.len() - 8);
    }
    println!();

    println!("cluster_portals: [{}]", bsp.cluster_portals.len());
    println!("sky_owner_clusters: {:?}", bsp.sky_owner_clusters);
    println!();

    println!("meshes_metadata: [{}]", bsp.meshes_metadata.len());
    for (i, m) in bsp.meshes_metadata.iter().take(4).enumerate() {
        println!(
            "  [{i:3}] vertex_type={} mesh_flags=0x{:02x} rigid_node={} idx_buffer_type={} parts=[{}]",
            m.vertex_type, m.mesh_flags, m.rigid_node_index, m.index_buffer_type, m.parts.len(),
        );
        for (j, p) in m.parts.iter().take(3).enumerate() {
            let mat = bsp
                .materials
                .get(p.render_method_index.max(0) as usize)
                .map(|m| m.render_method.as_str())
                .unwrap_or("<INVALID>");
            println!(
                "        part[{j}] rm_idx={} indices={}..+{} -> {}",
                p.render_method_index, p.index_start, p.index_count, short(mat),
            );
        }
    }
    if bsp.meshes_metadata.len() > 4 {
        println!("  ... +{} more", bsp.meshes_metadata.len() - 4);
    }
    println!();

    println!("markers: [{}]", bsp.markers.len());
    for (i, m) in bsp.markers.iter().enumerate() {
        println!(
            "  [{i}] {:24}  node={} pos=({:.2},{:.2},{:.2})",
            short_str(&m.name), m.node_index, m.position.x, m.position.y, m.position.z,
        );
    }
}

fn short(s: &str) -> &str {
    if s.is_empty() { "(none)" } else { s }
}

fn short_str(s: &str) -> &str {
    if s.is_empty() { "<unnamed>" } else { s }
}
