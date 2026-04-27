//! `list` — walk a tag directory and emit matching paths.
//!
//! For standalone tag files the file extension *is* the group name
//! (`.render_model`, `.biped`, `.scenario`, etc.), so filtering by
//! group needs zero file reads — a path-only walk over even a full
//! H3+Reach corpus stays comfortably under a second.
//!
//! `--group` accepts either form: a 4-byte group tag (`mode`) gets
//! translated to its long name (`render_model`) via the loaded
//! [`crate::tag_index::TagIndex`], so existing scripts keep working.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use blam_tags::parse_group_tag;
use regex::Regex;
use serde_json::json;

use crate::context::CliContext;

/// Path-shape filters for [`run`]. Each `Option` left as `None`
/// passes through unfiltered; setting any of them narrows the walk.
pub struct ListFilters {
    /// Group name (`"biped"`) or 4-byte group tag (`"bipd"`).
    pub group: Option<String>,
    /// Match filenames starting with this prefix.
    pub starts_with: Option<String>,
    /// Match paths containing this substring.
    pub contains: Option<String>,
    /// Match filenames ending with this suffix (extension matching).
    pub ends_with: Option<String>,
    /// Regex to match against full paths.
    pub regex: Option<String>,
    /// Path to a file with newline-separated candidates (skips the
    /// directory walk entirely).
    pub from_file: Option<String>,
}

/// Output mode for the `list` command. `Paths` is one path per line;
/// `Summary` tallies by group; `Json` emits a structured form.
pub enum OutputMode {
    /// One path per line.
    Paths,
    /// Group tally instead of a path list.
    Summary {
        /// Sort summary rows by count desc instead of name.
        sort_by_count: bool,
    },
    /// Machine-readable JSON.
    Json,
}

pub fn run(ctx: &CliContext, dir: &str, filters: ListFilters, mode: OutputMode) -> Result<()> {
    let regex = filters.regex.as_deref().map(Regex::new).transpose().context("invalid --regex pattern")?;
    let from_file = load_from_file(filters.from_file.as_deref())?;

    let candidates = if let Some(list) = from_file {
        list
    } else {
        let mut paths = Vec::new();
        walk(Path::new(dir), &mut paths)?;
        paths
    };

    // Resolve `--group` once: prefer it as an extension/long-name
    // (`render_model`); fall back to parsing it as a 4-byte group tag
    // (`mode`) and looking the long name up in the tag index. This
    // way `--group mode` and `--group render_model` are equivalent.
    let group_filter = filters
        .group
        .as_deref()
        .map(|raw| resolve_group(ctx, raw))
        .transpose()?;

    let mut matched: Vec<(PathBuf, String)> = Vec::new();
    for path in &candidates {
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else { continue };

        if let Some(want_ext) = &group_filter
            && ext != want_ext { continue; }

        let path_str = path.to_string_lossy();
        if let Some(prefix) = &filters.starts_with
            && !file_name_matches(path, |n| n.starts_with(prefix)) { continue; }
        if let Some(needle) = &filters.contains
            && !path_str.contains(needle) { continue; }
        if let Some(suffix) = &filters.ends_with
            && !file_name_matches(path, |n| n.ends_with(suffix)) { continue; }
        if let Some(re) = &regex
            && !re.is_match(&path_str) { continue; }

        matched.push((path.clone(), ext.to_string()));
    }

    matched.sort();

    match mode {
        OutputMode::Paths => {
            for (path, _) in &matched {
                println!("{}", path.display());
            }
        }
        OutputMode::Summary { sort_by_count } => {
            // Tally by extension only — for standalone tag files the
            // extension is the group, so the GROUP/EXTENSION split
            // the old per-file-read implementation produced was
            // always redundant.
            let mut counts: BTreeMap<String, u64> = BTreeMap::new();
            for (_, ext) in &matched {
                *counts.entry(ext.clone()).or_insert(0) += 1;
            }
            let mut rows: Vec<_> = counts.into_iter().collect();
            if sort_by_count {
                rows.sort_by(|a, b| b.1.cmp(&a.1));
            }
            println!("{:<32} {:>8}", "GROUP", "COUNT");
            println!("{}", "-".repeat(44));
            let mut total = 0u64;
            for (group, count) in &rows {
                println!("{:<32} {:>8}", group, count);
                total += count;
            }
            println!("{}", "-".repeat(44));
            println!("{:<32} {:>8}", format!("{} types", rows.len()), total);
        }
        OutputMode::Json => {
            let arr: Vec<_> = matched.iter()
                .map(|(path, group)| json!({ "path": path.to_string_lossy(), "group": group }))
                .collect();
            println!("{}", serde_json::to_string_pretty(&arr)?);
        }
    }

    Ok(())
}

/// Normalise `--group <X>` to a tag-file extension (which equals the
/// long group name for standalone tags). Accepts either form:
///
/// - long name (`"render_model"`) → returned verbatim if the index
///   knows it, otherwise also accepted (lets users filter for
///   experimental/unindexed extensions).
/// - 4-byte group tag (`"mode"`) → looked up in the tag index and
///   translated to its long name.
fn resolve_group(ctx: &CliContext, raw: &str) -> Result<String> {
    if ctx.tag_index.group_tag_for(raw).is_some() {
        return Ok(raw.to_string());
    }
    if raw.len() == 4
        && let Some(tag) = parse_group_tag(raw)
        && let Some(name) = ctx.tag_index.name_for(tag)
    {
        return Ok(name.to_string());
    }
    // Unknown to the index — fall through and match it as-is. Lets
    // a user filter for `.foo` even when `foo` isn't a registered
    // group, instead of erroring out.
    Ok(raw.to_string())
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.is_dir() { return Ok(()); }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out)?;
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && name == ".DS_Store" { continue; }
        out.push(path);
    }
    Ok(())
}

fn load_from_file(path: Option<&str>) -> Result<Option<Vec<PathBuf>>> {
    let Some(path) = path else { return Ok(None) };
    let file = File::open(path).with_context(|| format!("failed to open --from-file {}", path))?;
    let lines: Result<Vec<_>, _> = BufReader::new(file).lines().collect();
    Ok(Some(lines?.into_iter().map(PathBuf::from).collect()))
}

fn file_name_matches(path: &Path, f: impl FnOnce(&str) -> bool) -> bool {
    path.file_name().and_then(|n| n.to_str()).map(f).unwrap_or(false)
}
