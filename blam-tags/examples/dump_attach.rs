use std::path::PathBuf;
use blam_tags::TagFile;
use blam_tags::object::ObjectDefinition;
fn main() {
    for p in std::env::args().skip(1) {
        let path = PathBuf::from(&p);
        println!("==== {p} ====");
        let Ok(tag) = TagFile::read(&path) else { println!("  (read fail)"); continue; };
        let def = ObjectDefinition::from_tag(&tag).unwrap_or_default();
        println!("  model='{}'", def.model);
        println!("  creation_effect='{}'", def.creation_effect);
        println!("  attachments[{}]:", def.attachments.len());
        for (i,a) in def.attachments.iter().enumerate() {
            let g = std::str::from_utf8(&a.type_group).unwrap_or("????");
            println!("    [{i}] group={g} ref='{}' marker='{}'", a.type_ref, a.marker);
        }
    }
}
