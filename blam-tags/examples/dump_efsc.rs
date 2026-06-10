use std::path::PathBuf;
use blam_tags::TagFile;
use blam_tags::scenario::Scenario;
fn main() {
    let p = std::env::args().nth(1).unwrap();
    let tag = TagFile::read(PathBuf::from(&p)).unwrap();
    let sc = Scenario::from_tag(&tag).unwrap();
    println!("object_names[{}]:", sc.object_names.len());
    println!("effect_scenery placements[{}]:", sc.effect_scenery.len());
    for (i,o) in sc.effect_scenery.iter().enumerate() {
        let d=&o.object_data; let r=d.rotation;
        println!("  [{i}] pal={} name_idx={} pos=({:.3},{:.3},{:.3}) yaw={:.2} parent={:?}",
            o.palette_index, o.name_index, d.position.x,d.position.y,d.position.z,
            r.yaw.to_degrees(), d.parent_id);
    }
    // also crate scenery placements with names
    println!("scenery placements[{}]:", sc.scenery.len());
    for (i,o) in sc.scenery.iter().enumerate() {
        println!("  scen[{i}] pal={} name_idx={} parent={:?}", o.palette_index, o.name_index, o.object_data.parent_id);
    }
}
