//! `extract-data` — write the bytes of a single `tag_data` field to
//! a file for inspection. Errors if the field path doesn't resolve to
//! a `tag_data` leaf.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::context::CliContext;
use crate::suggest::suggest_field_name;

pub fn run(ctx: &mut CliContext, path: &str, output: Option<&str>) -> Result<()> {
    let resolved = ctx.resolve_path(path);
    let loaded = ctx.loaded("extract-data")?;
    let root = loaded.tag.root();

    let field = root.field_path(&resolved).ok_or_else(|| match suggest_field_name(&root, &resolved) {
        Some(s) => anyhow::anyhow!("field '{}' not found. Did you mean '{}'?", resolved, s),
        None => anyhow::anyhow!("field '{}' not found", resolved),
    })?;

    let bytes = field.as_data().ok_or_else(|| {
        anyhow::anyhow!(
            "field '{}' is {} (not a `tag_data` field)",
            resolved,
            field.type_name(),
        )
    })?;

    let target = match output {
        Some(p) => PathBuf::from(p),
        None => default_output_path(&loaded.path, field.name()),
    };

    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
    }

    let file = File::create(&target)
        .with_context(|| format!("create {}", target.display()))?;
    let mut writer = BufWriter::new(file);
    writer.write_all(bytes)?;
    writer.flush()?;

    println!("{}: {} bytes", target.display(), bytes.len());
    Ok(())
}

fn default_output_path(tag_path: &std::path::Path, field_name: &str) -> PathBuf {
    let stem = tag_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("tag");
    let safe_field = sanitize(field_name);
    PathBuf::from(format!("{stem}.{safe_field}.bin"))
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}
