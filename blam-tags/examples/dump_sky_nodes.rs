use blam_tags::render_model::RenderModel;
use blam_tags::TagFile;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        "/Users/camden/Halo/halo3_mcc/tags/levels/dlc/bunkerworld/sky/sky.render_model".to_string()
    });
    let tag = TagFile::read(&path).unwrap();
    let m = RenderModel::from_tag(&tag).unwrap();

    println!("=== {} ===", path);
    println!("nodes: {}   meshes: {}   materials: {}", m.nodes.len(), m.meshes.len(), m.materials.len());
    println!("default_node_orientations: {}", m.default_node_orientations.len());
    println!("\n--- NODES ---");
    for (i, n) in m.nodes.iter().enumerate() {
        let t = n.default_translation;
        let q = n.default_rotation;
        println!(
            "[{i:2}] {:<24} parent={:3} trans=({:9.3},{:9.3},{:9.3}) rot=({:.4},{:.4},{:.4},{:.4})",
            n.name, n.parent_index, t.x, t.y, t.z, q.i, q.j, q.k, q.w
        );
    }

    println!("\n--- MESHES (rigid_node + vertex bbox) ---");
    for (mi, mesh) in m.meshes.iter().enumerate() {
        let mut mn = [f32::INFINITY; 3];
        let mut mx = [f32::NEG_INFINITY; 3];
        for v in &mesh.vertices {
            let p = v.position;
            mn[0] = mn[0].min(p.x); mn[1] = mn[1].min(p.y); mn[2] = mn[2].min(p.z);
            mx[0] = mx[0].max(p.x); mx[1] = mx[1].max(p.y); mx[2] = mx[2].max(p.z);
        }
        // also gather which node_indices skinned verts reference
        let mut nodes_used = std::collections::BTreeSet::new();
        for v in &mesh.vertices {
            for (k, w) in v.node_indices.iter().zip(v.node_weights.iter()) {
                if *w > 0.0 { nodes_used.insert(*k); }
            }
        }
        println!(
            "mesh[{mi:2}] verts={:5} rigid_node={:?} parts={} bbox=[{:8.2},{:8.2},{:8.2}]..[{:8.2},{:8.2},{:8.2}] skin_nodes={:?}",
            mesh.vertices.len(), mesh.rigid_node_index, mesh.parts.len(),
            mn[0], mn[1], mn[2], mx[0], mx[1], mx[2], nodes_used
        );
    }
}
