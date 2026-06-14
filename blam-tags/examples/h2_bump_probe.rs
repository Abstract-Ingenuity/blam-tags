//! Probe H2 bitmaps: tally formats, and for p8/p8-bump report whether a
//! color plate (artist source) is present.
//!
//! Usage: h2_bump_probe <defs-dir> <tags-root>

use std::collections::BTreeMap;
use std::path::PathBuf;

use blam_tags::bitmap::{color_plate, Bitmap};
use blam_tags::classic::{read_classic_tag_file, ClassicHeader};
use blam_tags::layout::TagLayout;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let defs_dir = PathBuf::from(&args[1]);
    let root = &args[2];

    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(defs_dir.join("_meta.json")).unwrap()).unwrap();
    let mut name_for: BTreeMap<[u8; 4], String> = BTreeMap::new();
    for (k, v) in meta["tag_index"].as_object().unwrap() {
        let mut key = [b' '; 4];
        for (i, b) in k.bytes().take(4).enumerate() {
            key[i] = b;
        }
        name_for.insert(key, v.as_str().unwrap().to_owned());
    }

    let mut fmt_counts: BTreeMap<String, usize> = BTreeMap::new();
    // for p8/p8-bump: (has_plate, no_plate)
    let mut p8_plate = 0usize;
    let mut p8_noplate = 0usize;
    let mut examples_noplate: Vec<String> = Vec::new();

    for entry in walkdir(root) {
        if !entry.to_string_lossy().ends_with(".bitmap") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&entry) else { continue };
        let Some((header, _engine)) = ClassicHeader::parse(&bytes) else { continue };
        if name_for.get(&header.group_tag).map(|s| s.as_str()) != Some("bitmap") {
            continue;
        }
        let Ok(l) = TagLayout::from_json(defs_dir.join("bitmap.json")) else { continue };
        let tag = match read_classic_tag_file(&bytes, l) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let Ok(bm) = Bitmap::new(&tag) else { continue };
        let has_plate = color_plate(&tag).ok().flatten().is_some();
        let mut p8_here = false;
        for i in 0..bm.len() {
            let Some(img) = bm.image(i) else { continue };
            let f = img.format_name().unwrap_or_else(|| "?".into());
            *fmt_counts.entry(f.clone()).or_default() += 1;
            if f == "p8-bump" || f == "p8" {
                p8_here = true;
            }
        }
        if p8_here {
            if has_plate {
                p8_plate += 1;
            } else {
                p8_noplate += 1;
                if examples_noplate.len() < 10 {
                    examples_noplate.push(entry.to_string_lossy().into_owned());
                }
            }
        }
    }

    println!("=== format histogram (per image) ===");
    let mut sorted: Vec<_> = fmt_counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    for (f, c) in sorted {
        println!("  {c:>7}  {f}");
    }
    println!("=== p8/p8-bump tags: {p8_plate} with color plate, {p8_noplate} without ===");
    for e in &examples_noplate {
        println!("  no-plate: {e}");
    }
}

fn walkdir(root: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::from(root)];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}
