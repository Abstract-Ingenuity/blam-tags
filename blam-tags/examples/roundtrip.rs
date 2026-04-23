//! Full-corpus byte-exact roundtrip validator.
//!
//! Reads every tag under the given root directories, writes each back
//! to a temp file via [`TagFile::write`], and md5-compares source vs
//! temp. Panics on the first mismatch.
//!
//! Usage:
//!
//! ```text
//! cargo run --release -p blam-tags --example roundtrip -- \
//!     <DIR> [<DIR>...] [--exclude <FILE>]...
//! ```
//!
//! At least one `<DIR>` is required. `--exclude` (alias `-x`) may be
//! repeated to skip individual tag files — useful for known-broken
//! tags in a corpus (e.g. MCC ships a couple with truncated chunk
//! streams) so the sweep can run uninterrupted.

use std::error::Error;
use std::path::{Path, PathBuf};

use blam_tags::file::TagFile;

fn collect_tag_paths<P: AsRef<Path>>(dir: P) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut paths = vec![];

    if dir.as_ref().is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                paths.extend(collect_tag_paths(&path)?);
            } else {
                if let Some(file_name) = path.file_name() && file_name == ".DS_Store" {
                    continue;
                }

                if let Some(extension) = path.extension() && extension == "txt" {
                    continue;
                }

                paths.push(path);
            }
        }
    }

    Ok(paths)
}

struct Args {
    roots: Vec<PathBuf>,
    excludes: Vec<PathBuf>,
}

fn parse_args() -> Result<Args, Box<dyn Error>> {
    let mut roots = Vec::new();
    let mut excludes = Vec::new();
    let mut raw = std::env::args().skip(1);

    while let Some(arg) = raw.next() {
        match arg.as_str() {
            "--exclude" | "-x" => {
                let value = raw
                    .next()
                    .ok_or("--exclude/-x requires a file path argument")?;
                excludes.push(PathBuf::from(value));
            }
            "-h" | "--help" => {
                println!(
                    "Usage: roundtrip <DIR> [<DIR>...] [--exclude <FILE>]...\n\
                     \n\
                     Walks each <DIR> recursively, roundtrips every tag through\n\
                     TagFile::read -> TagFile::write, and md5-compares source vs\n\
                     temp. Panics on the first mismatch.\n\
                     \n\
                     Options:\n  \
                       -x, --exclude <FILE>   Skip this tag (repeatable).\n  \
                       -h, --help             Print this help and exit."
                );
                std::process::exit(0);
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown flag: {other}").into());
            }
            _ => roots.push(PathBuf::from(arg)),
        }
    }

    if roots.is_empty() {
        return Err(
            "expected at least one tag root directory (run with --help for usage)".into(),
        );
    }

    Ok(Args { roots, excludes })
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;

    let mut tag_file_paths = Vec::new();
    for root in &args.roots {
        tag_file_paths.extend(collect_tag_paths(root)?);
    }

    // Reuse a single temp path for the roundtrip — never write back to the
    // source tag file. Using the process temp dir so concurrent runs don't
    // clobber each other is fine here since we're single-threaded.
    let temp_path = std::env::temp_dir().join(format!(
        "blam_tags_roundtrip_{}.tmp",
        std::process::id(),
    ));

    for tag_file_path in tag_file_paths {
        if args.excludes.contains(&tag_file_path) {
            continue;
        }

        println!("Loading \"{}\"...", tag_file_path.display());

        let tag_file = TagFile::read(&tag_file_path)?;

        // Roundtrip: write to temp, md5 both, and compare digests.
        tag_file.write(&temp_path)?;

        let source_bytes = std::fs::read(&tag_file_path)?;
        let temp_bytes = std::fs::read(&temp_path)?;
        let source_digest = md5::compute(&source_bytes);
        let temp_digest = md5::compute(&temp_bytes);

        if source_digest != temp_digest {
            let mismatch_offset = source_bytes
                .iter()
                .zip(temp_bytes.iter())
                .position(|(a, b)| a != b);
            panic!(
                "roundtrip mismatch for \"{}\": source md5 {:x} ({} bytes), temp md5 {:x} ({} bytes), \
                 first differing byte at 0x{:X?}",
                tag_file_path.display(),
                source_digest,
                source_bytes.len(),
                temp_digest,
                temp_bytes.len(),
                mismatch_offset,
            );
        }
    }

    let _ = std::fs::remove_file(&temp_path);

    Ok(())
}
