//! `header` — print the file-level metadata (group tag, version,
//! checksum, authoring-toolset build) without descending into the
//! tag body. Cheap sanity check: if `header` errors, the file isn't
//! a valid tag at all.

use anyhow::{Context, Result};
use blam_tags::format_group_tag;
use serde_json::json;

use crate::context::CliContext;

pub fn run(ctx: &mut CliContext, json_output: bool) -> Result<()> {
    let loaded = ctx.loaded("header")?;
    let file_size = std::fs::metadata(&loaded.path).context("failed to stat file")?.len();
    let tag = &loaded.tag;
    let group = tag.group();
    let group_str = format_group_tag(group.tag);

    let mut streams = vec!["tag!"];
    if tag.dependency_list().is_some() { streams.push("want"); }
    if tag.import_info().is_some() { streams.push("info"); }
    if tag.asset_depot_storage().is_some() { streams.push("assd"); }

    if json_output {
        let out = json!({
            "group": group_str,
            "group_version": group.version,
            "build": { "version": tag.header.build_version, "number": tag.header.build_number },
            "version": tag.header.version,
            "checksum": format!("0x{:08X}", tag.header.checksum),
            "file_size": file_size,
            "streams": streams,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!("Tag File");
    println!("  Group:         {}", group_str);
    println!("  Group version: {}", group.version);
    println!("  Build:         {}.{}", tag.header.build_version, tag.header.build_number);
    println!("  Version:       {}", tag.header.version);
    println!("  Checksum:      0x{:08X}", tag.header.checksum);
    println!("  File size:     {} bytes", file_size);
    println!("  Streams:       {}", streams.join(", "));

    Ok(())
}
