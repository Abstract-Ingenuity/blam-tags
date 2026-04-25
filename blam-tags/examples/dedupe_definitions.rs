//! Strip parent-inherited definitions from each game's tag-schema JSON.
//!
//! For each `<defs_dir>/*.json` (excluding `_meta.json`):
//! - If `parent_tag` is set but its group tag isn't in `_meta.json`,
//!   the field is removed (treated as garbage data — e.g.
//!   `tag_dependency_list`).
//! - Otherwise the file's full ancestor chain is walked and any entry
//!   in the registries (`blocks`, `structs`, `arrays`, `enums_flags`,
//!   `datas`, `resources`, `interops`) whose key is also present in
//!   any ancestor with a structurally equal value is removed from the
//!   child. If the child's value differs from an ancestor's, the tool
//!   aborts so the divergence can be reviewed before re-running.
//!
//! Files are processed leaves-first so we always strip against the
//! *original* parent content (parents haven't been touched yet when we
//! reach their children). Output preserves CRLF line endings, 2-space
//! indent, and the original key ordering.
//!
//! Usage:
//!
//! ```text
//! cargo run --release -p blam-tags --example dedupe_definitions -- \
//!     <DEFS_DIR> [<DEFS_DIR>...]
//! ```

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::error::Error;
use std::path::{Path, PathBuf};

use serde_json::Value;

const REGISTRIES: &[&str] = &[
    "blocks",
    "structs",
    "arrays",
    "enums_flags",
    "datas",
    "resources",
    "interops",
];

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if args.is_empty() {
        eprintln!("usage: dedupe_definitions <DEFS_DIR> [<DEFS_DIR>...]");
        std::process::exit(2);
    }
    for dir in &args {
        process_dir(dir)?;
    }
    Ok(())
}

fn process_dir(defs_dir: &Path) -> Result<(), Box<dyn Error>> {
    println!("== {} ==", defs_dir.display());

    let meta_path = defs_dir.join("_meta.json");
    let meta_bytes = std::fs::read(&meta_path)
        .map_err(|e| format!("read {}: {e}", meta_path.display()))?;
    let meta: Value = serde_json::from_slice(&meta_bytes)?;
    let tag_index: BTreeMap<String, String> = meta
        .get("tag_index")
        .and_then(|v| v.as_object())
        .ok_or("_meta.json missing tag_index")?
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
        .collect();

    let mut files: BTreeMap<String, Value> = BTreeMap::new();
    for entry in std::fs::read_dir(defs_dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(fname) = path.file_name().and_then(|s| s.to_str()) else { continue };
        if fname == "_meta.json" || !fname.ends_with(".json") { continue; }
        let bytes = std::fs::read(&path)?;
        let value: Value = serde_json::from_slice(&bytes)?;
        files.insert(fname.to_owned(), value);
    }

    let mut modified: HashSet<String> = HashSet::new();

    // Drop bogus parent_tag (group tag not in tag_index).
    for (fname, value) in files.iter_mut() {
        let Some(obj) = value.as_object_mut() else { continue };
        let bogus = matches!(
            obj.get("parent_tag").and_then(|v| v.as_str()),
            Some(pt) if !tag_index.contains_key(pt),
        );
        if bogus {
            let was = obj
                .get("parent_tag")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .unwrap_or_default();
            obj.shift_remove("parent_tag");
            modified.insert(fname.clone());
            println!("  dropped bogus parent_tag {was:?} from {fname}");
        }
    }

    // Build child -> parent_filename map.
    let mut parent_of: BTreeMap<String, Option<String>> = BTreeMap::new();
    for (fname, value) in &files {
        let pt = value.get("parent_tag").and_then(|v| v.as_str());
        let parent_file = pt.and_then(|t| tag_index.get(t)).map(|n| format!("{n}.json"));
        if let Some(ref pf) = parent_file
            && !files.contains_key(pf)
        {
            return Err(format!("{fname}: parent_tag points to {pf} which is missing").into());
        }
        parent_of.insert(fname.clone(), parent_file);
    }

    let order = leaves_first_order(&parent_of);

    let mut total_removed = 0usize;
    for fname in order {
        let chain = ancestor_chain(&fname, &parent_of);
        if chain.is_empty() { continue; }

        // Snapshot ancestors before mutating the child entry. Leaves-
        // first guarantees parents are still pristine here.
        let chain_values: Vec<Value> = chain
            .iter()
            .map(|p| files.get(p).cloned().unwrap_or(Value::Null))
            .collect();

        let child = files.get_mut(&fname).unwrap();
        let removed = strip_against_chain(&fname, child, &chain, &chain_values)?;
        if removed > 0 {
            total_removed += removed;
            modified.insert(fname.clone());
            println!("  {fname}: removed {removed}");
        }
    }
    println!("  total entries removed: {total_removed}");

    // Only write files we actually changed — rewriting untouched files
    // would churn unrelated bytes via JSON re-serialization (e.g. raw
    // UTF-8 vs `\uXXXX` escapes).
    for fname in &modified {
        let value = &files[fname];
        let path = defs_dir.join(fname);
        write_pretty_crlf(&path, value)?;
    }
    println!("  files written: {}", modified.len());

    Ok(())
}

