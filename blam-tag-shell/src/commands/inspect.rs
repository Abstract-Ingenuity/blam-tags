use anyhow::{Context, Result};
use blam_tags::data::{TagBlockData, TagStruct, TagSubChunkContent};
use blam_tags::fields::TagFieldType;
use blam_tags::file::TagFile;
use blam_tags::layout::TagLayout;
use blam_tags::path::lookup;
use serde_json::{json, Value};

use crate::format::{format_value, value_to_json};

pub fn run(
    file: &str,
    path: Option<&str>,
    depth: usize,
    _show_all: bool,
    json_output: bool,
) -> Result<()> {
    let tag = TagFile::read(file).map_err(|e| anyhow::anyhow!("failed to load tag file: {e}"))?;
    let layout = &tag.tag_stream.layout.layout;
    let root_block = &tag.tag_stream.data;
    let root_element = root_block
        .elements
        .first()
        .context("tag has no root element")?;
    let root_raw = root_block.element_raw(layout, 0);

    if let Some(p) = path {
        let cursor = lookup(layout, root_block, p)
            .with_context(|| format!("field '{}' not found", p))?;
        let field = &layout.fields[cursor.field_index];
        let entry = cursor
            .struct_data
            .sub_chunks
            .iter()
            .find(|e| e.field_index == Some(cursor.field_index as u32));

        match (&field.field_type, entry.map(|e| &e.content)) {
            (TagFieldType::Struct, Some(_)) => {
                let (nested, nested_raw) = cursor
                    .struct_data
                    .nested_struct(layout, cursor.struct_raw, cursor.field_index)
                    .context("struct sub-chunk missing")?;
                if json_output {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&element_to_json(
                            layout, nested, nested_raw, depth
                        ))?
                    );
                } else {
                    print_element(layout, nested, nested_raw, depth, 0);
                }
            }
            (TagFieldType::Block, Some(TagSubChunkContent::Block(block))) => {
                print_block(layout, block, p, depth, json_output)?;
            }
            (TagFieldType::Array, Some(TagSubChunkContent::Array(elements))) => {
                if json_output {
                    let values: Vec<Value> = (0..elements.len())
                        .filter_map(|i| {
                            cursor
                                .struct_data
                                .array_element(layout, cursor.struct_raw, cursor.field_index, i)
                                .map(|(elem, raw)| element_to_json(layout, elem, raw, depth))
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&values)?);
                } else {
                    println!("{}: array [{} elements]", p, elements.len());
                    for i in 0..elements.len() {
                        if let Some((elem, raw)) = cursor.struct_data.array_element(
                            layout,
                            cursor.struct_raw,
                            cursor.field_index,
                            i,
                        ) {
                            println!("  [{}]", i);
                            print_element(layout, elem, raw, depth, 2);
                        }
                    }
                }
            }
            _ => anyhow::bail!("path '{}' does not resolve to a struct/block/array", p),
        }
        return Ok(());
    }

    // No path — dump the root element.
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&element_to_json(layout, root_element, root_raw, depth))?
        );
    } else {
        print_element(layout, root_element, root_raw, depth, 0);
    }

    Ok(())
}

fn print_block(
    layout: &TagLayout,
    block: &TagBlockData,
    label: &str,
    depth: usize,
    json_output: bool,
) -> Result<()> {
    if json_output {
        let values: Vec<Value> = block
            .iter_elements(layout)
            .map(|(raw, element)| element_to_json(layout, element, raw, depth))
            .collect();
        println!("{}", serde_json::to_string_pretty(&values)?);
    } else {
        println!("{}: block [{} elements]", label, block.elements.len());
        for (i, (raw, element)) in block.iter_elements(layout).enumerate() {
            println!("  [{}]", i);
            print_element(layout, element, raw, depth, 2);
        }
    }
    Ok(())
}

