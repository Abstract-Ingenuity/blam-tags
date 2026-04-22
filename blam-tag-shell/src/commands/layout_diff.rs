use anyhow::Result;
use blam_tags::fields::TagFieldType;
use blam_tags::file::TagFile;
use blam_tags::layout::{TagFieldDefinition, TagLayout, TagStructDefinition};

use crate::format::format_tag_group;

pub fn run(file_a: &str, file_b: &str) -> Result<()> {
    let tag_a =
        TagFile::read(file_a).map_err(|e| anyhow::anyhow!("failed to parse first file: {e}"))?;
    let tag_b =
        TagFile::read(file_b).map_err(|e| anyhow::anyhow!("failed to parse second file: {e}"))?;

    let name_a = std::path::Path::new(file_a)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(file_a);
    let name_b = std::path::Path::new(file_b)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(file_b);

    println!("Layout diff: {name_a} vs {name_b}");
    println!();

    let header_a = &tag_a.header;
    let header_b = &tag_b.header;
    if header_a.group_tag != header_b.group_tag {
        println!(
            "  group_tag: {} -> {}",
            format_tag_group(header_a.group_tag),
            format_tag_group(header_b.group_tag),
        );
    }
    if header_a.group_version != header_b.group_version {
        println!(
            "  group_version: {} -> {}",
            header_a.group_version, header_b.group_version
        );
    }

    let layout_a = &tag_a.tag_stream.layout.layout;
    let layout_b = &tag_b.tag_stream.layout.layout;

    let root_struct_a = layout_a.block_definitions[layout_a.header.tag_group_block_index as usize]
        .struct_index as usize;
    let root_struct_b = layout_b.block_definitions[layout_b.header.tag_group_block_index as usize]
        .struct_index as usize;

    diff_struct(layout_a, root_struct_a, layout_b, root_struct_b, 1)?;

    Ok(())
}

