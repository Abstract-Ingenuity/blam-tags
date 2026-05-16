//! Probe a Halo 4 monolithic tag cache — open it, print a summary,
//! and parse a few sample tags end-to-end.
//!
//! Usage:
//! ```text
//! cargo run --example monolithic_probe -- <path/to/tag_cache>
//! ```

use std::error::Error;
use std::path::PathBuf;

use blam_tags::monolithic::MonolithicCache;

fn main() -> Result<(), Box<dyn Error>> {
    let path: PathBuf = std::env::args()
        .nth(1)
        .ok_or("usage: monolithic_probe <tag_cache_dir>")?
        .into();

    let cache = MonolithicCache::open(&path)?;

    print!("session guid: ");
    for b in cache.session_guid.iter() {
        print!("{:02x}", b);
    }
    println!();

    println!("entries: {}", cache.len());
    println!(
        "tags heap: {} entries, {} partitions",
        cache.tag_heap.entries.len(),
        cache.tag_heap.partitions.len(),
    );
    println!(
        "cash heap: {} entries, {} partitions",
        cache.cache_heap.entries.len(),
        cache.cache_heap.partitions.len(),
    );

    println!("\nfirst 5 tags:");
    for entry in cache.iter_tags().take(5) {
        let group_bytes = entry.group_tag.to_be_bytes();
        let group_full = String::from_utf8_lossy(&group_bytes);
        let group = group_full.trim_end_matches(['\0', ' ']);
        let tag_block = cache.resolve_tag_block(entry);
        println!(
            "  {}:{} -> tag={:?}",
            group, entry.name, tag_block,
        );
    }

    // Sweep the entire corpus.
    println!("\nfull-corpus parse sweep ({} entries)...", cache.len());
    let mut ok = 0;
    let mut fail = 0;
    let mut no_tag_block = 0;
    let mut fail_examples: Vec<(String, String)> = Vec::new();
    let mut fail_by_kind: std::collections::BTreeMap<String, u32> =
        std::collections::BTreeMap::new();

    for entry in cache.iter_tags() {
        if cache.resolve_tag_block(entry).is_none() {
            no_tag_block += 1;
            continue;
        }
        let group_bytes = entry.group_tag.to_be_bytes();
        let group_full = String::from_utf8_lossy(&group_bytes);
        let group = group_full.trim_end_matches(['\0', ' ']).to_string();
        match cache.read_tag(entry) {
            Ok(_) => ok += 1,
            Err(e) => {
                fail += 1;
                let kind = format!("{}", error_kind(&e));
                *fail_by_kind.entry(kind).or_default() += 1;
                if fail_examples.len() < 8 {
                    fail_examples.push((
                        format!("{group}:{}", entry.name),
                        format!("{e:?}"),
                    ));
                }
            }
        }
    }

    println!("  ok:     {ok}");
    println!("  fail:   {fail}");
    println!("  no-tag-block (cache-only): {no_tag_block}");
    println!("\nfailure kinds:");
    for (k, v) in &fail_by_kind {
        println!("  {v:>7} {k}");
    }
    if !fail_examples.is_empty() {
        println!("\nfirst few failures:");
        for (tag, err) in &fail_examples {
            println!("  {tag} -> {err}");
        }
    }

    Ok(())
}

fn error_kind(e: &blam_tags::TagReadError) -> &'static str {
    use blam_tags::TagReadError as E;
    match e {
        E::Io(_) => "Io",
        E::BadChunkSignature { .. } => "BadChunkSignature",
        E::BadChunkVersion { .. } => "BadChunkVersion",
        E::CountMismatch { .. } => "CountMismatch",
        E::ChunkSizeMismatch { .. } => "ChunkSizeMismatch",
        E::UnknownSubChunkSignature { .. } => "UnknownSubChunkSignature",
        E::DuplicateOptionalStream { .. } => "DuplicateOptionalStream",
        E::UnsupportedLayoutVersion(_) => "UnsupportedLayoutVersion",
        E::UnsupportedBlockLayoutVersion(_) => "UnsupportedBlockLayoutVersion",
        E::UnsupportedFieldType { .. } => "UnsupportedFieldType",
        E::MissingSubChunk { .. } => "MissingSubChunk",
        E::InvalidUtf8 { .. } => "InvalidUtf8",
        E::StringOffsetOutOfBounds { .. } => "StringOffsetOutOfBounds",
        E::UnexpectedEof { .. } => "UnexpectedEof",
        _ => "Other",
    }
}
