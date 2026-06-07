//! Dump every enum / flags field embedded in a tag, with the tag's OWN
//! option/bit names (resolved from its embedded string table) and the
//! current value. Use this to compare what a tag actually carries against
//! the JSON schema / our authored typed enums — i.e. to spot options that
//! were added or removed across schema revisions.
//!
//! Usage: dump_tag_enums <tag-path> [--all-fields]
//!   --all-fields also lists non-enum field names (so you can see whether
//!   a field is ABSENT in this tag vs renamed/retyped).

use blam_tags::api::{TagOptions, TagStruct};
use blam_tags::fields::TagFieldType;
use blam_tags::file::TagFile;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: dump_tag_enums <tag-path> [--all-fields]");
    let all_fields = args.any(|a| a == "--all-fields");

    let tag = TagFile::read(&path).expect("failed to read tag");
    println!("=== {} (group '{}') ===", path, blam_tags::fields::format_group_tag(tag.group().tag));
    walk(&tag.root(), "", all_fields);
}

fn walk(s: &TagStruct<'_>, prefix: &str, all_fields: bool) {
    for f in s.fields() {
        let name = f.name();
        let ty = f.field_type();
        match f.options() {
            Some(TagOptions::Enum { names, current }) => {
                let cur_name = current
                    .and_then(|c| usize::try_from(c).ok())
                    .and_then(|i| names.get(i).copied())
                    .unwrap_or("<out-of-range>");
                println!(
                    "{prefix}{name} : ENUM = {current:?} ({cur_name})  options[{}]={:?}",
                    names.len(),
                    names,
                );
            }
            Some(TagOptions::Flags(bits)) => {
                let set: Vec<&str> = bits.iter().filter(|b| b.is_set).map(|b| b.name).collect();
                let all: Vec<&str> = bits.iter().map(|b| b.name).collect();
                println!(
                    "{prefix}{name} : FLAGS set={set:?}  options[{}]={:?}",
                    all.len(),
                    all,
                );
            }
            None => {
                if all_fields {
                    println!("{prefix}{name} : {ty:?}");
                }
            }
        }
        // Recurse into nested structs and the first element of blocks.
        if ty == TagFieldType::Struct {
            if let Some(sub) = f.as_struct() {
                walk(&sub, &format!("{prefix}{name}."), all_fields);
            }
        } else if let Some(block) = f.as_block() {
            if let Some(elem) = block.element(0) {
                walk(&elem, &format!("{prefix}{name}[0]."), all_fields);
            }
        }
    }
}
