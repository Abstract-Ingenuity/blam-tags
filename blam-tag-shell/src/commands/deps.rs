//! `deps` — list every tag_reference in a tag. The raw building
//! block for dependency analysis: `check --tags-root` uses the same
//! visitor pattern to validate each reference resolves on disk, and
//! downstream tooling can feed `deps --json` into a tag-graph
//! builder. Honours the REPL's `nav` stack so an `edit-block` into a
//! subtree scopes the listing.

use std::collections::BTreeSet;

use anyhow::{Context, Result};
use blam_tags::{TagField, TagFieldData};
use serde_json::json;

use crate::context::CliContext;
use crate::walk::{walk, FieldVisitor};

pub fn run(ctx: &mut CliContext, unique: bool, json_output: bool) -> Result<()> {
    let nav_path = ctx.nav.join("/");
    let loaded = ctx.loaded("deps")?;

    let start = if nav_path.is_empty() {
        loaded.tag.root()
    } else {
        loaded
            .tag
            .root()
            .descend(&nav_path)
            .with_context(|| format!("nav path '{}' does not resolve to a struct", nav_path))?
    };

    let mut visitor = DepsVisitor { refs: Vec::new() };
    walk(start, &mut visitor);

    let mut refs = visitor.refs;
    if unique {
        let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
        refs.retain(|(_, g, p)| seen.insert((g.clone(), p.clone())));
    }

    if json_output {
        let arr: Vec<_> = refs.iter()
            .map(|(field_path, group, path)| json!({
                "field_path": field_path,
                "group": group,
                "path": path,
            }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else {
        for (field_path, group, path) in refs {
            println!("{field_path}: {group}:{path}");
        }
    }

    Ok(())
}

struct DepsVisitor {
    refs: Vec<(String, String, String)>,
}

impl FieldVisitor for DepsVisitor {
    fn visit_leaf(&mut self, path: &str, _depth: usize, field: TagField<'_>) {
        let Some(TagFieldData::TagReference(r)) = field.value() else { return };
        let Some((group_tag, tag_path)) = r.group_tag_and_name else { return };
        self.refs.push((
            path.to_string(),
            blam_tags::format_group_tag(group_tag),
            tag_path,
        ));
    }
}
