//! `flag` — read or mutate a single bit in a flags field by name.
//!
//! The `set <field> 0x…` path covers whole-mask edits; `flag` exists
//! because the common case is "toggle this one bit" and users
//! shouldn't have to look up its bit index. Two-pass design mirrors
//! [`set`](crate::commands::set): the preview + bit lookup happen on
//! the immutable handle so `--dry-run` is genuinely touch-free.

use std::path::Path;

use anyhow::{Context, Result};

use crate::context::CliContext;

pub fn run(
    ctx: &mut CliContext,
    path: &str,
    flag_name: &str,
    action: Option<&str>,
    output: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    let resolved = ctx.resolve_path(path);

    // Look the flag up via the immutable handle first: read state,
    // validate the action, compute the new value. Only then decide
    // whether to enter the mutable path.
    let (current, new_value) = {
        let loaded = ctx.loaded("flag")?;
        let field = loaded
            .tag
            .root()
            .field_path(&resolved)
            .with_context(|| format!("field '{}' not found", resolved))?;
        let flag = field
            .flag(flag_name)
            .with_context(|| format!("flag '{}' not found on field '{}'", flag_name, resolved))?;
        let current = flag.is_set();
        let new = match action {
            None => {
                // Read-only path — just print and return.
                println!("{resolved}.{flag_name} = {}", if current { "on" } else { "off" });
                return Ok(());
            }
            Some("on") => true,
            Some("off") => false,
            Some("toggle") => !current,
            Some(other) => anyhow::bail!("unknown action '{}' (expected on, off, toggle)", other),
        };
        (current, new)
    };

    let core = format!(
        "set {resolved}.{flag_name} = {} (was {})",
        if new_value { "on" } else { "off" },
        if current { "on" } else { "off" },
    );

    if dry_run {
        println!("(dry run) would {core}");
        return Ok(());
    }

    let loaded = ctx.loaded_mut("flag")?;
    loaded
        .tag
        .root_mut()
        .field_path_mut(&resolved)
        .with_context(|| format!("field '{}' not found", resolved))?
        .flag_mut(flag_name)
        .with_context(|| format!("flag '{}' not found on field '{}'", flag_name, resolved))?
        .set(new_value);
    loaded.dirty = true;

    let commit = loaded.commit(output.map(Path::new))?;
    println!("{core}");
    if commit.redirected {
        println!("saved to {}", commit.target.display());
    }

    Ok(())
}
