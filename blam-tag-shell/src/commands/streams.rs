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

fn want_schema(game: &str) -> PathBuf {
    PathBuf::from("definitions").join(game).join("tag_dependency_list.json")
}

fn info_schema(game: &str) -> PathBuf {
    PathBuf::from("definitions").join(game).join("tag_import_information.json")
}

fn assd_schema(game: &str) -> PathBuf {
    PathBuf::from("definitions").join(game).join("asset_depot_storage.json")
}

fn require_schema(path: PathBuf, game: &str, kind: &str) -> Result<PathBuf> {
    if !path.exists() {
        return Err(anyhow!(
            "{kind} schema not found at {} (check that definitions/{}/ is populated)",
            path.display(),
            game,
        ));
    }
    Ok(path)
}

pub fn add_dependency_list(ctx: &mut CliContext, game: &str, output: Option<&str>) -> Result<()> {
    let schema = require_schema(want_schema(game), game, "dependency-list")?;
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

pub fn rebuild_dependency_list(ctx: &mut CliContext, game: &str, output: Option<&str>) -> Result<()> {
    let schema = require_schema(want_schema(game), game, "dependency-list")?;
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

pub fn add_import_info(ctx: &mut CliContext, game: &str, output: Option<&str>) -> Result<()> {
    let schema = require_schema(info_schema(game), game, "import-info")?;
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

pub fn add_asset_depot_storage(
    ctx: &mut CliContext,
    game: &str,
    output: Option<&str>,
) -> Result<()> {
    let schema = require_schema(assd_schema(game), game, "asset-depot-storage")?;
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
