//! Sweep `.bitmap` tags and tally how many have an `info`
//! (import-info) stream attached, plus how many actually carry
//! a non-empty `files` block of zipped source assets.
//!
//! Usage: bitmap_import_info_sweep <DIR> [<DIR>...]

use std::error::Error;
use std::path::{Path, PathBuf};

use blam_tags::TagFile;

#[derive(Default)]
struct Tally {
    tags_total: u64,
    read_failed: u64,
    has_info_stream: u64,
    has_files_block: u64,
    files_nonempty: u64,
    total_files_entries: u64,
    total_zipped_bytes: u128,
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
        eprintln!("usage: bitmap_import_info_sweep <DIR> [<DIR>...]");
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

            let info = match tag.import_info() {
                Some(info) => info,
                None => continue,
            };
            t.has_info_stream += 1;

            let Some(files) = info.field_path("files").and_then(|f| f.as_block()) else {
                continue;
            };
            t.has_files_block += 1;

            let n = files.len();
            if n > 0 {
                t.files_nonempty += 1;
                t.total_files_entries += n as u64;
                for j in 0..n {
                    if let Some(elem) = files.element(j) {
                        if let Some(bytes) = elem.field("zipped data").and_then(|f| f.as_data()) {
                            t.total_zipped_bytes += bytes.len() as u128;
                        }
                    }
                }
            }
        }

        println!();
        println!("tags scanned          : {}", t.tags_total);
        println!("read failed           : {}", t.read_failed);
        println!("has `info` stream     : {}", t.has_info_stream);
        println!("  has `files` block   : {}", t.has_files_block);
        println!("    files non-empty   : {}", t.files_nonempty);
        println!("    total entries     : {}", t.total_files_entries);
        println!("    total zipped bytes: {}", t.total_zipped_bytes);
        println!();
    }

    Ok(())
}
