use anyhow::{Context, Result};
use blam_tags::TagFile;
use serde_json::json;

use crate::format::{format_value, value_to_json};

pub fn run(file: &str, path: &str, raw_mode: bool, json_output: bool, hex_mode: bool) -> Result<()> {
    let tag = TagFile::read(file).map_err(|e| anyhow::anyhow!("failed to load tag file: {e}"))?;
    let root = tag.root();

    let field = root.field_path(path).ok_or_else(|| match root.suggest_field_name(path) {
        Some(s) => anyhow::anyhow!("field '{}' not found. Did you mean '{}'?", path, s),
        None => anyhow::anyhow!("field '{}' not found", path),
    })?;

    let type_name = field.type_name();

    // Containers have no parsed value — report a summary.
    if let Some(summary) = container_summary(&field) {
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

    let value = field.value().context("field has no parsed value")?;

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

fn container_summary(field: &blam_tags::TagField<'_>) -> Option<String> {
    if field.as_struct().is_some() {
        return Some("struct".into());
    }
    if let Some(block) = field.as_block() {
        let n = block.len();
        return Some(format!("block [{} element{}]", n, if n == 1 { "" } else { "s" }));
    }
    if let Some(array) = field.as_array() {
        return Some(format!("array [{} elements]", array.len()));
    }
    if field.as_resource().is_some() {
        return Some("pageable_resource".into());
    }
    None
}
