use anyhow::{Context, Result};
use blam_tags::{TagFile, TagOptions};

pub fn run(file: &str, path: &str) -> Result<()> {
    let tag = TagFile::read(file).map_err(|e| anyhow::anyhow!("failed to load tag file: {e}"))?;
    let field = tag
        .root()
        .field_path(path)
        .with_context(|| format!("field '{}' not found", path))?;

    match field.options() {
        Some(TagOptions::Enum { names, current }) => {
            println!("Enum options for '{}':", path);
            for (i, name) in names.iter().enumerate() {
                let marker = if current == Some(i as i64) { " <-" } else { "" };
                println!("  {i}: {name}{marker}");
            }
        }
        Some(TagOptions::Flags(items)) => {
            println!("Flag options for '{}':", path);
            for item in items {
                let marker = if item.is_set { "[x]" } else { "[ ]" };
                println!("  {}: {marker} {}", item.bit, item.name);
            }
        }
        None => anyhow::bail!("field '{}' is not an enum or flags field", path),
    }

    Ok(())
}
