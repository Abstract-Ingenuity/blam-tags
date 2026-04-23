//! `options` — enumerate the declared variants of an enum field, or
//! the declared bit names of a flags field. Lets users discover what
//! they can pass to `set` or `flag` without having to cross-reference
//! the schema out-of-band.

use anyhow::{Context, Result};
use blam_tags::TagOptions;
use serde_json::json;

use crate::context::CliContext;

pub fn run(ctx: &mut CliContext, path: &str, json_output: bool) -> Result<()> {
    let resolved = ctx.resolve_path(path);
    let loaded = ctx.loaded("options")?;
    let field = loaded
        .tag
        .root()
        .field_path(&resolved)
        .with_context(|| format!("field '{}' not found", resolved))?;

    match field.options() {
        Some(TagOptions::Enum { names, current }) => {
            if json_output {
                let out = json!({
                    "path": &resolved,
                    "kind": "enum",
                    "current": current,
                    "options": names,
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("Enum options for '{}':", resolved);
                for (i, name) in names.iter().enumerate() {
                    let marker = if current == Some(i as i64) { " <-" } else { "" };
                    println!("  {i}: {name}{marker}");
                }
            }
        }
        Some(TagOptions::Flags(items)) => {
            if json_output {
                let options: Vec<_> = items.iter()
                    .map(|o| json!({ "bit": o.bit, "name": o.name, "set": o.is_set }))
                    .collect();
                let out = json!({
                    "path": &resolved,
                    "kind": "flags",
                    "options": options,
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("Flag options for '{}':", resolved);
                for item in items {
                    let marker = if item.is_set { "[x]" } else { "[ ]" };
                    println!("  {}: {marker} {}", item.bit, item.name);
                }
            }
        }
        None => anyhow::bail!("field '{}' is not an enum or flags field", resolved),
    }

    Ok(())
}
