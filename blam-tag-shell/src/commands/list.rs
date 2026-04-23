//! `list` — walk a tag directory and emit matching paths.
//!
//! Replaces the old `scan` command. The default output is a simple
//! path list, one per line (raw fodder for scripts). `--summary`
//! re-creates the `scan`-style group/extension tally. `--json` emits
//! structured output.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use blam_tags::TagFile;
use rayon::prelude::*;
use regex::Regex;
use serde_json::json;

pub struct ListFilters {
    pub group: Option<String>,
    pub starts_with: Option<String>,
    pub contains: Option<String>,
    pub ends_with: Option<String>,
    pub regex: Option<String>,
    pub from_file: Option<String>,
    pub strict: bool,
}

pub enum OutputMode {
    Paths,
    Summary { sort_by_count: bool },
    Json,
}

pub fn run(dir: &str, filters: ListFilters, mode: OutputMode) -> Result<()> {
    let regex = filters.regex.as_deref().map(Regex::new).transpose().context("invalid --regex pattern")?;
    let from_file = load_from_file(filters.from_file.as_deref())?;

    let candidates = if let Some(list) = from_file {
        list
    } else {
        let mut paths = Vec::new();
        walk(Path::new(dir), &mut paths)?;
        paths
    };

    // Read each tag's header in parallel, filter, collect. In strict
    // mode we propagate the first read error; otherwise unreadable
    // files are silently skipped (the usual "corpus contains stray
    // non-tag files" case).
    let results: Vec<Result<Option<(PathBuf, String)>>> = candidates
        .par_iter()
        .map(|path| -> Result<Option<(PathBuf, String)>> {
            let tag = match TagFile::read(path) {
                Ok(t) => t,
                Err(e) => {
                    if filters.strict {
                        return Err(anyhow::anyhow!("failed to read '{}': {e}", path.display()));
                    }
                    return Ok(None);
                }
            };
            let group = tag.group().to_string();

            if let Some(g) = &filters.group {
                if group != *g { return Ok(None); }
            }

            let path_str = path.to_string_lossy();
            if let Some(prefix) = &filters.starts_with {
                if !file_name_matches(path, |n| n.starts_with(prefix)) { return Ok(None); }
            }
            if let Some(needle) = &filters.contains {
                if !path_str.contains(needle) { return Ok(None); }
            }
            if let Some(suffix) = &filters.ends_with {
                if !file_name_matches(path, |n| n.ends_with(suffix)) { return Ok(None); }
            }
            if let Some(re) = &regex {
                if !re.is_match(&path_str) { return Ok(None); }
            }

            Ok(Some((path.clone(), group)))
        })
        .collect();

    // Fail fast on the first read error in strict mode; drop skipped
    // entries (Ok(None)) and unwrap the matches.
    let matched: Vec<(PathBuf, String)> = results
        .into_iter()
        .filter_map(|r| r.transpose())
        .collect::<Result<Vec<_>>>()?;

    match mode {
        OutputMode::Paths => {
            for (path, _) in &matched {
                println!("{}", path.display());
            }
        }
        OutputMode::Summary { sort_by_count } => {
            let mut groups: BTreeMap<(String, String), u64> = BTreeMap::new();
            for (path, group) in &matched {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_string();
                *groups.entry((group.clone(), ext)).or_insert(0) += 1;
            }
            let mut rows: Vec<_> = groups.into_iter().map(|((g, e), c)| (g, e, c)).collect();
            if sort_by_count {
                rows.sort_by(|a, b| b.2.cmp(&a.2));
            }
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
        OutputMode::Json => {
            let arr: Vec<_> = matched.iter()
                .map(|(path, group)| json!({ "path": path.to_string_lossy(), "group": group }))
                .collect();
            println!("{}", serde_json::to_string_pretty(&arr)?);
        }
    }

    Ok(())
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
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name == ".DS_Store" { continue; }
        }
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
