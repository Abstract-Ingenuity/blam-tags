use blam_tags::TagFile;
fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        "/Users/camden/Halo/halo3_mcc/tags/globals/falling.damage_effect".into()
    });
    let t = TagFile::read(&path).unwrap();
    let root = t.root();
    println!("ROOT fields:");
    for f in root.fields() {
        println!("  type={:?}  name={:?}", f.field_type(), f.name());
    }
    if let Some(b) = root.field("player responses").and_then(|f| f.as_block()) {
        println!("\nplayer responses[{}] sample 0:", b.len());
        if let Some(elem) = b.element(0) {
            for f in elem.fields() {
                println!("  type={:?}  name={:?}", f.field_type(), f.name());
            }
        }
    }
}
