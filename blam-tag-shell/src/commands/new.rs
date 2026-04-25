//! `new <group>` — create a fresh tag from a schema JSON.
//!
//! Resolves the schema at `definitions/<game>/<group>.json` (game
//! comes from the global `--game` flag via [`CliContext::game`]) and
//! calls `TagFile::new`. Writes to `./<group>.<group>` in the cwd. No
//! optional streams attached by default — use `add-want` / `add-info`
//! afterward if you want them.

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use blam_tags::TagFile;

use crate::context::CliContext;

pub fn run(ctx: &CliContext, group: &str, output: Option<&str>) -> Result<()> {
    let schema = PathBuf::from("definitions").join(&ctx.game).join(format!("{group}.json"));
    if !schema.exists() {
        return Err(anyhow!(
            "schema not found: {} (is the group name right and `definitions/{}/` present?)",
            schema.display(),
            ctx.game,
        ));
    }

    let out: PathBuf = output
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("{group}.{group}")));
    if out.exists() {
        return Err(anyhow!("refusing to overwrite existing file: {}", out.display()));
    }

    let tag = TagFile::new(&schema)
        .map_err(|e| anyhow!("failed to build tag from {}: {e}", schema.display()))?;
    tag.write(&out)
        .with_context(|| format!("failed to write {}", out.display()))?;

    println!("created {} from {}", out.display(), schema.display());
    Ok(())
}
