//! Dump an `.model` (hlmt) tag's variants + their child objects.
use blam_tags::{Model, TagFile};

fn main() {
    for p in std::env::args().skip(1) {
        println!("==== {p} ====");
        let Ok(tag) = TagFile::read(&p) else { println!("  (read fail)"); continue; };
        let m = match Model::from_tag(&tag) {
            Ok(m) => m,
            Err(e) => { println!("  decode fail: {e}"); continue; }
        };
        println!("  render_model='{}'", m.render_model);
        println!("  variants[{}]:", m.variants.len());
        for v in &m.variants {
            println!("    variant '{}' — {} child object(s)", v.name, v.objects.len());
            for o in &v.objects {
                let g = std::str::from_utf8(&o.child_object_group).unwrap_or("????");
                println!(
                    "      parent_marker='{}' child_marker='{}' child_variant='{}' child={} ({})",
                    o.parent_marker, o.child_marker, o.child_variant_name, o.child_object, g,
                );
            }
        }
    }
}
