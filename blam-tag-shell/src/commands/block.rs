use anyhow::{Context, Result};
use blam_tags::{TagFile, TagIndexError::OutOfRange};

pub fn run(
    file: &str,
    path: &str,
    action: &str,
    index: Option<usize>,
    output: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    // `count` is read-only.
    if action == "count" {
        let tag = TagFile::read(file).map_err(|e| anyhow::anyhow!("failed to load tag file: {e}"))?;
        let block = tag
            .root()
            .field_path(path)
            .with_context(|| format!("field '{}' not found", path))?
            .as_block()
            .with_context(|| format!("'{}' is not a block", path))?;
        println!("{}", block.len());
        return Ok(());
    }

    let mut tag = TagFile::read(file).map_err(|e| anyhow::anyhow!("failed to load tag file: {e}"))?;

    let description = {
        let mut root = tag.root_mut();
        let mut field = root
            .field_path_mut(path)
            .with_context(|| format!("field '{}' not found", path))?;

        let mut block = field
            .as_block_mut()
            .with_context(|| format!("'{}' is not a block", path))?;

        match action {
            "add" => {
                if dry_run {
                    format!(
                        "(dry run) would add element to {} (currently {} elements)",
                        path, block.len(),
                    )
                } else {
                    let i = block.add();
                    format!("added element [{}] to {}", i, path)
                }
            }
            "insert" => {
                let idx = index.context("insert requires an index argument")?;
                if dry_run {
                    format!("(dry run) would insert element at {}[{}]", path, idx)
                } else {
                    block.insert(idx).map_err(|OutOfRange { index, len }| anyhow::anyhow!("index {} out of range (block has {} elements)", index, len))?;
                    format!("inserted element at {}[{}]", path, idx)
                }
            }
            "duplicate" => {
                let idx = index.context("duplicate requires an index argument")?;
                if dry_run {
                    format!("(dry run) would duplicate {}[{}]", path, idx)
                } else {
                    let new_idx = block.duplicate(idx).map_err(|OutOfRange { index, len }| anyhow::anyhow!("index {} out of range (block has {} elements)", index, len))?;
                    format!("duplicated {}[{}] -> [{}]", path, idx, new_idx)
                }
            }
            "delete" => {
                let idx = index.context("delete requires an index argument")?;
                if dry_run {
                    format!("(dry run) would delete {}[{}]", path, idx)
                } else {
                    block.delete(idx).map_err(|OutOfRange { index, len }| anyhow::anyhow!("index {} out of range (block has {} elements)", index, len))?;
                    format!("deleted {}[{}]", path, idx)
                }
            }
            "clear" => {
                let count = block.len();
                if dry_run {
                    format!("(dry run) would clear {} ({} elements)", path, count)
                } else {
                    block.clear();
                    format!("cleared {} ({} elements removed)", path, count)
                }
            }
            _ => anyhow::bail!(
                "unknown action '{}' (expected: count, add, insert, duplicate, delete, clear)",
                action,
            ),
        }
    };

    if dry_run {
        println!("{description}");
        return Ok(());
    }

    let out_path = output.unwrap_or(file);
    tag.write(out_path)
        .map_err(|e| anyhow::anyhow!("failed to save tag file: {e}"))?;
    println!("{description}");
    if out_path != file {
        println!("saved to {out_path}");
    }

    Ok(())
}