fn diff_struct(
    layout_a: &TagLayout,
    struct_a_index: usize,
    layout_b: &TagLayout,
    struct_b_index: usize,
    indent: usize,
) -> Result<()> {
    let pad = "  ".repeat(indent);
    let struct_a = &layout_a.struct_definitions[struct_a_index];
    let struct_b = &layout_b.struct_definitions[struct_b_index];

    let name_a = layout_a.get_string(struct_a.name_offset).unwrap_or("?");
    let name_b = layout_b.get_string(struct_b.name_offset).unwrap_or("?");
    let name_changed = name_a != name_b;
    let size_changed = struct_a.size != struct_b.size;
    let guid_changed = struct_a.guid != struct_b.guid;

    let fields_a = collect_fields(layout_a, struct_a);
    let fields_b = collect_fields(layout_b, struct_b);

    if !name_changed && !size_changed && !guid_changed && fields_equal(layout_a, &fields_a, layout_b, &fields_b) {
        return Ok(());
    }

    if name_changed {
        println!("{pad}struct {} -> {}:", name_a, name_b);
    } else {
        println!("{pad}struct {}:", name_a);
    }

    if guid_changed {
        println!(
            "{pad}  guid: {:02x?} -> {:02x?}",
            &struct_a.guid[..4],
            &struct_b.guid[..4]
        );
    }
    if size_changed {
        let delta = struct_b.size as isize - struct_a.size as isize;
        println!(
            "{pad}  size: {} -> {} ({:+})",
            struct_a.size, struct_b.size, delta
        );
    }

    // Removed fields.
    for &idx_a in &fields_a {
        let field = &layout_a.fields[idx_a];
        let name = layout_a.get_string(field.name_offset).unwrap_or("?");
        if name.is_empty() || !fields_b.iter().any(|&i| layout_b.get_string(layout_b.fields[i].name_offset).unwrap_or("?") == name) {
            let type_name = field_type_name(layout_a, field);
            println!("{pad}  - {name} : {type_name} @ {}", field.offset);
        }
    }

    // Added fields.
    for &idx_b in &fields_b {
        let field = &layout_b.fields[idx_b];
        let name = layout_b.get_string(field.name_offset).unwrap_or("?");
        if name.is_empty() || !fields_a.iter().any(|&i| layout_a.get_string(layout_a.fields[i].name_offset).unwrap_or("?") == name) {
            let type_name = field_type_name(layout_b, field);
            println!("{pad}  + {name} : {type_name} @ {}", field.offset);
        }
    }

    // Changed / recursed fields.
    for &idx_a in &fields_a {
        let field_a = &layout_a.fields[idx_a];
        let name_a = layout_a.get_string(field_a.name_offset).unwrap_or("?");
        if name_a.is_empty() {
            continue;
        }
        let Some(&idx_b) = fields_b.iter().find(|&&i| {
            layout_b.get_string(layout_b.fields[i].name_offset).unwrap_or("?") == name_a
        }) else {
            continue;
        };
        let field_b = &layout_b.fields[idx_b];

        let mut changes = Vec::new();
        if field_a.field_type != field_b.field_type {
            changes.push(format!(
                "type: {} -> {}",
                field_type_name(layout_a, field_a),
                field_type_name(layout_b, field_b),
            ));
        }
        if field_a.offset != field_b.offset {
            changes.push(format!("offset: {} -> {}", field_a.offset, field_b.offset));
        }

        if !changes.is_empty() {
            println!("{pad}  ~ {name_a} : {}", changes.join(", "));
        }

        // Recurse into nested struct / block / array.
        match (&field_a.field_type, &field_b.field_type) {
            (TagFieldType::Struct, TagFieldType::Struct) => {
                diff_struct(
                    layout_a,
                    field_a.definition as usize,
                    layout_b,
                    field_b.definition as usize,
                    indent + 2,
                )?;
            }
            (TagFieldType::Block, TagFieldType::Block) => {
                let block_a = &layout_a.block_definitions[field_a.definition as usize];
                let block_b = &layout_b.block_definitions[field_b.definition as usize];
                if block_a.max_count != block_b.max_count {
                    println!(
                        "{pad}    block max_count: {} -> {}",
                        block_a.max_count, block_b.max_count
                    );
                }
                diff_struct(
                    layout_a,
                    block_a.struct_index as usize,
                    layout_b,
                    block_b.struct_index as usize,
                    indent + 2,
                )?;
            }
            (TagFieldType::Array, TagFieldType::Array) => {
                let array_a = &layout_a.array_definitions[field_a.definition as usize];
                let array_b = &layout_b.array_definitions[field_b.definition as usize];
                if array_a.count != array_b.count {
                    println!(
                        "{pad}    array count: {} -> {}",
                        array_a.count, array_b.count
                    );
                }
                diff_struct(
                    layout_a,
                    array_a.struct_index as usize,
                    layout_b,
                    array_b.struct_index as usize,
                    indent + 2,
                )?;
            }
            _ => {}
        }
    }

    Ok(())
}

/// Collect the indices (into `layout.fields`) of all fields belonging
/// to `struct_definition`, up to but excluding the terminator.
fn collect_fields(layout: &TagLayout, struct_definition: &TagStructDefinition) -> Vec<usize> {
    let mut result = Vec::new();
    let mut field_index = struct_definition.first_field_index as usize;
    loop {
        let field = &layout.fields[field_index];
        if field.field_type == TagFieldType::Terminator {
            break;
        }
        result.push(field_index);
        field_index += 1;
    }
    result
}

fn field_type_name<'a>(layout: &'a TagLayout, field: &TagFieldDefinition) -> &'a str {
    layout
        .get_string(layout.field_types[field.type_index as usize].name_offset)
        .unwrap_or("?")
}

fn fields_equal(
    layout_a: &TagLayout,
    fields_a: &[usize],
    layout_b: &TagLayout,
    fields_b: &[usize],
) -> bool {
    if fields_a.len() != fields_b.len() {
        return false;
    }
    for (&ia, &ib) in fields_a.iter().zip(fields_b.iter()) {
        let fa = &layout_a.fields[ia];
        let fb = &layout_b.fields[ib];
        let na = layout_a.get_string(fa.name_offset).unwrap_or("");
        let nb = layout_b.get_string(fb.name_offset).unwrap_or("");
        if na != nb || fa.field_type != fb.field_type || fa.offset != fb.offset {
            return false;
        }
    }
    true
}
