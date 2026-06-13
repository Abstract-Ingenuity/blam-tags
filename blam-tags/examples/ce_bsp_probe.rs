//! Probe one CE scenario_structure_bsp: build render + collision JMS,
//! print counts, and check whether render vertex positions fall within
//! the tag's `world bounds` (a sanity check on float endianness).
//! Usage: ce_bsp_probe <defs-dir> <sbsp-file>

use std::path::PathBuf;

use blam_tags::classic::read_classic_tag_file;
use blam_tags::jms::JmsFile;
use blam_tags::layout::TagLayout;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let defs = PathBuf::from(&a[1]);
    let bytes = std::fs::read(&a[2]).unwrap();
    let layout = TagLayout::from_json(defs.join("scenario_structure_bsp.json")).unwrap();
    let tag = read_classic_tag_file(&bytes, layout).unwrap();
    let root = tag.root();

    let bounds = |n: &str| {
        root.field_path(n).and_then(|f| f.value()).map(|v| format!("{v:?}"))
    };
    println!("world bounds x/y/z: {:?} {:?} {:?}",
        bounds("world bounds x"), bounds("world bounds y"), bounds("world bounds z"));

    let render = JmsFile::from_scenario_structure_bsp_ce(&tag).unwrap();
    let coll = JmsFile::from_scenario_structure_bsp_ce_collision(&tag).unwrap();
    println!("RENDER: {} mats, {} verts, {} tris", render.materials.len(), render.vertices.len(), render.triangles.len());
    println!("COLL:   {} mats, {} verts, {} tris", coll.materials.len(), coll.vertices.len(), coll.triangles.len());

    // Bounding box of render verts (in JMS world units = tag units * 100).
    if !render.vertices.is_empty() {
        let mut mn = [f32::MAX; 3]; let mut mx = [f32::MIN; 3];
        for v in &render.vertices {
            let p = [v.position.x, v.position.y, v.position.z];
            for i in 0..3 { mn[i] = mn[i].min(p[i]); mx[i] = mx[i].max(p[i]); }
        }
        // divide by 100 to compare against tag-space world bounds
        println!("render bbox (tag units): x[{:.2},{:.2}] y[{:.2},{:.2}] z[{:.2},{:.2}]",
            mn[0]/100.0, mx[0]/100.0, mn[1]/100.0, mx[1]/100.0, mn[2]/100.0, mx[2]/100.0);
    }
}
