use anyhow::{Context, Result};
use blam_tags::data::ContainerKind;
use blam_tags::file::TagFile;
use blam_tags::path::lookup;
use serde_json::json;

use crate::format::{format_value, value_to_json};
use crate::resolve;

pub fn run(file: &str, path: &str, raw_mode: bool, json_output: bool, hex_mode: bool) -> Result<()> {
    let tag = TagFile::read(file).map_err(|e| anyhow::anyhow!("failed to load tag file: {e}"))?;
    let layout = &tag.tag_stream.layout.layout;
    let root = tag
        .tag_stream
        .data
        .elements
        .first()
        .context("tag has no root element")?;

    let cursor = lookup(layout, &tag.tag_stream.data, path).ok_or_else(|| {
        let names = resolve::available_field_names(layout, root);
        match resolve::suggest_field(path, &names) {
            Some(s) => anyhow::anyhow!("field '{}' not found. Did you mean '{}'?", path, s),
            None => anyhow::anyhow!("field '{}' not found", path),
        }
    })?;

    let field = &layout.fields[cursor.field_index];
    let type_name = layout
        .get_string(layout.field_types[field.type_index as usize].name_offset)
        .unwrap_or("?");

    // Containers: no parsed value — report a summary.
    if let Some(kind) = cursor.struct_data.container_kind(layout, cursor.field_index) {
        let summary = container_summary(kind);
        if json_output {
            let v = json!({ "path": path, "type": type_name, "summary": summary });
            println!("{}", serde_json::to_string_pretty(&v)?);
        } else if raw_mode {
            println!("{summary}");
        } else {
            println!("{path}: {type_name} = {summary}");
        }
        return Ok(());
    }

    let value = cursor.parse(layout).context("field has no parsed value")?;

    if json_output {
        let out = json!({ "path": path, "type": type_name, "value": value_to_json(&value) });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    let formatted = format_value(&value, hex_mode);
    if raw_mode {
        println!("{formatted}");
    } else {
        println!("{path}: {type_name} = {formatted}");
    }

    Ok(())
}

fn container_summary(kind: ContainerKind) -> String {
    match kind {
        ContainerKind::Struct => "struct".into(),
        ContainerKind::Block { count } => {
            format!("block [{} element{}]", count, if count == 1 { "" } else { "s" })
        }
        ContainerKind::Array { count } => format!("array [{} elements]", count),
        ContainerKind::PageableResource => "pageable_resource".into(),
    }
}
