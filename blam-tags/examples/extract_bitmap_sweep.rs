//! Sweep `.bitmap` tags and try to extract each to a DDS in memory.
//! Reports per-format success / failure counts plus per-error
//! sample paths. No files are written.
//!
//! Usage: extract_bitmap_sweep <DIR> [<DIR>...]

use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use blam_tags::{Bitmap, BitmapError, TagFile};

#[derive(Default)]
struct Tally {
    ok: u64,
    fail_format_unsupported: u64,
    fail_pixel_oob: u64,
    fail_unsupported_type: u64,
    fail_other: u64,
    sample_failure: Option<String>,
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
        eprintln!("usage: extract_bitmap_sweep <DIR> [<DIR>...]");
        std::process::exit(2);
    }

    for dir in &dirs {
        println!("== {} ==", dir.display());

        let mut paths = Vec::new();
        collect_bitmaps(dir, &mut paths)?;
        println!("found {} .bitmap tags", paths.len());

        let mut tags_total = 0u64;
        let mut tags_failed_to_read = 0u64;
        let mut images_total = 0u64;
        let mut by_format: BTreeMap<String, Tally> = BTreeMap::new();

        for (i, path) in paths.iter().enumerate() {
            if i % 1000 == 0 && i > 0 {
                eprintln!("  progress: {} / {}", i, paths.len());
            }
            tags_total += 1;

            let tag = match TagFile::read(path) {
                Ok(t) => t,
                Err(_) => {
                    tags_failed_to_read += 1;
                    continue;
                }
            };

            let bitmap = match Bitmap::new(&tag) {
                Ok(b) => b,
                Err(_) => continue,
            };

            for image in bitmap.iter() {
                images_total += 1;
                let format_name = image.format_name().unwrap_or_else(|| "?".into());

                // Try a dry-run extract: build the DDS into a sink
                // counter and check format/slice errors only.
                let mut sink = std::io::sink();
                let result = image.write_dds(&mut sink);

                let entry = by_format.entry(format_name.clone()).or_default();
                match result {
                    Ok(()) => entry.ok += 1,
                    Err(BitmapError::FormatNotSupported(_)) => entry.fail_format_unsupported += 1,
                    Err(BitmapError::PixelSliceOutOfBounds { .. }) => entry.fail_pixel_oob += 1,
                    Err(BitmapError::UnsupportedTextureType(_)) => entry.fail_unsupported_type += 1,
                    Err(_) => entry.fail_other += 1,
                }
                if !matches!(result, Ok(())) && entry.sample_failure.is_none() {
                    entry.sample_failure = Some(format!(
                        "{}: {}",
                        path.display(),
                        result.err().map(|e| e.to_string()).unwrap_or_default()
                    ));
                }
            }
        }

        println!();
        println!("tags scanned : {tags_total}");
        println!("read failed  : {tags_failed_to_read}");
        println!("images total : {images_total}");
        println!();
        println!(
            "  {:<25}  {:>9}  {:>10}  {:>10}  {:>10}  {:>10}",
            "format", "ok", "unsupp", "oob", "bad-type", "other"
        );

        let mut rows: Vec<_> = by_format.iter().collect();
        rows.sort_by_key(|(_, v)| std::cmp::Reverse(v.ok + v.fail_format_unsupported + v.fail_pixel_oob + v.fail_unsupported_type + v.fail_other));
        for (name, t) in rows {
            println!(
                "  {:<25}  {:>9}  {:>10}  {:>10}  {:>10}  {:>10}",
                name, t.ok, t.fail_format_unsupported, t.fail_pixel_oob,
                t.fail_unsupported_type, t.fail_other,
            );
            if let Some(s) = &t.sample_failure {
                println!("        sample: {s}");
            }
        }
        println!();
    }

    Ok(())
}