/// Topo sort children before parents (leaves first). Falls back to
/// insertion order if a cycle is detected.
fn leaves_first_order(parent_of: &BTreeMap<String, Option<String>>) -> Vec<String> {
    let mut child_count: BTreeMap<String, usize> =
        parent_of.keys().map(|n| (n.clone(), 0)).collect();
    for parent in parent_of.values().flatten() {
        if let Some(c) = child_count.get_mut(parent) {
            *c += 1;
        }
    }
    let mut queue: VecDeque<String> = child_count
        .iter()
        .filter_map(|(n, &c)| (c == 0).then(|| n.clone()))
        .collect();
    let mut out = Vec::with_capacity(parent_of.len());
    let mut visited: BTreeSet<String> = BTreeSet::new();
    while let Some(node) = queue.pop_front() {
        if !visited.insert(node.clone()) { continue; }
        out.push(node.clone());
        if let Some(Some(parent)) = parent_of.get(&node) {
            if let Some(c) = child_count.get_mut(parent) {
                *c = c.saturating_sub(1);
                if *c == 0 { queue.push_back(parent.clone()); }
            }
        }
    }
    if out.len() != parent_of.len() {
        for k in parent_of.keys() {
            if !visited.contains(k) {
                out.push(k.clone());
            }
        }
    }
    out
}

fn ancestor_chain(start: &str, parent_of: &BTreeMap<String, Option<String>>) -> Vec<String> {
    let mut chain = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut cur = parent_of.get(start).and_then(|p| p.clone());
    while let Some(p) = cur {
        if !seen.insert(p.clone()) { break; }
        chain.push(p.clone());
        cur = parent_of.get(&p).and_then(|x| x.clone());
    }
    chain
}

fn strip_against_chain(
    fname: &str,
    child: &mut Value,
    chain: &[String],
    chain_values: &[Value],
) -> Result<usize, Box<dyn Error>> {
    let Some(obj) = child.as_object_mut() else { return Ok(0); };

    let mut removed = 0usize;
    for &reg in REGISTRIES {
        // Snapshot ancestor maps for this registry.
        let ancestor_maps: Vec<(&str, &serde_json::Map<String, Value>)> = chain_values
            .iter()
            .enumerate()
            .filter_map(|(i, av)| {
                av.get(reg)
                    .and_then(|v| v.as_object())
                    .map(|m| (chain[i].as_str(), m))
            })
            .collect();
        if ancestor_maps.is_empty() { continue; }

        let Some(child_map) = obj.get_mut(reg).and_then(|v| v.as_object_mut()) else { continue };

        // Two passes: first verify all matches (or bail), then drop.
        let mut to_drop: Vec<String> = Vec::new();
        for (k, cv) in child_map.iter() {
            for (anc_fname, amap) in &ancestor_maps {
                if let Some(av) = amap.get(k) {
                    if av == cv {
                        to_drop.push(k.clone());
                    } else {
                        return Err(format!(
                            "{fname}: divergent {reg}/{k} vs {anc_fname}; \
                             refusing to strip — investigate before re-running",
                        )
                        .into());
                    }
                    break;
                }
            }
        }

        for k in &to_drop {
            child_map.shift_remove(k);
        }
        removed += to_drop.len();

        // Drop the registry entirely if it's now empty, matching the
        // dumper's habit of omitting empty maps.
        if obj.get(reg).and_then(|v| v.as_object()).is_some_and(|m| m.is_empty()) {
            obj.shift_remove(reg);
        }
    }
    Ok(removed)
}

fn write_pretty_crlf(path: &Path, value: &Value) -> Result<(), Box<dyn Error>> {
    let mut s = serde_json::to_string_pretty(value)?;
    if !s.contains('\r') {
        s = s.replace('\n', "\r\n");
    }
    std::fs::write(path, s)?;
    Ok(())
}
