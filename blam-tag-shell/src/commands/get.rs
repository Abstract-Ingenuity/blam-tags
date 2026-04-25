//! `get` — read a single field's value. The narrow complement to
//! `inspect`: no traversal, just the one leaf. `--raw` strips the
//! label for shell use (`x=$(blam-tag-shell get … --raw)`), `--json`
//! emits a typed record, and `--hex` formats integers as hex.

use anyhow::{Context, Result};
use serde_json::json;

use crate::context::CliContext;
use crate::format::{format_value, value_to_json};
use crate::suggest::suggest_field_name;

pub fn run(ctx: &mut CliContext, path: &str, raw_mode: bool, json_output: bool, hex_mode: bool) -> Result<()> {
    let resolved = ctx.resolve_path(path);
    let loaded = ctx.loaded("get")?;
    let root = loaded.tag.root();

    let field = root.field_path(&resolved).ok_or_else(|| match suggest_field_name(&root, &resolved) {
        Some(s) => anyhow::anyhow!("field '{}' not found. Did you mean '{}'?", resolved, s),
        None => anyhow::anyhow!("field '{}' not found", resolved),
    })?;

    let type_name = field.type_name();

    // Containers have no parsed value — report a summary.
    if let Some(summary) = container_summary(&field) {
        if json_output {
            let v = json!({ "path": &resolved, "type": type_name, "summary": summary });
            println!("{}", serde_json::to_string_pretty(&v)?);
        } else if raw_mode {
            println!("{summary}");
        } else {
            println!("{resolved}: {type_name} = {summary}");
        }
        return Ok(());
    }

    let value = field.value().context("field has no parsed value")?;

    if json_output {
        let out = json!({ "path": &resolved, "type": type_name, "value": value_to_json(ctx, &value) });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    let formatted = format_value(ctx, &value, hex_mode);
    if raw_mode {
        println!("{formatted}");
    } else {
        println!("{resolved}: {type_name} = {formatted}");
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
