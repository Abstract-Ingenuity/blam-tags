//! Iterate every tag in a monolithic cache; print those whose name
//! contains a substring.

use std::error::Error;

use blam_tags::monolithic::MonolithicCache;

fn main() -> Result<(), Box<dyn Error>> {
    let cache_dir = std::env::args().nth(1).ok_or("usage: <cache_dir> [needle]")?;
    let needle = std::env::args().nth(2).unwrap_or_default();
    let cache = MonolithicCache::open(&cache_dir)?;

    let needle_lc = needle.to_lowercase();
    for entry in cache.iter_tags() {
        if entry.name.to_lowercase().contains(&needle_lc) {
            let g = entry.group_tag.to_be_bytes();
            let group = std::str::from_utf8(&g).unwrap_or("?").trim_end_matches(['\0', ' ']);
            println!("  [{group:<4}]  {}", entry.name);
        }
    }
    Ok(())
}
