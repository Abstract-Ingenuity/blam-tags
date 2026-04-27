//! Optional-stream management: `add-dependency-list`,
//! `remove-dependency-list`, `rebuild-dependency-list`,
//! `add-import-info`, `remove-import-info`,
//! `add-asset-depot-storage`, `remove-asset-depot-storage`.
//!
//! Load-modify-save mirroring `set` / `flag`. Stream schemas live at
//! `definitions/<game>/{tag_dependency_list,tag_import_information,
//! asset_depot_storage}.json`.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

use crate::context::CliContext;

fn schema_path(game: &str, stem: &str) -> PathBuf {
    PathBuf::from("definitions").join(game).join(format!("{stem}.json"))
}

fn require_schema(game: &str, stem: &str, kind: &str) -> Result<PathBuf> {
    let path = schema_path(game, stem);
    if !path.exists() {
        return Err(anyhow!(
            "{kind} schema not found at {} (check that definitions/{}/ is populated)",
            path.display(),
            game,
        ));
    }
    Ok(path)
}

/// Attach an empty `want` (dependency-list) stream to the loaded
/// tag. No-op if one is already present.
pub fn add_dependency_list(ctx: &mut CliContext, output: Option<&str>) -> Result<()> {
    let schema = require_schema(&ctx.game, "tag_dependency_list", "dependency-list")?;
    let loaded = ctx.loaded_mut("add-dependency-list")?;
    loaded.tag
        .add_dependency_list(&schema)
        .map_err(|e| anyhow!("add-dependency-list failed: {e}"))?;
    loaded.dirty = true;
    let commit = loaded.commit(output.map(Path::new))?;
    println!("attached empty dependency-list stream");
    if commit.redirected {
        println!("saved to {}", commit.target.display());
    }
    Ok(())
}

/// Drop the `want` stream from the loaded tag if present.
pub fn remove_dependency_list(ctx: &mut CliContext, output: Option<&str>) -> Result<()> {
    let loaded = ctx.loaded_mut("remove-dependency-list")?;
    loaded.tag.remove_dependency_list();
    loaded.dirty = true;
    let commit = loaded.commit(output.map(Path::new))?;
    println!("removed dependency-list stream");
    if commit.redirected {
        println!("saved to {}", commit.target.display());
    }
    Ok(())
}

/// Walk the tag's data, collect every non-null non-`impo`
/// `tag_reference`, and write one entry per ref into `want`.
/// Creates the stream first if missing.
pub fn rebuild_dependency_list(ctx: &mut CliContext, output: Option<&str>) -> Result<()> {
    let schema = require_schema(&ctx.game, "tag_dependency_list", "dependency-list")?;
    let loaded = ctx.loaded_mut("rebuild-dependency-list")?;
    loaded.tag
        .rebuild_dependency_list(&schema)
        .map_err(|e| anyhow!("rebuild-dependency-list failed: {e}"))?;
    loaded.dirty = true;
    let count = loaded
        .tag
        .dependency_list()
        .and_then(|r| r.field_path("dependencies"))
        .and_then(|f| f.as_block().map(|b| b.len()))
        .unwrap_or(0);
    let commit = loaded.commit(output.map(Path::new))?;
    println!("rebuilt dependency-list ({} entries)", count);
    if commit.redirected {
        println!("saved to {}", commit.target.display());
    }
    Ok(())
}

/// Attach an empty `info` (import-info) stream. No-op if one is
/// already present. Caller populates build / version / culprit /
/// import-date / files / events fields via `set` afterward.
pub fn add_import_info(ctx: &mut CliContext, output: Option<&str>) -> Result<()> {
    let schema = require_schema(&ctx.game, "tag_import_information", "import-info")?;
    let loaded = ctx.loaded_mut("add-import-info")?;
    loaded.tag
        .add_import_info(&schema)
        .map_err(|e| anyhow!("add-import-info failed: {e}"))?;
    loaded.dirty = true;
    let commit = loaded.commit(output.map(Path::new))?;
    println!("attached empty import-info stream");
    if commit.redirected {
        println!("saved to {}", commit.target.display());
    }
    Ok(())
}

/// Drop the `info` stream from the loaded tag if present.
pub fn remove_import_info(ctx: &mut CliContext, output: Option<&str>) -> Result<()> {
    let loaded = ctx.loaded_mut("remove-import-info")?;
    loaded.tag.remove_import_info();
    loaded.dirty = true;
    let commit = loaded.commit(output.map(Path::new))?;
    println!("removed import-info stream");
    if commit.redirected {
        println!("saved to {}", commit.target.display());
    }
    Ok(())
}

/// Attach an empty `assd` (asset-depot-storage / tag-editor icon)
/// stream. Zero presence in the observed H3/Reach corpus, kept for
/// completeness.
pub fn add_asset_depot_storage(ctx: &mut CliContext, output: Option<&str>) -> Result<()> {
    let schema = require_schema(&ctx.game, "asset_depot_storage", "asset-depot-storage")?;
    let loaded = ctx.loaded_mut("add-asset-depot-storage")?;
    loaded.tag
        .add_asset_depot_storage(&schema)
        .map_err(|e| anyhow!("add-asset-depot-storage failed: {e}"))?;
    loaded.dirty = true;
    let commit = loaded.commit(output.map(Path::new))?;
    println!("attached empty asset-depot-storage stream");
    if commit.redirected {
        println!("saved to {}", commit.target.display());
    }
    Ok(())
}

/// Drop the `assd` stream from the loaded tag if present.
pub fn remove_asset_depot_storage(ctx: &mut CliContext, output: Option<&str>) -> Result<()> {
    let loaded = ctx.loaded_mut("remove-asset-depot-storage")?;
    loaded.tag.remove_asset_depot_storage();
    loaded.dirty = true;
    let commit = loaded.commit(output.map(Path::new))?;
    println!("removed asset-depot-storage stream");
    if commit.redirected {
        println!("saved to {}", commit.target.display());
    }
    Ok(())
}
