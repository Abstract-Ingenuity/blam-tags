use blam_tags::render_model::RenderModel;
use blam_tags::TagFile;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let tag = TagFile::read(&path).unwrap();
    let m = RenderModel::from_tag(&tag).unwrap();
    println!("=== {} markers ({}) ===", path, m.markers.len());
    for mk in &m.markers {
        println!("  '{}' node={} pos=({:.3},{:.3},{:.3})", mk.name, mk.node_index, mk.translation.x, mk.translation.y, mk.translation.z);
    }
}
