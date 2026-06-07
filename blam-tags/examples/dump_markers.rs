use blam_tags::render_model::RenderModel;
use blam_tags::TagFile;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let tag = TagFile::read(&path).unwrap();
    let m = RenderModel::from_tag(&tag).unwrap();
    let total: usize = m.marker_groups.iter().map(|g| g.markers.len()).sum();
    println!("=== {} markers ({}) ===", path, total);
    for g in &m.marker_groups {
        for mk in &g.markers {
            println!("  '{}' node={} pos=({:.3},{:.3},{:.3})", g.name, mk.node_index, mk.translation.x, mk.translation.y, mk.translation.z);
        }
    }
}
