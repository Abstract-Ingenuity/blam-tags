//! Sweep every `bitm` tag in a monolithic cache and try to extract
//! each image to in-memory DDS. Report ok/fail tallies by format and
//! the first few example failures per kind so we can chase down
//! the long tail.
//!
//! Usage: `cargo run --release --example bitmap_extract_sweep -- <cache_dir>`

use std::collections::BTreeMap;
use std::error::Error;

use blam_tags::monolithic::MonolithicCache;
use blam_tags::Bitmap;

const BITM: u32 = u32::from_be_bytes(*b"bitm");

fn main() -> Result<(), Box<dyn Error>> {
    let cache_dir = std::env::args().nth(1).ok_or("usage: <cache_dir>")?;
    let cache = MonolithicCache::open(&cache_dir)?;

    let mut ok_by_format: BTreeMap<String, u32> = BTreeMap::new();
    let mut fail_by_format: BTreeMap<String, u32> = BTreeMap::new();
    let mut fail_examples: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut total_ok = 0u32;
    let mut total_fail = 0u32;

    for entry in cache.iter_tags() {
        if entry.group_tag != BITM {
            continue;
        }
        let tag = match cache.read_tag(entry) {
            Ok(t) => t,
            Err(e) => {
                let key = "parse-fail".to_string();
                *fail_by_format.entry(key.clone()).or_default() += 1;
                let exs = fail_examples.entry(key).or_default();
                if exs.len() < 3 {
                    exs.push((entry.name.clone(), format!("{e:?}")));
                }
                total_fail += 1;
                continue;
            }
        };
        let bitmap = match Bitmap::new(&tag) {
            Ok(b) => b,
            Err(e) => {
                let key = "not-a-bitmap".to_string();
                *fail_by_format.entry(key.clone()).or_default() += 1;
                let exs = fail_examples.entry(key).or_default();
                if exs.len() < 3 {
                    exs.push((entry.name.clone(), format!("{e}")));
                }
                total_fail += 1;
                continue;
            }
        };
        for (i, image) in bitmap.iter().enumerate() {
            let fmt = image
                .format_name()
                .unwrap_or_else(|| "<missing>".into());
            let mut buf: Vec<u8> = Vec::new();
            match image.write_dds(&mut buf) {
                Ok(()) => {
                    *ok_by_format.entry(fmt).or_default() += 1;
                    total_ok += 1;
                }
                Err(e) => {
                    *fail_by_format.entry(fmt.clone()).or_default() += 1;
                    let exs = fail_examples.entry(fmt).or_default();
                    if exs.len() < 3 {
                        exs.push((format!("{}[{}]", entry.name, i), format!("{e}")));
                    }
                    total_fail += 1;
                }
            }
        }
    }

    println!("ok: {total_ok}");
    println!("fail: {total_fail}");

    println!("\nok counts by format:");
    for (f, n) in &ok_by_format {
        println!("  {:>7}  {}", n, f);
    }

    println!("\nfail counts by format:");
    for (f, n) in &fail_by_format {
        println!("  {:>7}  {}", n, f);
        if let Some(exs) = fail_examples.get(f) {
            for (name, err) in exs {
                println!("           {}  →  {}", name, err);
            }
        }
    }

    Ok(())
}
