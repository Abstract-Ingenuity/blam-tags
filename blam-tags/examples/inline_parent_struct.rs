//! Patch each Reach child schema's root struct to inline the parent
//! struct as field [0].
//!
//! The Reach dumper omits the implicit parent struct embed that real
//! `.biped` / `.weapon` / `.shader_*` etc. tags carry on disk (verified
//! against `Blam-Creation-Suite` reach defs). For each group with
//! `parent_tag`, this tool:
//!
//!   1. Reads the child JSON and walks `parent_tag` via `_meta.json`.
//!   2. Resolves the parent's root struct name from the parent JSON's
//!      `blocks[block].struct`.
//!   3. Prepends a synthetic `{type:"struct", name:"<parent_group>",
//!      definition:"<parent_root_struct>"}` to the child's root struct's
//!      `fields` array.
//!   4. Adds the parent's root struct size to the child's root struct
//!      `size` declaration.
//!   5. Writes the file back (preserve key order, 2-space indent, CRLF
//!      — matches the dumper's output format).
//!
//! Files are processed parents-first so each level reads its parent's
//! *post-patch* size and cascading totals end up correct (e.g. biped
//! sees unit's size as `unit_own + object_own`, not just `unit_own`).
//! The patch is idempotent: re-running on an already-patched schema is a
//! no-op (skips when field [0] already references the parent root).
//!
//! Usage:
//!
//! ```text
//! cargo run --release -p blam-tags --example inline_parent_struct -- \
//!     <DEFS_DIR> [<DEFS_DIR>...]
//! ```

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if args.is_empty() {
        eprintln!("usage: inline_parent_struct <DEFS_DIR> [<DEFS_DIR>...]");
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

    // Discover all group JSONs.
    let mut group_files: BTreeMap<String, PathBuf> = BTreeMap::new();
    for entry in std::fs::read_dir(defs_dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(fname) = path.file_name().and_then(|s| s.to_str()) else { continue };
        if fname == "_meta.json" || !fname.ends_with(".json") { continue; }
        let stem = fname.trim_end_matches(".json").to_owned();
        group_files.insert(stem, path);
    }

    // Build child -> parent_group_name map (only for resolvable parents).
    let mut parent_of: BTreeMap<String, Option<String>> = BTreeMap::new();
    for (group, path) in &group_files {
        let bytes = std::fs::read(path)?;
        let value: Value = serde_json::from_slice(&bytes)?;
        let parent_group = value
            .get("parent_tag")
            .and_then(|v| v.as_str())
            .and_then(|t| tag_index.get(t))
            .cloned()
            .filter(|n| group_files.contains_key(n));
        parent_of.insert(group.clone(), parent_group);
    }

    // Topo-sort parents-first so we read each parent's *post-patch* size.
    let order = parents_first_order(&parent_of);

    let mut patched = 0usize;
    let mut skipped_idempotent = 0usize;
    for group in order {
        let Some(parent_group) = parent_of.get(&group).and_then(|p| p.clone()) else {
            continue;
        };
        let group_path = &group_files[&group];
        let parent_path = &group_files[&parent_group];

        let outcome = patch_one(group_path, parent_path, &parent_group)?;
        match outcome {
            PatchOutcome::Patched { parent_root, parent_size, new_size } => {
                println!(
                    "  {}: prepended struct '{}' -> {} (size +{} = {})",
                    group, parent_group, parent_root, parent_size, new_size,
                );
                patched += 1;
            }
            PatchOutcome::AlreadyPatched => {
                skipped_idempotent += 1;
            }
            PatchOutcome::Bail(reason) => {
                println!("  {}: skipped — {}", group, reason);
            }
        }
    }
    println!("  patched: {patched}, already-patched (no-op): {skipped_idempotent}");
    Ok(())
}

enum PatchOutcome {
    Patched { parent_root: String, parent_size: u64, new_size: u64 },
    AlreadyPatched,
    Bail(String),
}

