//! `set` — write a field's value from a string. The central mutation
//! point for scalar edits; `flag` and `block` are specialized forms
//! of the same pattern. Two-pass design: read the previous value and
//! validate the parse on the immutable handle first, then only enter
//! the mutable path for real runs. Emits `(was X)` so diffs are
//! self-describing.

use std::path::Path;

use anyhow::{Context, Result};
use blam_tags::TagSetError;

use crate::context::CliContext;

pub fn run(
    ctx: &mut CliContext,
    path: &str,
    value: &str,
    output: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    let resolved = ctx.resolve_path(path);

    // Read the previous value for the "(was …)" context. Captures
    // via the immutable handle so dry-run stays touch-free.
    let previous = {
        let loaded = ctx.loaded("set")?;
        let field = loaded
            .tag
            .root()
            .field_path(&resolved)
            .with_context(|| format!("field '{}' not found", resolved))?;
        field.value().map(|v| format!("{}", v))
    };
    let was_phrase = previous
        .map(|p| format!(" (was {p})"))
        .unwrap_or_default();
    let core = format!("set {resolved} = {value}{was_phrase}");

    if dry_run {
        // Parse-only through the immutable handle so the in-memory
        // tag isn't touched. Validation errors surface the same way
        // as a real set.
        let loaded = ctx.loaded("set")?;
        loaded.tag
            .root()
            .field_path(&resolved)
            .with_context(|| format!("field '{}' not found", resolved))?
            .parse(value)
            .map_err(set_error_to_anyhow)?;
        println!("(dry run) would {core}");
        return Ok(());
    }

    let loaded = ctx.loaded_mut("set")?;

    loaded.tag
        .root_mut()
        .field_path_mut(&resolved)
        .with_context(|| format!("field '{}' not found", resolved))?
        .parse_and_set(value)
        .map_err(set_error_to_anyhow)?;
    loaded.dirty = true;

    let commit = loaded.commit(output.map(Path::new))?;
    println!("{core}");
    if commit.redirected {
        println!("saved to {}", commit.target.display());
    }

    Ok(())
}

fn set_error_to_anyhow(e: TagSetError) -> anyhow::Error {
    match e {
        TagSetError::ParseError(msg) => anyhow::anyhow!("{msg}"),
        TagSetError::NotAssignable => anyhow::anyhow!("cannot set container field types directly"),
        TagSetError::TypeMismatch { expected, got } => {
            anyhow::anyhow!("type mismatch: expected {expected}, got {got}")
        }
    }
}
