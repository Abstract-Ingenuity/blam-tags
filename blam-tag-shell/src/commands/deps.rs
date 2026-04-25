//! `deps` — list every tag_reference in a tag. The raw building
//! block for dependency analysis: `check --tags-root` uses the same
//! visitor pattern to validate each reference resolves on disk, and
//! downstream tooling can feed `deps --json` into a tag-graph
//! builder. Honours the REPL's `nav` stack so an `edit-block` into a
//! subtree scopes the listing.

use std::collections::BTreeSet;

use anyhow::{Context, Result};
use blam_tags::{TagField, TagFieldData, TagReferenceData};
use serde_json::json;

use crate::context::CliContext;
use crate::format::write_tag_reference;
use crate::walk::{walk, FieldVisitor};

pub fn run(ctx: &mut CliContext, unique: bool, json_output: bool) -> Result<()> {
    let nav_path = ctx.nav.join("/");
    let loaded = ctx.loaded.as_ref().context("`deps` needs a loaded tag")?;

    let start = if nav_path.is_empty() {
        loaded.tag.root()
    } else {
        loaded
            .tag
            .root()
            .descend(&nav_path)
            .with_context(|| format!("nav path '{}' does not resolve to a struct", nav_path))?
    };

    let mut visitor = DepsVisitor { ctx, refs: Vec::new() };
    walk(start, &mut visitor);

    let mut refs = visitor.refs;
    if unique {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        refs.retain(|(_, rendered)| seen.insert(rendered.clone()));
    }

    if json_output {
        let arr: Vec<_> = refs.iter()
            .map(|(field_path, rendered)| json!({
                "field_path": field_path,
                "reference": rendered,
            }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else {
        for (field_path, rendered) in refs {
            println!("{field_path}: {rendered}");
        }
    }

    Ok(())
}

struct DepsVisitor<'a> {
    ctx: &'a CliContext,
    refs: Vec<(String, String)>,
}

impl FieldVisitor for DepsVisitor<'_> {
    fn visit_leaf(&mut self, path: &str, _depth: usize, field: TagField<'_>) {
        let Some(TagFieldData::TagReference(r)) = field.value() else { return };
        if r.group_tag_and_name.is_none() { return; }
        let mut rendered = String::new();
        write_tag_reference(
            self.ctx,
            &mut rendered,
            &TagReferenceData { group_tag_and_name: r.group_tag_and_name.clone() },
        );
        self.refs.push((path.to_string(), rendered));
    }
}