fn patch_one(
    child_path: &Path,
    parent_path: &Path,
    parent_group_name: &str,
) -> Result<PatchOutcome, Box<dyn Error>> {
    let child_bytes = std::fs::read(child_path)?;
    let mut child: Value = serde_json::from_slice(&child_bytes)?;

    let parent_bytes = std::fs::read(parent_path)?;
    let parent: Value = serde_json::from_slice(&parent_bytes)?;

    // Pull the parent's root struct name + size.
    let Some(parent_root_name) = parent
        .get("block")
        .and_then(|v| v.as_str())
        .and_then(|root_block| {
            parent
                .get("blocks")?
                .get(root_block)?
                .get("struct")?
                .as_str()
        })
        .map(str::to_owned)
    else {
        return Ok(PatchOutcome::Bail("parent has no root block/struct".to_owned()));
    };

    let Some(parent_size) = parent
        .get("structs")
        .and_then(|s| s.get(&parent_root_name))
        .and_then(|s| s.get("size"))
        .and_then(|v| v.as_u64())
    else {
        return Ok(PatchOutcome::Bail(format!(
            "parent root struct {parent_root_name} missing size"
        )));
    };

    // Locate the child's root struct.
    let child_root_block = child
        .get("block")
        .and_then(|v| v.as_str())
        .ok_or("child schema missing 'block'")?
        .to_owned();
    let child_root_struct = child
        .get("blocks")
        .and_then(|b| b.get(&child_root_block))
        .and_then(|b| b.get("struct"))
        .and_then(|v| v.as_str())
        .ok_or("child schema's root block has no struct")?
        .to_owned();

    let structs = child
        .get_mut("structs")
        .and_then(|v| v.as_object_mut())
        .ok_or("child schema missing 'structs'")?;
    let root_struct = structs
        .get_mut(&child_root_struct)
        .and_then(|v| v.as_object_mut())
        .ok_or("child root struct not in 'structs'")?;

    // Idempotence: if field [0] is already `struct <parent_root_name>`, no-op.
    let fields = root_struct
        .get_mut("fields")
        .and_then(|v| v.as_array_mut())
        .ok_or("child root struct missing 'fields'")?;
    if let Some(first) = fields.first()
        && first.get("type").and_then(|v| v.as_str()) == Some("struct")
        && first.get("definition").and_then(|v| v.as_str()) == Some(parent_root_name.as_str())
    {
        return Ok(PatchOutcome::AlreadyPatched);
    }

    // Build the synthetic field, preserving key order: type, name, definition.
    let mut synth = serde_json::Map::new();
    synth.insert("type".to_owned(), Value::String("struct".to_owned()));
    synth.insert("name".to_owned(), Value::String(parent_group_name.to_owned()));
    synth.insert("definition".to_owned(), Value::String(parent_root_name.clone()));
    fields.insert(0, Value::Object(synth));

    // Bump the declared size.
    let old_size = root_struct
        .get("size")
        .and_then(|v| v.as_u64())
        .ok_or("child root struct missing 'size'")?;
    let new_size = old_size.saturating_add(parent_size);
    root_struct.insert("size".to_owned(), Value::Number(new_size.into()));

    write_pretty_crlf(child_path, &child)?;

    Ok(PatchOutcome::Patched {
        parent_root: parent_root_name,
        parent_size,
        new_size,
    })
}

fn parents_first_order(parent_of: &BTreeMap<String, Option<String>>) -> Vec<String> {
    // In-degree = 1 if has parent, else 0. Kahn's algorithm.
    let mut indeg: BTreeMap<String, usize> = parent_of
        .iter()
        .map(|(k, p)| (k.clone(), p.as_ref().map(|_| 1).unwrap_or(0)))
        .collect();
    let mut queue: VecDeque<String> = indeg
        .iter()
        .filter_map(|(n, &d)| (d == 0).then(|| n.clone()))
        .collect();

    // Reverse adjacency: parent -> [children].
    let mut children_of: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (child, p) in parent_of {
        if let Some(parent) = p {
            children_of.entry(parent.clone()).or_default().push(child.clone());
        }
    }

    let mut out = Vec::with_capacity(parent_of.len());
    let mut visited: BTreeSet<String> = BTreeSet::new();
    while let Some(node) = queue.pop_front() {
        if !visited.insert(node.clone()) { continue; }
        out.push(node.clone());
        if let Some(kids) = children_of.get(&node) {
            for k in kids {
                if let Some(d) = indeg.get_mut(k) {
                    *d = d.saturating_sub(1);
                    if *d == 0 { queue.push_back(k.clone()); }
                }
            }
        }
    }
    if out.len() != parent_of.len() {
        for k in parent_of.keys() {
            if !visited.contains(k) { out.push(k.clone()); }
        }
    }
    out
}

fn write_pretty_crlf(path: &Path, value: &Value) -> Result<(), Box<dyn Error>> {
    let mut s = serde_json::to_string_pretty(value)?;
    if !s.contains('\r') {
        s = s.replace('\n', "\r\n");
    }
    std::fs::write(path, s)?;
    Ok(())
}
