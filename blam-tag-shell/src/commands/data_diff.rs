//! `data-diff` — compare two tags' *values* at every leaf path.
//!
//! Unlike [`crate::commands::layout_diff`], which compares the two
//! files' schemas (struct layouts, field types, offsets), this walks
//! the tree of live values and reports scalar-level divergences:
//!
//! - `~ path: a -> b` when both tags have a leaf at `path` with
//!   different values,
//! - `- path: a` when only `a` has a leaf at `path` (common when the
//!   tags have different block lengths — the surplus elements of `a`
//!   land here),
//! - `+ path: b` when only `b` has one.
//!
//! Containers (struct / block / array) are *not* compared directly —
//! they're traversed. The visitor's output is the union of every
//! leaf path seen in either walk.
//!
//! `--only <subtree>` restricts both walks to the same subtree of
//! each tag. Useful when comparing just e.g. `unit/unit camera`.
//!
//! `--json` emits an array of `{ path, kind, a, b }` records.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use blam_tags::{TagField, TagFile};
use serde_json::{json, Value};

use crate::format::format_value;
use crate::walk::{walk, FieldVisitor};

pub fn run(
    file_a: &str,
    file_b: &str,
    subtree: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let tag_a = TagFile::read(std::path::Path::new(file_a))
        .map_err(|e| anyhow::anyhow!("failed to read '{file_a}': {e}"))?;
    let tag_b = TagFile::read(std::path::Path::new(file_b))
        .map_err(|e| anyhow::anyhow!("failed to read '{file_b}': {e}"))?;

    let map_a = collect(&tag_a, subtree).with_context(|| format!("walking '{file_a}'"))?;
    let map_b = collect(&tag_b, subtree).with_context(|| format!("walking '{file_b}'"))?;

    let diffs = compute_diffs(&map_a, &map_b);

    if json_output {
        emit_json(&diffs)?;
    } else {
        emit_text(&diffs, file_a, file_b, subtree);
    }

    Ok(())
}

fn collect(tag: &TagFile, subtree: Option<&str>) -> Result<BTreeMap<String, String>> {
    let root = tag.root();
    let start = match subtree {
        None => root,
        Some(s) => root
            .descend(s)
            .with_context(|| format!("subtree '{s}' does not resolve to a struct"))?,
    };

    let mut visitor = CollectVisitor {
        values: BTreeMap::new(),
    };
    walk(start, &mut visitor);
    Ok(visitor.values)
}

struct CollectVisitor {
    values: BTreeMap<String, String>,
}

impl FieldVisitor for CollectVisitor {
    fn visit_leaf(&mut self, path: &str, _depth: usize, field: TagField<'_>) {
        if let Some(value) = field.value() {
            // Skipping unnamed filler (same reasoning as export): the
            // user can't address it in a `set`, so surfacing a delta
            // on it is noise.
            if field.name().is_empty() {
                return;
            }
            self.values.insert(path.to_string(), format_value(&value, false));
        }
    }
}

#[derive(Debug)]
enum DiffKind {
    Changed { a: String, b: String },
    OnlyInA(String),
    OnlyInB(String),
}

fn compute_diffs(
    a: &BTreeMap<String, String>,
    b: &BTreeMap<String, String>,
) -> Vec<(String, DiffKind)> {
    let mut out = Vec::new();
    for (path, va) in a {
        match b.get(path) {
            None => out.push((path.clone(), DiffKind::OnlyInA(va.clone()))),
            Some(vb) if vb != va => out.push((
                path.clone(),
                DiffKind::Changed { a: va.clone(), b: vb.clone() },
            )),
            Some(_) => {}
        }
    }
    for (path, vb) in b {
        if !a.contains_key(path) {
            out.push((path.clone(), DiffKind::OnlyInB(vb.clone())));
        }
    }
    out
}

fn emit_text(diffs: &[(String, DiffKind)], a: &str, b: &str, subtree: Option<&str>) {
    if diffs.is_empty() {
        println!("identical — no differences under {}", subtree.unwrap_or("/"));
        return;
    }
    println!("--- {a}");
    println!("+++ {b}");
    if let Some(s) = subtree {
        println!("subtree: {s}");
    }
    println!();
    let mut n_changed = 0;
    let mut n_only_a = 0;
    let mut n_only_b = 0;
    for (path, kind) in diffs {
        match kind {
            DiffKind::Changed { a, b } => {
                println!("~ {path}: {a} -> {b}");
                n_changed += 1;
            }
            DiffKind::OnlyInA(v) => {
                println!("- {path}: {v}");
                n_only_a += 1;
            }
            DiffKind::OnlyInB(v) => {
                println!("+ {path}: {v}");
                n_only_b += 1;
            }
        }
    }
    println!();
    println!(
        "{} changed, {} only in a, {} only in b",
        n_changed, n_only_a, n_only_b,
    );
}

fn emit_json(diffs: &[(String, DiffKind)]) -> Result<()> {
    let arr: Vec<Value> = diffs
        .iter()
        .map(|(path, kind)| match kind {
            DiffKind::Changed { a, b } => json!({
                "path": path, "kind": "changed", "a": a, "b": b,
            }),
            DiffKind::OnlyInA(v) => json!({
                "path": path, "kind": "only_in_a", "a": v,
            }),
            DiffKind::OnlyInB(v) => json!({
                "path": path, "kind": "only_in_b", "b": v,
            }),
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&arr)?);
    Ok(())
}
