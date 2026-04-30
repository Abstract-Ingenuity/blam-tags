//! Per-FAIL-group delta histogram.
//!
//! Walks every tag of every group under `<TAGS_ROOT>` that doesn't
//! match its dumped schema's root struct (size + field count). For
//! each group with ≥1 mismatch, prints a histogram of `(size delta,
//! field-count delta)` → tag count, sorted by frequency.
//!
//! Use this to distinguish "dumper drift" (one stable delta covering
//! every tag) from "dev-era drift" (multiple deltas, scattered).
//!
//! ```text
//! cargo run --release -p blam-tags --example schema_delta_histogram -- \
//!     definitions/halo4_mcc /Users/camden/Halo/halo4_mcc/tags
//! ```

use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use blam_tags::TagFile;

fn collect_tags(root: &Path, ext: &OsStr, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(root) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_tags(&path, ext, out);
        } else if path.extension() == Some(ext) {
            out.push(path);
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let defs_dir = PathBuf::from(
        args.next().ok_or("usage: schema_delta_histogram <DEFS_DIR> <TAGS_ROOT>")?,
    );
    let tags_root = PathBuf::from(
        args.next().ok_or("usage: schema_delta_histogram <DEFS_DIR> <TAGS_ROOT>")?,
    );

    let mut group_paths: Vec<PathBuf> = std::fs::read_dir(&defs_dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension() == Some(OsStr::new("json"))
            && p.file_name() != Some(OsStr::new("_meta.json")))
        .collect();
    group_paths.sort();

    for schema_path in &group_paths {
        let group = schema_path.file_stem().and_then(|s| s.to_str()).unwrap_or("?").to_owned();
        let Ok(schema_tag) = TagFile::new(schema_path) else { continue };
        let schema_root = schema_tag.definitions().root_struct();
        let schema_size = schema_root.size() as isize;
        let schema_fields = schema_root.fields().count() as isize;

        let mut tags: Vec<PathBuf> = Vec::new();
        collect_tags(&tags_root, OsStr::new(&group), &mut tags);
        if tags.is_empty() { continue; }

        // (size_delta, field_delta) → count
        let mut hist: BTreeMap<(isize, isize), usize> = BTreeMap::new();
        let mut matched = 0usize;
        let mut errored = 0usize;
        for tag_path in &tags {
            let real = match TagFile::read(tag_path) {
                Ok(t) => t,
                Err(_) => { errored += 1; continue; }
            };
            let r = real.definitions().root_struct();
            let dz = schema_size - r.size() as isize;
            let df = schema_fields - r.fields().count() as isize;
            if dz == 0 && df == 0 {
                matched += 1;
            } else {
                *hist.entry((dz, df)).or_insert(0) += 1;
            }
        }

        if hist.is_empty() { continue; }

        println!(
            "{:42}  matched {}/{}  ({} errored)  schema=size{} fields{}",
            group, matched, tags.len(), errored, schema_size, schema_fields,
        );
        // Sort by frequency descending
        let mut rows: Vec<_> = hist.into_iter().collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        for ((dz, df), n) in rows {
            println!(
                "  Δsize={:+5}  Δfields={:+3}  ×{}",
                dz, df, n
            );
        }
    }

    Ok(())
}
