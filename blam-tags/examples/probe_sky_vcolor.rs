//! Probe construct sky render_model: per-mesh vert_color range + flags.
use blam_tags::{RenderModel, TagFile};

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        "/Users/camden/Halo/halo3_mcc/tags/levels/multi/construct/sky/sky.render_model".to_string()
    });
    let tag = TagFile::read(&path).expect("read");
    let rm = RenderModel::from_tag(&tag).expect("from_tag");
    let meshes = RenderModel::derive_render_meshes(&tag).expect("derive");

    println!("materials:");
    for (i, m) in rm.materials.iter().enumerate() {
        println!("  [{i}] {}", m.shader_name());
    }
    println!("\nper-mesh vert_color (raw offline block):");
    for (i, m) in meshes.iter().enumerate() {
        let mut mn = [f32::MAX; 3];
        let mut mx = [f32::MIN; 3];
        let mut sum = [0f64; 3];
        for v in &m.vertices {
            let c = [v.vert_color.i, v.vert_color.j, v.vert_color.k];
            for k in 0..3 {
                mn[k] = mn[k].min(c[k]);
                mx[k] = mx[k].max(c[k]);
                sum[k] += c[k] as f64;
            }
        }
        let n = m.vertices.len().max(1) as f64;
        let mats: Vec<usize> = m.parts.iter().map(|p| p.material_index as usize).collect();
        let mat_names: Vec<String> = mats
            .iter()
            .map(|&mi| rm.materials.get(mi).map(|x| x.shader_name().to_string()).unwrap_or_else(|| "?".to_string()))
            .collect();
        println!(
            "  mesh[{i:2}] verts={:5} vc min=({:.2},{:.2},{:.2}) max=({:.2},{:.2},{:.2}) mean=({:.2},{:.2},{:.2}) mats={:?}",
            m.vertices.len(),
            mn[0], mn[1], mn[2], mx[0], mx[1], mx[2],
            sum[0]/n, sum[1]/n, sum[2]/n,
            mat_names,
        );
    }
}