fn print_element(
    layout: &TagLayout,
    element: &TagStruct,
    element_raw: &[u8],
    depth: usize,
    indent: usize,
) {
    let struct_definition = &layout.struct_definitions[element.struct_index as usize];
    let prefix = " ".repeat(indent);
    let mut field_index = struct_definition.first_field_index as usize;

    loop {
        let field = &layout.fields[field_index];
        if field.field_type == TagFieldType::Terminator {
            break;
        }

        let name = layout.get_string(field.name_offset).unwrap_or("?");
        let type_name = layout
            .get_string(layout.field_types[field.type_index as usize].name_offset)
            .unwrap_or("?");

        match field.field_type {
            TagFieldType::Pad
            | TagFieldType::UselessPad
            | TagFieldType::Skip
            | TagFieldType::Explanation
            | TagFieldType::Terminator
            | TagFieldType::Unknown => {
                field_index += 1;
                continue;
            }

            TagFieldType::Struct => {
                println!("{prefix}{name}: struct");
                if depth > 0 {
                    if let Some((nested, nested_raw)) =
                        element.nested_struct(layout, element_raw, field_index)
                    {
                        print_element(layout, nested, nested_raw, depth - 1, indent + 2);
                    }
                }
            }

            TagFieldType::Block => {
                if let Some(TagSubChunkContent::Block(block)) = element
                    .sub_chunks
                    .iter()
                    .find(|e| e.field_index == Some(field_index as u32))
                    .map(|e| &e.content)
                {
                    println!("{prefix}{name}: block [{} elements]", block.elements.len());
                    if depth > 0 {
                        for (i, (raw, nested)) in block.iter_elements(layout).enumerate() {
                            println!("{prefix}  [{i}]");
                            print_element(layout, nested, raw, depth - 1, indent + 4);
                        }
                    }
                } else {
                    println!("{prefix}{name}: block (missing)");
                }
            }

            TagFieldType::Array => {
                if let Some(TagSubChunkContent::Array(elements)) = element
                    .sub_chunks
                    .iter()
                    .find(|e| e.field_index == Some(field_index as u32))
                    .map(|e| &e.content)
                {
                    println!("{prefix}{name}: array [{} elements]", elements.len());
                    if depth > 0 {
                        for i in 0..elements.len() {
                            if let Some((nested, nested_raw)) =
                                element.array_element(layout, element_raw, field_index, i)
                            {
                                println!("{prefix}  [{i}]");
                                print_element(layout, nested, nested_raw, depth - 1, indent + 4);
                            }
                        }
                    }
                } else {
                    println!("{prefix}{name}: array (missing)");
                }
            }

            TagFieldType::PageableResource => {
                println!("{prefix}{name}: pageable_resource");
            }

            _ => match element.parse_field(layout, element_raw, field_index) {
                Some(value) => {
                    let formatted = format_value(&value, false);
                    println!("{prefix}{name}: {type_name} = {formatted}");
                }
                None => {
                    println!("{prefix}{name}: {type_name}");
                }
            },
        }

        field_index += 1;
    }
}

fn element_to_json(
    layout: &TagLayout,
    element: &TagStruct,
    element_raw: &[u8],
    depth: usize,
) -> Value {
    let struct_definition = &layout.struct_definitions[element.struct_index as usize];
    let mut fields_json = Vec::new();
    let mut field_index = struct_definition.first_field_index as usize;

    loop {
        let field = &layout.fields[field_index];
        if field.field_type == TagFieldType::Terminator {
            break;
        }
        if matches!(
            field.field_type,
            TagFieldType::Pad
                | TagFieldType::UselessPad
                | TagFieldType::Skip
                | TagFieldType::Explanation
                | TagFieldType::Unknown,
        ) {
            field_index += 1;
            continue;
        }

        let name = layout.get_string(field.name_offset).unwrap_or("?").to_string();
        let type_name = layout
            .get_string(layout.field_types[field.type_index as usize].name_offset)
            .unwrap_or("?")
            .to_string();

        let mut obj = serde_json::Map::new();
        obj.insert("name".into(), json!(name));
        obj.insert("type".into(), json!(type_name));

        match field.field_type {
            TagFieldType::Struct => {
                if depth > 0 {
                    if let Some((nested, nested_raw)) =
                        element.nested_struct(layout, element_raw, field_index)
                    {
                        obj.insert(
                            "fields".into(),
                            element_to_json(layout, nested, nested_raw, depth - 1),
                        );
                    }
                }
            }
            TagFieldType::Block => {
                if let Some(TagSubChunkContent::Block(block)) = element
                    .sub_chunks
                    .iter()
                    .find(|e| e.field_index == Some(field_index as u32))
                    .map(|e| &e.content)
                {
                    obj.insert("count".into(), json!(block.elements.len()));
                    if depth > 0 {
                        let elements: Vec<Value> = block
                            .iter_elements(layout)
                            .map(|(raw, e)| element_to_json(layout, e, raw, depth - 1))
                            .collect();
                        obj.insert("elements".into(), json!(elements));
                    }
                }
            }
            TagFieldType::Array => {
                if let Some(TagSubChunkContent::Array(elements)) = element
                    .sub_chunks
                    .iter()
                    .find(|e| e.field_index == Some(field_index as u32))
                    .map(|e| &e.content)
                {
                    obj.insert("count".into(), json!(elements.len()));
                    if depth > 0 {
                        let values: Vec<Value> = (0..elements.len())
                            .filter_map(|i| {
                                element
                                    .array_element(layout, element_raw, field_index, i)
                                    .map(|(e, raw)| element_to_json(layout, e, raw, depth - 1))
                            })
                            .collect();
                        obj.insert("elements".into(), json!(values));
                    }
                }
            }
            TagFieldType::PageableResource => {
                obj.insert("kind".into(), json!("pageable_resource"));
            }
            _ => {
                if let Some(value) = element.parse_field(layout, element_raw, field_index) {
                    obj.insert("value".into(), value_to_json(&value));
                }
            }
        }

        fields_json.push(Value::Object(obj));
        field_index += 1;
    }

    json!(fields_json)
}
