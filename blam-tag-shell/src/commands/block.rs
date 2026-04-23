//! `block` — structural edits to a tag's block fields.
//!
//! The `count` action is read-only; every other action mutates.
//! Actions are a strongly-typed enum ([`BlockAction`]) so clap rejects
//! misspellings at parse time with a proper help message rather than
//! bubbling them through to a runtime error.
//!
//! Two-pass design: we bounds-check + build a human-readable preview
//! via the *immutable* handle first, then (for non-dry-run) acquire
//! the mutable handle and execute. This keeps `--dry-run` genuinely
//! touch-free and gives the real-run path a clean, already-validated
//! slate to work against.

use std::path::Path;

use anyhow::{Context, Result};
use blam_tags::TagIndexError::OutOfRange;
use clap::ValueEnum;

use crate::context::CliContext;

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum BlockAction {
    Count,
    Add,
    Insert,
    Duplicate,
    Delete,
    Clear,
    Swap,
    Move,
}

pub fn run(
    ctx: &mut CliContext,
    path: &str,
    action: BlockAction,
    index: Option<usize>,
    index2: Option<usize>,
    output: Option<&str>,
    dry_run: bool,
    json_output: bool,
) -> Result<()> {
    let resolved = ctx.resolve_path(path);

    // Validate + compute the preview message via the immutable
    // handle. This also bounds-checks indices before any mutation.
    let preview = {
        let loaded = ctx.loaded("block")?;
        let block = loaded
            .tag
            .root()
            .field_path(&resolved)
            .with_context(|| format!("field '{}' not found", resolved))?
            .as_block()
            .with_context(|| format!("field '{}' is not a block", resolved))?;
        let len = block.len();

        match action {
            BlockAction::Count => {
                if json_output {
                    println!("{}", serde_json::json!({ "path": &resolved, "count": len }));
                } else {
                    println!("{len}");
                }
                return Ok(());
            }
            BlockAction::Add => format!("add element at [{len}] to {resolved}"),
            BlockAction::Insert => {
                let idx = index.context("insert requires an index argument")?;
                if idx > len {
                    anyhow::bail!("index {} out of range (block has {} elements)", idx, len);
                }
                format!("insert element at {resolved}[{idx}]")
            }
            BlockAction::Duplicate => {
                let idx = index.context("duplicate requires an index argument")?;
                if idx >= len {
                    anyhow::bail!("index {} out of range (block has {} elements)", idx, len);
                }
                format!("duplicate {resolved}[{idx}] -> [{}]", idx + 1)
            }
            BlockAction::Delete => {
                let idx = index.context("delete requires an index argument")?;
                if idx >= len {
                    anyhow::bail!("index {} out of range (block has {} elements)", idx, len);
                }
                format!("delete {resolved}[{idx}]")
            }
            BlockAction::Clear => format!("clear {resolved} ({len} elements)"),
            BlockAction::Swap => {
                let i = index.context("swap requires two index arguments")?;
                let j = index2.context("swap requires two index arguments")?;
                if i >= len {
                    anyhow::bail!("index {} out of range (block has {} elements)", i, len);
                }
                if j >= len {
                    anyhow::bail!("index {} out of range (block has {} elements)", j, len);
                }
                format!("swap {resolved}[{i}] <-> [{j}]")
            }
            BlockAction::Move => {
                let from = index.context("move requires from and to indices")?;
                let to = index2.context("move requires from and to indices")?;
                if from >= len {
                    anyhow::bail!("index {} out of range (block has {} elements)", from, len);
                }
                if to >= len {
                    anyhow::bail!("index {} out of range (block has {} elements)", to, len);
                }
                format!("move {resolved}[{from}] -> [{to}]")
            }
        }
    };

    if dry_run {
        println!("(dry run) would {preview}");
        return Ok(());
    }

    // Real path: re-resolve mutably and execute. Bounds-checks in the
    // immutable pass above mean these `.map_err` arms are defensive
    // (would fire if the tag mutated between passes — impossible here
    // since we hold the single context).
    let loaded = ctx.loaded_mut("block")?;

    let out_of_range = |OutOfRange { index, len }| {
        anyhow::anyhow!("index {} out of range (block has {} elements)", index, len)
    };

    {
        let mut root = loaded.tag.root_mut();
        let mut field = root
            .field_path_mut(&resolved)
            .with_context(|| format!("field '{}' not found", resolved))?;
        let mut block = field
            .as_block_mut()
            .with_context(|| format!("'{}' is not a block", resolved))?;

        match action {
            BlockAction::Count => unreachable!("returned early above"),
            BlockAction::Add => { block.add(); }
            BlockAction::Insert => { block.insert(index.unwrap()).map_err(out_of_range)?; }
            BlockAction::Duplicate => { block.duplicate(index.unwrap()).map_err(out_of_range)?; }
            BlockAction::Delete => { block.delete(index.unwrap()).map_err(out_of_range)?; }
            BlockAction::Clear => { block.clear(); }
            BlockAction::Swap => { block.swap(index.unwrap(), index2.unwrap()).map_err(out_of_range)?; }
            BlockAction::Move => { block.move_to(index.unwrap(), index2.unwrap()).map_err(out_of_range)?; }
        }
    }
    loaded.dirty = true;

    let commit = loaded.commit(output.map(Path::new))?;
    println!("{preview}");
    if commit.redirected {
        println!("saved to {}", commit.target.display());
    }

    Ok(())
}
