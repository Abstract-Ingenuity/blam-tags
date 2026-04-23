use anyhow::{Context, Result};
use blam_tags::TagFile;

pub fn run(file: &str) -> Result<()> {
    let tag = TagFile::read(file).map_err(|e| anyhow::anyhow!("failed to load tag file: {e}"))?;
    let file_size = std::fs::metadata(file).context("failed to stat file")?.len();

    let group = tag.group();
    println!("Tag File");
    println!("  Group:         {}", group);
    println!("  Group version: {}", group.version);
    println!("  Build:         {}.{}", tag.header.build_version, tag.header.build_number);
    println!("  Version:       {}", tag.header.version);
    println!("  Checksum:      0x{:08X}", tag.header.checksum);
    println!("  File size:     {} bytes", file_size);

    // Stream list: tag! is mandatory; want/info are optional.
    let mut streams = vec!["tag!"];
    if tag.dependency_list().is_some() { streams.push("want"); }
    if tag.import_info().is_some() { streams.push("info"); }
    println!("  Streams:       {}", streams.join(", "));

    Ok(())
}
