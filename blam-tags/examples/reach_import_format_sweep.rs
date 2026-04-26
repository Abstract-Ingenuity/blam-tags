//! Reach .render_model import_info file-format census.
//!
//! Walks every .render_model under the given root, peeks into the
//! import_info stream's `files[]/path` to classify the asset format
//! by extension. Counts `.jms` vs `.gr2` vs everything else, with
//! per-tag and per-payload breakdowns.

use std::path::{Path, PathBuf};
use blam_tags::{TagFieldData, TagFile};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return; };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() { walk(&p, out); }
        else if p.extension().and_then(|s| s.to_str()) == Some("render_model") {
            out.push(p);
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::args().nth(1).ok_or("usage: reach_import_format_sweep <DIR>")?;
    let mut paths = Vec::new();
    walk(Path::new(&dir), &mut paths);
    eprintln!("scanning {} render_models under {}", paths.len(), dir);

    let mut total = 0;
    let mut no_import_info = 0;
    let mut no_files = 0;
    let mut no_files_entries = 0;
    let mut by_ext: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut by_ext_with_payload: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut tags_with_jms_entry: usize = 0;
    let mut tags_with_gr2_entry: usize = 0;
    let mut tags_with_jms_payload: usize = 0;
    let mut tags_with_gr2_payload: usize = 0;
    let mut tags_with_any_payload: usize = 0;
    let mut empty_zipped: usize = 0;
    let mut nonempty_zipped: usize = 0;

    for p in &paths {
        total += 1;
        let Ok(tag) = TagFile::read(p) else { continue; };
        let Some(info) = tag.import_info() else { no_import_info += 1; continue };
        let Some(files) = info.field_path("files").and_then(|f| f.as_block()) else { no_files += 1; continue };
        if files.len() == 0 { no_files_entries += 1; continue; }

        let mut tag_has_jms = false;
        let mut tag_has_gr2 = false;
        let mut tag_has_jms_payload = false;
        let mut tag_has_gr2_payload = false;
        let mut tag_has_any_payload = false;
        for i in 0..files.len() {
            let elem = files.element(i).unwrap();
            let path_str = match elem.field("path").and_then(|f| f.value()) {
                Some(TagFieldData::LongString(s) | TagFieldData::String(s)) => s,
                _ => String::new(),
            };
            let ext = std::path::Path::new(&path_str.replace('\\', "/"))
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase())
                .unwrap_or_else(|| "<none>".to_string());
            *by_ext.entry(ext.clone()).or_insert(0) += 1;
            let zipped = elem.field("zipped data").and_then(|f| f.as_data()).unwrap_or(&[]);
            if zipped.is_empty() { empty_zipped += 1; }
            else {
                nonempty_zipped += 1;
                tag_has_any_payload = true;
                *by_ext_with_payload.entry(ext.clone()).or_insert(0) += 1;
            }
            match ext.as_str() {
                "jms" => {
                    tag_has_jms = true;
                    if !zipped.is_empty() { tag_has_jms_payload = true; }
                }
                "gr2" => {
                    tag_has_gr2 = true;
                    if !zipped.is_empty() { tag_has_gr2_payload = true; }
                }
                _ => {}
            }
        }
        if tag_has_jms { tags_with_jms_entry += 1; }
        if tag_has_gr2 { tags_with_gr2_entry += 1; }
        if tag_has_jms_payload { tags_with_jms_payload += 1; }
        if tag_has_gr2_payload { tags_with_gr2_payload += 1; }
        if tag_has_any_payload { tags_with_any_payload += 1; }
    }

    println!("total render_models: {total}");
    println!("  no import_info stream:  {no_import_info}");
    println!("  no `files` block:       {no_files}");
    println!("  empty `files` block:    {no_files_entries}");
    println!();
    println!("Per-TAG (each tag counted once per format that appears):");
    println!("  has any .jms entry:    {tags_with_jms_entry}");
    println!("  has any .gr2 entry:    {tags_with_gr2_entry}");
    println!("  has .jms with payload: {tags_with_jms_payload}");
    println!("  has .gr2 with payload: {tags_with_gr2_payload}");
    println!("  has any payload:       {tags_with_any_payload}");
    println!();
    println!("Per-FILE-ENTRY (across all files[] across all tags):");
    let mut by_ext_sorted: Vec<(String, usize)> = by_ext.into_iter().collect();
    by_ext_sorted.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (ext, n) in &by_ext_sorted {
        let with = by_ext_with_payload.get(ext).copied().unwrap_or(0);
        println!("  .{ext:8} {n:6}  ({with} with payload)");
    }
    println!();
    println!("zipped data presence (entries):");
    println!("  empty:    {empty_zipped}");
    println!("  nonempty: {nonempty_zipped}");
    Ok(())
}
