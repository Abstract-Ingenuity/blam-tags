//! Sweep `.bitmap` tags and try to write each image as a TIFF in
//! memory. Reports per-format counts split into:
//!   - ok                      — wrote a Tool-importable TIFF
//!   - bc-deferred             — BC formats waiting on Phase 3
//!   - layout-deferred         — cube / array / 3D waiting on Phase 4
//!   - oob / unsupp / other    — actual extraction failures
//!
//! No files are written.
//!
//! Usage: extract_tiff_sweep <DIR> [<DIR>...]

use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use blam_tags::{Bitmap, BitmapError, TagFile};

#[derive(Default)]
struct Tally {
    ok: u64,
    bc_deferred: u64,
    layout_deferred: u64,
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
        eprintln!("usage: extract_tiff_sweep <DIR> [<DIR>...]");
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

                let mut sink = std::io::sink();
                let result = image.write_tiff(&mut sink);

                let entry = by_format.entry(format_name.clone()).or_default();
                match &result {
                    Ok(()) => entry.ok += 1,
                    Err(BitmapError::FormatNotSupported(msg)) => {
                        // Distinguish "compressed; awaiting Phase 3"
                        // from genuine unsupported. The decode
                        // module's message includes "compressed".
                        if msg.contains("compressed") {
                            entry.bc_deferred += 1;
                        } else {
                            entry.fail_format_unsupported += 1;
                        }
                    }
                    Err(BitmapError::TiffLayoutDeferred(_)) => entry.layout_deferred += 1,
                    Err(BitmapError::PixelSliceOutOfBounds { .. }) => entry.fail_pixel_oob += 1,
                    Err(BitmapError::UnsupportedTextureType(_)) => entry.fail_unsupported_type += 1,
                    Err(_) => entry.fail_other += 1,
                }
                let is_real_failure = !matches!(
                    result,
                    Ok(())
                        | Err(BitmapError::FormatNotSupported(_))
                        | Err(BitmapError::TiffLayoutDeferred(_))
                );
                if is_real_failure && entry.sample_failure.is_none() {
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
            "  {:<22}  {:>9}  {:>11}  {:>15}  {:>10}  {:>10}  {:>10}  {:>10}",
            "format", "ok", "bc-deferred", "layout-deferred",
            "unsupp", "oob", "bad-type", "other"
        );

        let mut rows: Vec<_> = by_format.iter().collect();
        rows.sort_by_key(|(_, v)| std::cmp::Reverse(
            v.ok + v.bc_deferred + v.layout_deferred
                + v.fail_format_unsupported + v.fail_pixel_oob
                + v.fail_unsupported_type + v.fail_other
        ));
        for (name, t) in rows {
            println!(
                "  {:<22}  {:>9}  {:>11}  {:>15}  {:>10}  {:>10}  {:>10}  {:>10}",
                name, t.ok, t.bc_deferred, t.layout_deferred,
                t.fail_format_unsupported, t.fail_pixel_oob,
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
