use blam_tags::{TagFieldData, TagFile};
fn main() {
    let tag = TagFile::read("/Users/camden/Halo/halo3_mcc/tags/objects/characters/masterchief/masterchief.render_model").unwrap();
    let nodes = tag.root().field_path("nodes").and_then(|f| f.as_block()).unwrap();
    println!("=== TAG nodes (first 6) ===");
    for i in 0..6.min(nodes.len()) {
        let n = nodes.element(i).unwrap();
        let name = n.field("name").and_then(|f| f.value()).map(|v| match v {
            TagFieldData::StringId(s) | TagFieldData::OldStringId(s) => s.string,
            _ => String::new(),
        }).unwrap_or_default();
        let parent = n.field("parent node").and_then(|f| f.value()).map(|v| match v {
            TagFieldData::ShortBlockIndex(p) => p as i32, _ => -99,
        }).unwrap_or(-99);
        let trans = n.field("default translation").and_then(|f| f.value()).map(|v| match v {
            TagFieldData::RealPoint3d(p) => format!("({:.4}, {:.4}, {:.4})", p.x, p.y, p.z),
            _ => "?".to_string(),
        }).unwrap_or_default();
        let rot = n.field("default rotation").and_then(|f| f.value()).map(|v| match v {
            TagFieldData::RealQuaternion(q) => format!("({:.4}, {:.4}, {:.4}, {:.4})", q.i, q.j, q.k, q.w),
            _ => "?".to_string(),
        }).unwrap_or_default();
        let inv_pos = n.field("inverse position").and_then(|f| f.value()).map(|v| match v {
            TagFieldData::RealPoint3d(p) => format!("({:.4}, {:.4}, {:.4})", p.x, p.y, p.z),
            _ => "?".to_string(),
        }).unwrap_or_default();
        let dist = n.field("distance from parent").and_then(|f| f.value()).map(|v| match v {
            TagFieldData::Real(r) => format!("{:.4}", r), _ => "?".to_string(),
        }).unwrap_or_default();
        println!("[{i}] {} parent={} dist={} trans={} rot={} inv_pos={}", name, parent, dist, trans, rot, inv_pos);
    }
}
