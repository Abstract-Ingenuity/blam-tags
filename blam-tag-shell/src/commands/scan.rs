use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use blam_tags::TagFile;
use serde_json::json;

pub fn run(dir: &str, json_output: bool, sort: &str) -> Result<()> {
    // (group_tag string, extension) → count
    let mut groups: BTreeMap<(String, String), u64> = BTreeMap::new();

    walk_dir(Path::new(dir), &mut groups).context("failed to scan directory")?;

    let mut rows: Vec<(String, String, u64)> = groups
        .into_iter()
        .map(|((group, ext), count)| (group, ext, count))
        .collect();

    match sort {
        "count" => rows.sort_by(|a, b| b.2.cmp(&a.2)),
        _ => rows.sort_by(|a, b| a.1.cmp(&b.1)),
    }

    if json_output {
        let arr: Vec<_> = rows
            .iter()
            .map(|(group, ext, count)| json!({ "group": group, "extension": ext, "count": count }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else {
        println!("{:<8} {:<24} {:>8}", "GROUP", "EXTENSION", "COUNT");
        println!("{}", "-".repeat(44));
        let mut total = 0u64;
        for (group, ext, count) in &rows {
            println!("{:<8} {:<24} {:>8}", group, ext, count);
            total += count;
        }
        println!("{}", "-".repeat(44));
        println!("{:<8} {:<24} {:>8}", format!("{} types", rows.len()), "", total);
    }

    Ok(())
}

fn walk_dir(dir: &Path, groups: &mut BTreeMap<(String, String), u64>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, groups)?;
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name == ".DS_Store" {
                continue;
            }
        }
        // We're loading the full TagFile which is overkill for scanning,
        // but there's no lighter "just the header" loader yet and this
        // matches how roundtrip loads files.
        let tag = match TagFile::read(&path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let group = tag.group().to_string();
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();
        *groups.entry((group, extension)).or_insert(0) += 1;
    }
    Ok(())
}
