use anyhow::{Context, Result};
use blam_tags::file::TagFile;

use crate::format::format_tag_group;

pub fn run(file: &str) -> Result<()> {
    let tag = TagFile::read(file).map_err(|e| anyhow::anyhow!("failed to load tag file: {e}"))?;
    let file_size = std::fs::metadata(file).context("failed to stat file")?.len();

    let header = &tag.header;
    println!("Tag File");
    println!("  Group:         {}", format_tag_group(header.group_tag));
    println!("  Group version: {}", header.group_version);
    println!("  Build:         {}.{}", header.build_version, header.build_number);
    println!("  Version:       {}", header.version);
    println!("  Checksum:      0x{:08X}", header.checksum);
    println!("  File size:     {} bytes", file_size);

    // Stream list: tag! is mandatory; want/info are optional.
    let mut streams = vec!["tag!"];
    if tag.dependency_list_stream.is_some() {
        streams.push("want");
    }
    if tag.import_info_stream.is_some() {
        streams.push("info");
    }
    println!("  Streams:       {}", streams.join(", "));

    Ok(())
}
