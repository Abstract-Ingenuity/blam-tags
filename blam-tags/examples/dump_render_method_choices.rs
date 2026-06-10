//! Verify the typed render-method layer: subclass class, outer
//! `material_names`, and name-resolved blend mode.
//!
//! usage: dump_render_method_choices <tags_root> <rm** tag path>

use blam_tags::file::TagFile;
use blam_tags::render_method::{RenderMethod, RenderMethodChoices, RenderMethodDefinition};

fn main() {
    let tags_root = std::env::args().nth(1).expect("usage: <tags_root> <rm path>");
    let rm_path = std::env::args().nth(2).expect("usage: <tags_root> <rm path>");

    let rm_tag = TagFile::read(&rm_path).expect("read rm");
    let rm = RenderMethod::from_tag(&rm_tag).expect("parse rm");

    println!("=== {rm_path} ===");
    println!("class           = {:?}", rm.class);
    println!("group_tag       = {:?}", String::from_utf8_lossy(&rm.group_tag.to_be_bytes()));
    println!("material_names  = {:?}", rm.material_names);
    println!("definition      = {}", rm.definition_path);

    // Resolve the rmdf and the category choices.
    let def = rm.definition_path.replace('\\', "/");
    let rmdf_path = format!("{tags_root}/{def}.render_method_definition");
    let rmdf_tag = TagFile::read(&rmdf_path).expect("read rmdf");
    let rmdf = RenderMethodDefinition::from_tag(&rmdf_tag).expect("parse rmdf");
    let choices = RenderMethodChoices::resolve(&rm, &rmdf);

    println!("choices:");
    for c in choices.choices() {
        println!("  {:<22} = {:<28} (option_index {})", c.category_name, c.option_name, c.option_index);
    }
    println!("blend_mode (typed, by name) = {:?}", choices.blend_mode());
}
