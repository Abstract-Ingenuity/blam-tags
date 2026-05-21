//! Dump the (category_name, [option_names]) list from a
//! `render_method_definition` tag.

use blam_tags::file::TagFile;
use blam_tags::render_method::RenderMethodDefinition;

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump_rmdf_categories <rmdf path>");
    let tag = TagFile::read(&path).expect("read tag");
    let rmdf = RenderMethodDefinition::from_tag(&tag).expect("parse rmdf");
    println!("=== {} ===", path);
    println!("{} categories:", rmdf.categories.len());
    for (i, cat) in rmdf.categories.iter().enumerate() {
        let opts: Vec<String> = cat.options.iter().map(|o| o.option_name.clone()).collect();
        println!("  [{i:2}] {} → [{}]", cat.category_name, opts.join(", "));
    }
}
