//! Sweep `.model_animation_graph` (jmad) tags and collect those that
//! have no `tgrc` (Exploded) resource chunk anywhere in the tag.
//! Writes the matching paths, one per line, to `OUTPUT`.
//!
//! Usage: jmad_missing_tgrc <OUTPUT> <DIR> [<DIR>...]

use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use blam_tags::{TagFile, TagResourceKind, TagStruct};

fn collect_jmads(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_jmads(&path, out)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("model_animation_graph") {
            out.push(path);
        }
    }
    Ok(())
}

fn has_tgrc(s: TagStruct<'_>) -> bool {
    for field in s.fields() {
        if let Some(resource) = field.as_resource() {
            if matches!(resource.kind(), TagResourceKind::Exploded) {
                return true;
            }
            if let Some(header) = resource.as_struct() {
                if has_tgrc(header) {
                    return true;
                }
            }
        } else if let Some(nested) = field.as_struct() {
            if has_tgrc(nested) {
                return true;
            }
        } else if let Some(block) = field.as_block() {
            for elem in block.iter() {
                if has_tgrc(elem) {
                    return true;
                }
            }
        } else if let Some(array) = field.as_array() {
            for elem in array.iter() {
                if has_tgrc(elem) {
                    return true;
                }
            }
        }
    }
    false
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let output = args.next().ok_or("usage: jmad_missing_tgrc <OUTPUT> <DIR> [<DIR>...]")?;
    let dirs: Vec<PathBuf> = args.map(PathBuf::from).collect();
    if dirs.is_empty() {
        eprintln!("usage: jmad_missing_tgrc <OUTPUT> <DIR> [<DIR>...]");
        std::process::exit(2);
    }

    let mut paths = Vec::new();
    for dir in &dirs {
        eprintln!("scanning {}", dir.display());
        collect_jmads(dir, &mut paths)?;
    }
    eprintln!("found {} .model_animation_graph tags", paths.len());

    let mut writer = BufWriter::new(File::create(&output)?);
    let mut scanned = 0u64;
    let mut read_failed = 0u64;
    let mut missing_tgrc = 0u64;

    for (i, path) in paths.iter().enumerate() {
        if i % 500 == 0 && i > 0 {
            eprintln!("  progress: {} / {}", i, paths.len());
        }
        scanned += 1;

        let tag = match TagFile::read(path) {
            Ok(t) => t,
            Err(e) => {
                read_failed += 1;
                eprintln!("  read failed: {} ({e})", path.display());
                continue;
            }
        };

        if !has_tgrc(tag.root()) {
            missing_tgrc += 1;
            writeln!(writer, "{}", path.display())?;
        }
    }

    writer.flush()?;

    eprintln!();
    eprintln!("scanned       : {scanned}");
    eprintln!("read failed   : {read_failed}");
    eprintln!("missing tgrc  : {missing_tgrc}");
    eprintln!("output        : {output}");

    Ok(())
}
