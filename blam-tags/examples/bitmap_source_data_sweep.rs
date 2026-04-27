//! Sweep `.bitmap` tags and report which ones still carry artist
//! `source data` (the color-plate / TIFF blob) versus only
//! `processed pixel data`. Useful for deciding whether a TIFF
//! exporter can pass through the original source vs. having to
//! synthesize from processed pixels.
//!
//! Usage: bitmap_source_data_sweep <DIR> [<DIR>...]

use std::error::Error;
use std::path::{Path, PathBuf};

use blam_tags::TagFile;

#[derive(Default)]
struct Tally {
    tags_total: u64,
    read_failed: u64,
    not_a_bitmap_root: u64,
    has_source_field: u64,
    has_processed_field: u64,
    source_nonempty: u64,
    processed_nonempty: u64,
    source_total_bytes: u128,
    processed_total_bytes: u128,
    source_max_bytes: u64,
    sample_with_source: Vec<(PathBuf, u64)>,
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

fn main() -> Result<(), Box<dyn Error>> {
    let dirs: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if dirs.is_empty() {
        eprintln!("usage: bitmap_source_data_sweep <DIR> [<DIR>...]");
        std::process::exit(2);
    }

    for dir in &dirs {
        println!("== {} ==", dir.display());

        let mut paths = Vec::new();
        collect_bitmaps(dir, &mut paths)?;
        println!("found {} .bitmap tags", paths.len());

        let mut t = Tally::default();

        for (i, path) in paths.iter().enumerate() {
            if i % 2000 == 0 && i > 0 {
                eprintln!("  progress: {} / {}", i, paths.len());
            }
            t.tags_total += 1;

            let tag = match TagFile::read(path) {
                Ok(tag) => tag,
                Err(_) => {
                    t.read_failed += 1;
                    continue;
                }
            };

            let root = tag.root();

            let source = root.field_path("source data").and_then(|f| f.as_data());
            let processed = root.field_path("processed pixel data").and_then(|f| f.as_data());

            if source.is_none() && processed.is_none() {
                t.not_a_bitmap_root += 1;
                continue;
            }

            if let Some(bytes) = source {
                t.has_source_field += 1;
                if !bytes.is_empty() {
                    t.source_nonempty += 1;
                    let n = bytes.len() as u64;
                    t.source_total_bytes += n as u128;
                    if n > t.source_max_bytes {
                        t.source_max_bytes = n;
                    }
                    if t.sample_with_source.len() < 5 {
                        t.sample_with_source.push((path.clone(), n));
                    }
                }
            }
            if let Some(bytes) = processed {
                t.has_processed_field += 1;
                if !bytes.is_empty() {
                    t.processed_nonempty += 1;
                    t.processed_total_bytes += bytes.len() as u128;
                }
            }
        }

        println!();
        println!("tags scanned          : {}", t.tags_total);
        println!("read failed           : {}", t.read_failed);
        println!("no source/processed   : {}", t.not_a_bitmap_root);
        println!("has `source data`     : {}", t.has_source_field);
        println!("  non-empty           : {}", t.source_nonempty);
        println!("  total bytes         : {}", t.source_total_bytes);
        println!("  largest single blob : {}", t.source_max_bytes);
        println!("has `processed pixel` : {}", t.has_processed_field);
        println!("  non-empty           : {}", t.processed_nonempty);
        println!("  total bytes         : {}", t.processed_total_bytes);
        if !t.sample_with_source.is_empty() {
            println!();
            println!("samples with source data:");
            for (p, n) in &t.sample_with_source {
                println!("  [{n} bytes] {}", p.display());
            }
        }
        println!();
    }

    Ok(())
}
