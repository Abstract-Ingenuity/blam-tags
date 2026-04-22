use anyhow::{Context, Result};
use blam_tags::data::TagSubChunkContent;
use blam_tags::fields::TagFieldType;
use blam_tags::file::TagFile;
use blam_tags::path::{lookup, lookup_mut};

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
        let tag =
            TagFile::read(file).map_err(|e| anyhow::anyhow!("failed to load tag file: {e}"))?;
        let layout = &tag.tag_stream.layout.layout;
        let cursor = lookup(layout, &tag.tag_stream.data, path)
            .with_context(|| format!("field '{}' not found", path))?;
        if !matches!(layout.fields[cursor.field_index].field_type, TagFieldType::Block) {
            anyhow::bail!("'{}' is not a block", path);
        }
        let entry = cursor
            .struct_data
            .sub_chunks
            .iter()
            .find(|e| e.field_index == Some(cursor.field_index as u32))
            .context("block sub-chunk missing")?;
        let count = match &entry.content {
            TagSubChunkContent::Block(b) => b.elements.len(),
            _ => anyhow::bail!("'{}' sub-chunk is not a block", path),
        };
        println!("{count}");
        return Ok(());
    }

    let mut tag =
        TagFile::read(file).map_err(|e| anyhow::anyhow!("failed to load tag file: {e}"))?;

    let description = {
        let tag_stream = &mut tag.tag_stream;
        let layout = &tag_stream.layout.layout;

        let cursor = lookup_mut(layout, &mut tag_stream.data, path)
            .with_context(|| format!("field '{}' not found", path))?;

        if !matches!(layout.fields[cursor.field_index].field_type, TagFieldType::Block) {
            anyhow::bail!("'{}' is not a block", path);
        }

        let field_index = cursor.field_index;
        let entry = cursor
            .struct_data
            .sub_chunks
            .iter_mut()
            .find(|e| e.field_index == Some(field_index as u32))
            .context("block sub-chunk missing")?;

        let block = match &mut entry.content {
            TagSubChunkContent::Block(b) => b,
            _ => anyhow::bail!("'{}' sub-chunk is not a block", path),
        };

        match action {
            "add" => {
                if dry_run {
                    format!(
                        "(dry run) would add element to {} (currently {} elements)",
                        path,
                        block.elements.len(),
                    )
                } else {
                    block.add_element(layout);
                    format!("added element [{}] to {}", block.elements.len() - 1, path)
                }
            }
            "insert" => {
                let idx = index.context("insert requires an index argument")?;
                if idx > block.elements.len() {
                    anyhow::bail!(
                        "index {} out of range (block has {} elements)",
                        idx,
                        block.elements.len()
                    );
                }
                if dry_run {
                    format!("(dry run) would insert element at {}[{}]", path, idx)
                } else {
                    block.insert_at(layout, idx);
                    format!("inserted element at {}[{}]", path, idx)
                }
            }
            "duplicate" => {
                let idx = index.context("duplicate requires an index argument")?;
                if idx >= block.elements.len() {
                    anyhow::bail!(
                        "index {} out of range (block has {} elements)",
                        idx,
                        block.elements.len()
                    );
                }
                if dry_run {
                    format!("(dry run) would duplicate {}[{}]", path, idx)
                } else {
                    block.duplicate_at(layout, idx);
                    format!("duplicated {}[{}] -> [{}]", path, idx, idx + 1)
                }
            }
            "delete" => {
                let idx = index.context("delete requires an index argument")?;
                if idx >= block.elements.len() {
                    anyhow::bail!(
                        "index {} out of range (block has {} elements)",
                        idx,
                        block.elements.len()
                    );
                }
                if dry_run {
                    format!("(dry run) would delete {}[{}]", path, idx)
                } else {
                    block.delete_at(layout, idx);
                    format!("deleted {}[{}]", path, idx)
                }
            }
            "clear" => {
                let count = block.elements.len();
                if dry_run {
                    format!("(dry run) would clear {} ({} elements)", path, count)
                } else {
                    block.clear();
                    format!("cleared {} ({} elements removed)", path, count)
                }
            }
            _ => anyhow::bail!(
                "unknown action '{}' (expected: count, add, insert, duplicate, delete, clear)",
                action
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
