//! Walk every render-geometry-carrying tag in a monolithic cache,
//! call `MonolithicCache::read_tag` (which triggers vertex hydration),
//! and report which tags fail / which vertex formats panic.
//!
//! Each tag's hydration runs in `catch_unwind` so one bad mesh
//! doesn't kill the sweep.

use std::collections::BTreeMap;
use std::error::Error;
use std::panic::AssertUnwindSafe;

use blam_tags::monolithic::MonolithicCache;

const GROUPS: &[&[u8; 4]] =
    &[b"mode", b"sbsp", b"pmdf", b"impo", b"iimz", b"Lbsp", b"rmla"];

fn main() -> Result<(), Box<dyn Error>> {
    let cache_dir = std::env::args().nth(1).ok_or("usage: <cache_dir>")?;
    let cache = MonolithicCache::open(&cache_dir)?;

    let mut by_group: BTreeMap<String, (u64, u64, u64)> = BTreeMap::new(); // (ok, panic, parse_err)
    let mut panics: BTreeMap<String, u64> = BTreeMap::new();
    let mut tags_seen: u64 = 0;

    for entry in cache.iter_tags() {
        let group_bytes = entry.group_tag.to_be_bytes();
        if !GROUPS.iter().any(|g| **g == group_bytes) {
            continue;
        }
        tags_seen += 1;
        let group_label = std::str::from_utf8(&group_bytes)
            .unwrap_or("?")
            .trim_end_matches(['\0', ' '])
            .to_string();
        let counters = by_group.entry(group_label.clone()).or_default();

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| cache.read_tag(entry)));
        match result {
            Ok(Ok(_)) => counters.0 += 1,
            Ok(Err(_)) => counters.2 += 1,
            Err(panic) => {
                counters.1 += 1;
                let msg = if let Some(s) = panic.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = panic.downcast_ref::<&str>() {
                    s.to_string()
                } else {
                    "<unknown panic>".to_string()
                };
                // Normalize the panic message to a "signature" by
                // stripping the dynamic stride / count numbers.
                let sig = normalize(&msg);
                *panics.entry(sig).or_default() += 1;
            }
        }
    }

    println!("tags scanned: {tags_seen}\n");
    println!("by group:  group     ok     panic    parse_err");
    for (g, (ok, panic, perr)) in &by_group {
        println!("           {g:<8} {ok:>6}    {panic:>5}    {perr:>5}");
    }
    if !panics.is_empty() {
        println!("\npanic signatures:");
        let mut sorted: Vec<_> = panics.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        for (sig, n) in sorted {
            println!("  {n:>5}  {sig}");
        }
    }
    Ok(())
}

fn normalize(msg: &str) -> String {
    // Replace any sequence of digits with `#` so panic strings that
    // only differ in stride / counts collapse to one signature.
    let mut out = String::with_capacity(msg.len());
    let mut prev_digit = false;
    for c in msg.chars() {
        if c.is_ascii_digit() {
            if !prev_digit {
                out.push('#');
            }
            prev_digit = true;
        } else {
            out.push(c);
            prev_digit = false;
        }
    }
    out
}
