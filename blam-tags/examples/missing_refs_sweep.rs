//! Walk every tag under one or more roots, visit every
//! `tag_reference` field, and report any whose target file is
//! missing on disk. Matches by exact group extension (looked up in
//! `definitions/<game>/_meta.json`'s `tag_index` map), not by stem
//! prefix — so a `physics_model` ref that targets a path where only
//! a `model` file lives is correctly flagged as missing.
//!
//! Usage:
//!   missing_refs_sweep <META.json> <TAGS_ROOT>
//!
//! Output: one line per (referencing_tag, field_path, missing_target).
//! Trailing summary aggregates missing targets by reference count.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use blam_tags::{
    format_group_tag, TagBlock, TagField, TagFieldData, TagFile, TagStruct,
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let meta_path = args.next().ok_or("usage: missing_refs_sweep <META.json> <TAGS_ROOT>")?;
    let tags_root = PathBuf::from(args.next().ok_or("usage: missing_refs_sweep <META.json> <TAGS_ROOT>")?);

    let group_to_ext = load_tag_index(Path::new(&meta_path))?;
    eprintln!("loaded {} group → extension entries", group_to_ext.len());

    let mut tag_paths = Vec::new();
    collect_tag_files(&tags_root, &mut tag_paths)?;
    tag_paths.sort();
    eprintln!("scanning {} tags under {}", tag_paths.len(), tags_root.display());

    // missing_targets[(group, target_path)] -> Vec<(referencing_tag_relative_path, field_path)>
    let mut missing_targets: BTreeMap<(String, String), Vec<(String, String)>> = BTreeMap::new();
    let mut tags_read = 0u64;
    let mut tags_failed_to_read = 0u64;

    for (i, path) in tag_paths.iter().enumerate() {
        if i % 5000 == 0 && i > 0 {
            eprintln!("  progress: {} / {}  ({} missing-target sites so far)",
                i, tag_paths.len(),
                missing_targets.values().map(|v| v.len()).sum::<usize>(),
            );
        }
        let tag = match TagFile::read(path) {
            Ok(t) => t,
            Err(_) => { tags_failed_to_read += 1; continue; }
        };
        tags_read += 1;

        let rel_referencing = path
            .strip_prefix(&tags_root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();

        let mut local_misses: Vec<(String, String, String)> = Vec::new();
        walk_struct(tag.root(), "", &mut |field_path, field| {
            let Some(TagFieldData::TagReference(r)) = field.value() else { return; };
            let Some((group_u32, target_name)) = r.group_tag_and_name else { return; };
            let group_str = format_group_tag(group_u32);
            let Some(extension) = group_to_ext.get(&group_str) else {
                // Unknown group code — skip rather than spuriously flag.
                return;
            };
            // Normalize Halo's `\` to OS sep, append the group extension.
            let normalized: PathBuf = target_name.split('\\').collect();
            let mut abs = tags_root.join(&normalized);
            abs.set_extension(extension);
            if !abs.is_file() {
                local_misses.push((
                    field_path.to_string(),
                    group_str.clone(),
                    target_name.clone(),
                ));
            }
        });

        for (field_path, group, target) in local_misses {
            missing_targets
                .entry((group, target))
                .or_default()
                .push((rel_referencing.clone(), field_path));
        }
    }

    // Print one block per unique missing target. The header line is
    // the target path with its full group filename extension
    // (e.g. `.bitmap` not `.bitm`); each indented sub-line is a
    // distinct referencing tag (deduped + sorted). Path separators
    // normalized to `/` throughout.
    let mut entries: Vec<(String, Vec<String>)> = missing_targets
        .iter()
        .map(|((group, target), referencers)| {
            let extension = group_to_ext
                .get(group)
                .map(String::as_str)
                .unwrap_or(group.as_str());
            let header = format!("{}.{extension}", target.replace('\\', "/"));
            // Dedupe referencing tags: the same tag may reference
            // the missing target via multiple field paths (e.g. a
            // bitmap appearing in `secondary_map` AND `tertiary_map`).
            let mut tags: Vec<String> = referencers
                .iter()
                .map(|(t, _field)| t.replace('\\', "/"))
                .collect();
            tags.sort();
            tags.dedup();
            (header, tags)
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (header, tags) in &entries {
        println!("{header}");
        for t in tags {
            println!("    {t}");
        }
    }

    eprintln!();
    eprintln!("=== summary ===");
    eprintln!("tags scanned     : {}", tags_read);
    eprintln!("tags read failed : {}", tags_failed_to_read);
    eprintln!("missing targets  : {}", missing_targets.len());
    let total_sites: usize = missing_targets.values().map(|v| v.len()).sum();
    eprintln!("ref sites        : {}", total_sites);

    Ok(())
}

fn collect_tag_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_tag_files(&path, out)?;
        } else if path.extension().is_some() {
            out.push(path);
        }
    }
    Ok(())
}

/// Parse `definitions/<game>/_meta.json`'s `tag_index` map: group-tag
/// 4-char string → group filename (e.g. `"phmo"` → `"physics_model"`).
fn load_tag_index(path: &Path) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let text = fs::read_to_string(path)?;
    let value: serde_json::Value = serde_json::from_str(&text)?;
    let map = value.get("tag_index")
        .and_then(|v| v.as_object())
        .ok_or("missing `tag_index` in _meta.json")?;
    let mut out = BTreeMap::new();
    for (k, v) in map {
        if let Some(ext) = v.as_str() {
            out.insert(k.clone(), ext.to_string());
        }
    }
    Ok(out)
}

/// Recursively visit every leaf field in a tag tree, calling `visit`
/// with `(field_path, field)`. Descends through inline structs,
/// blocks, arrays, and pageable resources (whose exploded `tgrc`
/// payloads can also carry tag-refs).
fn walk_struct<'a, F>(s: TagStruct<'a>, parent_path: &str, visit: &mut F)
where
    F: FnMut(&str, TagField<'a>),
{
    for field in s.fields() {
        let name = field.name().to_string();
        let path = if parent_path.is_empty() { name.clone() } else { format!("{parent_path}/{name}") };

        if let Some(child) = field.as_struct() {
            walk_struct(child, &path, visit);
            continue;
        }
        if let Some(block) = field.as_block() {
            walk_block(block, &path, visit);
            continue;
        }
        if let Some(arr) = field.as_array() {
            for i in 0..arr.len() {
                if let Some(elem) = arr.element(i) {
                    let elem_path = format!("{path}[{i}]");
                    walk_struct(elem, &elem_path, visit);
                }
            }
            continue;
        }
        if let Some(res) = field.as_resource() {
            if let Some(inner) = res.as_struct() {
                walk_struct(inner, &path, visit);
            }
            continue;
        }

        visit(&path, field);
    }
}

fn walk_block<'a, F>(block: TagBlock<'a>, parent_path: &str, visit: &mut F)
where
    F: FnMut(&str, TagField<'a>),
{
    for i in 0..block.len() {
        let Some(elem) = block.element(i) else { continue; };
        let elem_path = format!("{parent_path}[{i}]");
        walk_struct(elem, &elem_path, visit);
    }
}
