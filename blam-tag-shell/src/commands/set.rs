use anyhow::{Context, Result};
use blam_tags::{TagFile, TagSetError};

pub fn run(
    file: &str,
    path: &str,
    value: &str,
    output: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    let mut tag = TagFile::read(file).map_err(|e| anyhow::anyhow!("failed to load tag file: {e}"))?;

    tag.root_mut()
        .field_path_mut(path)
        .with_context(|| format!("field '{}' not found", path))?
        .parse_and_set(value)
        .map_err(|e| match e {
            TagSetError::ParseError(msg) => anyhow::anyhow!("{msg}"),
            TagSetError::NotAssignable => anyhow::anyhow!("cannot set container field types directly"),
            TagSetError::TypeMismatch { expected, got } => {
                anyhow::anyhow!("type mismatch: expected {expected}, got {got}")
            }
        })?;

    if dry_run {
        println!("(dry run) would set {path} = {value}");
        return Ok(());
    }

    let out_path = output.unwrap_or(file);
    tag.write(out_path)
        .map_err(|e| anyhow::anyhow!("failed to save tag file: {e}"))?;
    println!("set {path} = {value}");
    if out_path != file {
        println!("saved to {out_path}");
    }

    Ok(())
}
