//! Survey every `bitm` tag in a monolithic / MCC cache by reading the
//! `format` enum on its `bitmaps[]` (or `xenon bitmaps[]`) elements.
//! Prints a histogram so we know which `BitmapFormat` variants matter
//! for the corpus in question.

use std::collections::BTreeMap;
use std::error::Error;

use blam_tags::monolithic::MonolithicCache;

const BITM: u32 = u32::from_be_bytes(*b"bitm");

fn main() -> Result<(), Box<dyn Error>> {
    let cache_dir = std::env::args().nth(1).ok_or("usage: <cache_dir>")?;
    let cache = MonolithicCache::open(&cache_dir)?;

    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    let mut total_images: u32 = 0;
    let mut tags_seen: u32 = 0;
    let mut tags_failed: u32 = 0;

    for entry in cache.iter_tags() {
        if entry.group_tag != BITM {
            continue;
        }
        tags_seen += 1;
        let tag = match cache.read_tag(entry) {
            Ok(t) => t,
            Err(_) => {
                tags_failed += 1;
                continue;
            }
        };
        let root = tag.root();
        for &block_name in &["bitmaps", "xenon bitmaps"] {
            let Some(field) = root.field_path(block_name) else { continue };
            let Some(block) = field.as_block() else { continue };
            for elem in block.iter() {
                let name = elem
                    .read_enum_name("format")
                    .unwrap_or_else(|| "<missing>".into());
                *counts.entry(name).or_default() += 1;
                total_images += 1;
            }
        }
    }

    println!("bitm tags: {tags_seen} ({tags_failed} failed to parse)");
    println!("total images: {total_images}");
    println!("\nformat histogram:");
    for (name, n) in &counts {
        println!("  {:>7}  {}", n, name);
    }
    Ok(())
}
