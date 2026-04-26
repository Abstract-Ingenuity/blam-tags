//! Sweep tags' import_info streams to find embedded zipped source files.
//! For each tag, count how many `files[]` entries have non-empty zipped
//! data and what extension the path ends in.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use blam_tags::{TagFile, TagFieldData};
fn collect(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() { return Ok(()); }
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() { collect(&p, ext, out)?; }
        else if p.extension().and_then(|s| s.to_str()) == Some(ext) { out.push(p); }
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let ext = args.next().unwrap();
    let dirs: Vec<PathBuf> = args.map(PathBuf::from).collect();
    let mut paths = Vec::new();
    for d in &dirs { collect(d, &ext, &mut paths)?; }
    eprintln!("scanning {} .{} tags", paths.len(), ext);

    let mut tags_total = 0u64;
    let mut tags_no_info = 0u64;
    let mut tags_no_files = 0u64;
    let mut tags_with_zipped = 0u64;
    let mut zipped_total_bytes = 0u64;
    let mut by_ext: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_ext_zipped: BTreeMap<String, u64> = BTreeMap::new();

    for (i, path) in paths.iter().enumerate() {
        if i % 500 == 0 && i > 0 { eprintln!("  progress {}/{}", i, paths.len()); }
        tags_total += 1;
        let tag = match TagFile::read(path) { Ok(t) => t, Err(_) => continue };
        let info = match tag.import_info() { Some(s) => s, None => { tags_no_info += 1; continue } };
        let files = match info.field_path("files").and_then(|f| f.as_block()) {
            Some(b) => b, None => { tags_no_files += 1; continue }
        };
        let mut had_zipped = false;
        for j in 0..files.len() {
            let elem = files.element(j).unwrap();
            let path_str = match elem.field("path").and_then(|f| f.value()) {
                Some(TagFieldData::LongString(s)) => s,
                Some(TagFieldData::String(s)) => s,
                _ => String::new(),
            };
            let path_ext = Path::new(&path_str)
                .extension().and_then(|s| s.to_str()).unwrap_or("?").to_lowercase();
            *by_ext.entry(path_ext.clone()).or_insert(0) += 1;
            let zlen = elem.field("zipped data").and_then(|f| f.as_data()).map(|b| b.len()).unwrap_or(0);
            if zlen > 0 {
                had_zipped = true;
                zipped_total_bytes += zlen as u64;
                *by_ext_zipped.entry(path_ext).or_insert(0) += 1;
            }
        }
        if had_zipped { tags_with_zipped += 1; }
    }

    println!();
    println!("tags scanned        : {tags_total}");
    println!("no import_info      : {tags_no_info}");
    println!("no files block      : {tags_no_files}");
    println!("with zipped data    : {tags_with_zipped}  ({:.2}%)", 100.0 * tags_with_zipped as f64 / tags_total.max(1) as f64);
    println!("total zipped bytes  : {} ({:.1} MB)", zipped_total_bytes, zipped_total_bytes as f64 / 1_048_576.0);
    println!();
    println!("file extensions seen (any zipped status):");
    let mut rows: Vec<_> = by_ext.iter().collect();
    rows.sort_by_key(|(_, c)| std::cmp::Reverse(**c));
    for (ext, count) in rows.iter().take(15) {
        let z = by_ext_zipped.get(*ext).copied().unwrap_or(0);
        println!("  {:<10}  {:>8}  ({} with zipped)", ext, count, z);
    }
    Ok(())
}
