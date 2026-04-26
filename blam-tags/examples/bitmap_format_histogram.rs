//! Sweep a directory of `.bitmap` tags and tally bitmap_data.format
//! values + extractability signal (inline pixel data vs paged out).
//!
//! Usage: bitmap_format_histogram <DIR> [<DIR>...]

use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use blam_tags::{TagFieldData, TagFile};

#[derive(Default)]
struct FormatStats {
    /// Total occurrences of this format across all bitmaps.
    images: u64,
    /// Distinct tags that contain at least one image of this format.
    tags: u64,
    /// Of those `images`, how many had non-zero `processed pixel data`
    /// available for direct extraction (no hardware-textures resource
    /// indirection).
    images_with_inline_pixels: u64,
}

#[derive(Default)]
struct GameStats {
    tags_scanned: u64,
    tags_failed: u64,
    tags_with_zero_images: u64,
    tags_with_inline_pixel_data: u64,
    tags_with_hardware_textures: u64,
    /// key: "33 dxn" / "14 dxt1" — integer + resolved name.
    by_format: BTreeMap<String, FormatStats>,
}

fn collect_bitmaps(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_bitmaps(&path, out)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("bitmap") {
            out.push(path);
        }
    }
    Ok(())
}

fn process_tag(path: &Path, stats: &mut GameStats) {
    stats.tags_scanned += 1;

    let tag = match TagFile::read(path) {
        Ok(t) => t,
        Err(e) => {
            stats.tags_failed += 1;
            eprintln!("read failed: {} : {e}", path.display());
            return;
        }
    };

    let root = tag.root();

    // Inline pixel data presence — top-level "processed pixel data".
    let inline_size = root
        .field_path("processed pixel data")
        .and_then(|f| f.value())
        .and_then(|v| match v {
            TagFieldData::Data(bytes) => Some(bytes.len()),
            _ => None,
        })
        .unwrap_or(0);
    if inline_size > 0 {
        stats.tags_with_inline_pixel_data += 1;
    }

    // Hardware textures present?
    let hw_count = root
        .field_path("hardware textures")
        .and_then(|f| f.as_block())
        .map(|b| b.len())
        .unwrap_or(0);
    if hw_count > 0 {
        stats.tags_with_hardware_textures += 1;
    }

    // Walk bitmaps[].
    let Some(bitmaps) = root.field_path("bitmaps").and_then(|f| f.as_block()) else {
        stats.tags_with_zero_images += 1;
        return;
    };
    if bitmaps.is_empty() {
        stats.tags_with_zero_images += 1;
        return;
    }

    let mut seen_formats_in_tag: BTreeMap<String, ()> = BTreeMap::new();

    for elem in bitmaps.iter() {
        let Some(format_field) = elem.field("format") else { continue };
        let key = match format_field.value() {
            Some(TagFieldData::ShortEnum { value, name }) => {
                format!("{:>3} {}", value, name.as_deref().unwrap_or("(unnamed)"))
            }
            _ => "(?)".to_owned(),
        };
        let entry = stats.by_format.entry(key.clone()).or_default();
        entry.images += 1;
        if inline_size > 0 {
            entry.images_with_inline_pixels += 1;
        }
        seen_formats_in_tag.insert(key, ());
    }

    // Bump "tags" count once per (tag, format) pair.
    for key in seen_formats_in_tag.keys() {
        if let Some(entry) = stats.by_format.get_mut(key) {
            entry.tags += 1;
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let dirs: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if dirs.is_empty() {
        eprintln!("usage: bitmap_format_histogram <DIR> [<DIR>...]");
        std::process::exit(2);
    }

    for dir in &dirs {
        println!("== {} ==", dir.display());

        let mut paths = Vec::new();
        collect_bitmaps(dir, &mut paths)?;
        println!("found {} .bitmap tags", paths.len());

        let mut stats = GameStats::default();
        for (i, path) in paths.iter().enumerate() {
            if i % 1000 == 0 && i > 0 {
                eprintln!("  progress: {} / {}", i, paths.len());
            }
            process_tag(path, &mut stats);
        }

        println!();
        println!("scanned        : {}", stats.tags_scanned);
        println!("failed         : {}", stats.tags_failed);
        println!("zero-image tags: {}", stats.tags_with_zero_images);
        println!("inline pixels  : {}  (processed pixel data > 0)", stats.tags_with_inline_pixel_data);
        println!("hw textures    : {}  (hardware textures block populated)", stats.tags_with_hardware_textures);
        println!();
        println!("format histogram (sorted by image count):");
        println!("  {:<35}  {:>9}  {:>9}  {:>9}", "format", "images", "tags", "inline");
        let mut rows: Vec<_> = stats.by_format.iter().collect();
        rows.sort_by_key(|(_, v)| std::cmp::Reverse(v.images));
        for (key, v) in rows {
            println!(
                "  {:<35}  {:>9}  {:>9}  {:>9}",
                key, v.images, v.tags, v.images_with_inline_pixels
            );
        }
        println!();
    }

    Ok(())
}
