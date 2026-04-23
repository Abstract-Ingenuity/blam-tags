use anyhow::{Context, Result};
use blam_tags::{TagArray, TagBlock, TagField, TagFile, TagStruct};
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
    let root = tag.root();

    let Some(p) = path else {
        if json_output {
            println!("{}", serde_json::to_string_pretty(&struct_to_json(root, depth))?);
        } else {
            print_struct(root, depth, 0);
        }
        return Ok(());
    };

    let field = root.field_path(p).with_context(|| format!("field '{}' not found", p))?;

    if let Some(nested) = field.as_struct() {
        if json_output {
            println!("{}", serde_json::to_string_pretty(&struct_to_json(nested, depth))?);
        } else {
            print_struct(nested, depth, 0);
        }
    } else if let Some(block) = field.as_block() {
        print_block(block, p, depth, json_output)?;
    } else if let Some(array) = field.as_array() {
        print_array(array, p, depth, json_output)?;
    } else {
        anyhow::bail!("path '{}' does not resolve to a struct/block/array", p);
    }

    Ok(())
}

fn print_block(block: TagBlock<'_>, label: &str, depth: usize, json_output: bool) -> Result<()> {
    if json_output {
        let values: Vec<Value> = block.iter().map(|s| struct_to_json(s, depth)).collect();
        println!("{}", serde_json::to_string_pretty(&values)?);
    } else {
        println!("{}: block [{} elements]", label, block.len());
        for (i, s) in block.iter().enumerate() {
            println!("  [{}]", i);
            print_struct(s, depth, 2);
        }
    }
    Ok(())
}

fn print_array(array: TagArray<'_>, label: &str, depth: usize, json_output: bool) -> Result<()> {
    if json_output {
        let values: Vec<Value> = array.iter().map(|s| struct_to_json(s, depth)).collect();
        println!("{}", serde_json::to_string_pretty(&values)?);
    } else {
        println!("{}: array [{} elements]", label, array.len());
        for (i, s) in array.iter().enumerate() {
            println!("  [{}]", i);
            print_struct(s, depth, 2);
        }
    }
    Ok(())
}

fn print_struct(s: TagStruct<'_>, depth: usize, indent: usize) {
    let prefix = " ".repeat(indent);
    for field in s.fields() {
        let name = field.name();

        if let Some(nested) = field.as_struct() {
            println!("{prefix}{name}: struct");
            if depth > 0 {
                print_struct(nested, depth - 1, indent + 2);
            }
        } else if let Some(block) = field.as_block() {
            println!("{prefix}{name}: block [{} elements]", block.len());
            if depth > 0 {
                for (i, elem) in block.iter().enumerate() {
                    println!("{prefix}  [{i}]");
                    print_struct(elem, depth - 1, indent + 4);
                }
            }
        } else if let Some(array) = field.as_array() {
            println!("{prefix}{name}: array [{} elements]", array.len());
            if depth > 0 {
                for (i, elem) in array.iter().enumerate() {
                    println!("{prefix}  [{i}]");
                    print_struct(elem, depth - 1, indent + 4);
                }
            }
        } else if field.as_resource().is_some() {
            println!("{prefix}{name}: pageable_resource");
        } else {
            let type_name = field.type_name();
            match field.value() {
                Some(value) => {
                    println!("{prefix}{name}: {type_name} = {}", format_value(&value, false))
                }
                None => println!("{prefix}{name}: {type_name}"),
            }
        }
    }
}

fn struct_to_json(s: TagStruct<'_>, depth: usize) -> Value {
    let fields: Vec<Value> = s.fields().map(|field| field_to_json(field, depth)).collect();
    json!(fields)
}

fn field_to_json(field: TagField<'_>, depth: usize) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("name".into(), json!(field.name()));
    obj.insert("type".into(), json!(field.type_name()));

    if let Some(nested) = field.as_struct() {
        if depth > 0 {
            obj.insert("fields".into(), struct_to_json(nested, depth - 1));
        }
    } else if let Some(block) = field.as_block() {
        obj.insert("count".into(), json!(block.len()));
        if depth > 0 {
            let elements: Vec<Value> = block.iter().map(|s| struct_to_json(s, depth - 1)).collect();
            obj.insert("elements".into(), json!(elements));
        }
    } else if let Some(array) = field.as_array() {
        obj.insert("count".into(), json!(array.len()));
        if depth > 0 {
            let elements: Vec<Value> = array.iter().map(|s| struct_to_json(s, depth - 1)).collect();
            obj.insert("elements".into(), json!(elements));
        }
    } else if field.as_resource().is_some() {
        obj.insert("kind".into(), json!("pageable_resource"));
    } else if let Some(value) = field.value() {
        obj.insert("value".into(), value_to_json(&value));
    }

    Value::Object(obj)
}
