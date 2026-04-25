//! `inspect` — the workhorse tree view. Walks structs/blocks/arrays
//! under `path` (or the current REPL nav, if no path) to a bounded
//! `--depth`, with name / value filters. Text mode goes through the
//! shared [`walk`] visitor; JSON mode stays bespoke because its
//! naturally-recursive output shape is awkward to emit through the
//! flat visitor protocol.

use anyhow::{Context, Result};
use blam_tags::{TagArray, TagBlock, TagField, TagStruct};
use serde_json::{json, Value};

use crate::context::CliContext;
use crate::format::{format_value, value_to_json};
use crate::walk::{walk, FieldVisitor, VisitControl};

/// Name / value filtering options accepted by the `inspect` subcommand.
/// An empty [`InspectFilters`] matches every field.
pub struct InspectFilters {
    pub names: Vec<String>,
    pub excludes: Vec<String>,
    pub value: Option<String>,
}

impl InspectFilters {
    fn is_active(&self) -> bool {
        !self.names.is_empty() || !self.excludes.is_empty() || self.value.is_some()
    }

    /// Leaf-level match. Containers aren't filtered (they recurse
    /// regardless so descendants can match).
    fn leaf_matches(&self, name: &str, formatted: Option<&str>) -> bool {
        if !self.names.is_empty() && !self.names.iter().any(|s| name.contains(s)) {
            return false;
        }
        if self.excludes.iter().any(|s| name.contains(s)) {
            return false;
        }
        if let Some(needle) = &self.value {
            let Some(f) = formatted else { return false };
            if !f.contains(needle) {
                return false;
            }
        }
        true
    }
}

pub fn run(
    ctx: &mut CliContext,
    path: Option<&str>,
    depth: usize,
    show_all: bool,
    expand_blocks: bool,
    json_output: bool,
    filters: InspectFilters,
) -> Result<()> {
    let nav_path = ctx.nav.join("/");
    let resolved = path.map(|p| ctx.resolve_path(p));
    let loaded = ctx.loaded.as_ref().context("`inspect` needs a loaded tag")?;
    let root = loaded.tag.root();

    // Two cases:
    //  - No path arg: inspect the struct at the current nav position.
    //    Use `descend` so a nav like `seats[0]` lands inside that
    //    specific element rather than dumping all block elements.
    //  - With path arg: inspect that specific field, interpreting
    //    block/array endpoints as the container itself (so
    //    `inspect seats` dumps the block).
    if resolved.is_none() {
        let target = if nav_path.is_empty() {
            root
        } else {
            root.descend(&nav_path)
                .with_context(|| format!("nav path '{}' does not resolve to a struct", nav_path))?
        };
        if json_output {
            println!("{}", serde_json::to_string_pretty(&struct_to_json(ctx, target, depth, &filters, show_all, expand_blocks))?);
        } else {
            print_via_walker(ctx, target, depth, &filters, show_all, expand_blocks);
        }
        return Ok(());
    }

    let resolved = resolved.unwrap();
    let p = resolved.as_str();

    // Trailing `[N]` selects an element; descend straight into that
    // element's struct so `inspect block[0]` drills in regardless of
    // `--full`. Without a trailing index, fall back to the
    // field-as-target dispatch (so `inspect block` still shows
    // count + descendants per the `--full` rule).
    if p.ends_with(']')
        && let Some(target) = root.descend(p)
    {
        if json_output {
            println!("{}", serde_json::to_string_pretty(&struct_to_json(ctx, target, depth, &filters, show_all, expand_blocks))?);
        } else {
            print_via_walker(ctx, target, depth, &filters, show_all, expand_blocks);
        }
        return Ok(());
    }

    let field = root.field_path(p).with_context(|| format!("field '{}' not found", p))?;

    if let Some(nested) = field.as_struct() {
        if json_output {
            println!("{}", serde_json::to_string_pretty(&struct_to_json(ctx, nested, depth, &filters, show_all, expand_blocks))?);
        } else {
            print_via_walker(ctx, nested, depth, &filters, show_all, expand_blocks);
        }
    } else if let Some(block) = field.as_block() {
        print_block(ctx, block, p, depth, json_output, &filters, show_all, expand_blocks)?;
    } else if let Some(array) = field.as_array() {
        print_array(ctx, array, p, depth, json_output, &filters, show_all, expand_blocks)?;
    } else {
        anyhow::bail!("field '{}' is not a struct, block, or array", p);
    }

    Ok(())
}

/// Text-mode tree walker. Uses the shared [`FieldVisitor`]
/// infrastructure — depth limiting comes from walker-provided
/// depth plus the user's `--depth` cap, indent is derived from
/// depth.
fn print_via_walker(ctx: &CliContext, start: TagStruct<'_>, max_depth: usize, filters: &InspectFilters, show_all: bool, expand_blocks: bool) {
    let mut visitor = InspectText {
        ctx,
        max_depth,
        filters,
        show_all,
        expand_blocks,
    };
    walk(start, &mut visitor);
}

struct InspectText<'a> {
    ctx: &'a CliContext,
    max_depth: usize,
    filters: &'a InspectFilters,
    show_all: bool,
    /// When false, `enter_block` prints the count line and stops
    /// (does not descend into elements). Arrays always descend
    /// regardless — they're fixed-count from the schema.
    expand_blocks: bool,
}

impl<'a> InspectText<'a> {
    fn indent(&self, depth: usize) -> String {
        " ".repeat(depth * 2)
    }

    /// If `elem` is an inline-able single-leaf element, print the
    /// `[i] name: type = value` line at indent `depth` and return
    /// `true` (consuming the element). Otherwise return `false` and
    /// leave printing to the caller's normal multi-line path.
    ///
    /// Filter rejection is also a "handled" outcome — we want
    /// silent skip in that case rather than falling through and
    /// printing the bare `[i]` header.
    fn try_inline_element(&self, depth: usize, index: usize, elem: TagStruct<'_>) -> bool {
        if self.show_all {
            return false;
        }
        let mut iter = elem.fields();
        let only = match (iter.next(), iter.next()) {
            (Some(only), None) if only.value().is_some() => only,
            _ => return false,
        };
        let value = only.value().unwrap();
        let formatted = format_value(self.ctx, &value, false);
        let name = only.name();
        if !self.filters.is_active() || self.filters.leaf_matches(name, Some(&formatted)) {
            println!("{}[{index}] {name}: {type_name} = {formatted}",
                self.indent(depth),
                type_name = only.type_name());
        }
        true
    }
}

impl<'a> FieldVisitor for InspectText<'a> {
    fn include_padding(&self) -> bool {
        self.show_all
    }

    fn enter_struct(&mut self, _path: &str, depth: usize, field: TagField<'_>) -> VisitControl {
        if !self.filters.is_active() {
            println!("{}{}: struct", self.indent(depth), field.name());
        }
        if depth < self.max_depth { VisitControl::Descend } else { VisitControl::Skip }
    }

    fn enter_block(&mut self, _path: &str, depth: usize, field: TagField<'_>, block: TagBlock<'_>) -> VisitControl {
        if !self.filters.is_active() {
            println!("{}{}: block [{} elements]", self.indent(depth), field.name(), block.len());
        }
        if !self.expand_blocks {
            return VisitControl::Skip;
        }
        if depth < self.max_depth { VisitControl::Descend } else { VisitControl::Skip }
    }

    fn enter_array(&mut self, _path: &str, depth: usize, field: TagField<'_>, array: TagArray<'_>) -> VisitControl {
        if !self.filters.is_active() {
            println!("{}{}: array [{} elements]", self.indent(depth), field.name(), array.len());
        }
        if depth < self.max_depth { VisitControl::Descend } else { VisitControl::Skip }
    }

    fn enter_element(&mut self, _path: &str, depth: usize, index: usize, elem: TagStruct<'_>) -> VisitControl {
        // If the element collapses to a single-leaf inline line,
        // print it and skip recursion. This is the common case for
        // spherical-harmonic / coefficient arrays where each element
        // is a one-field struct, and the vertical form chews up
        // screen space without adding information.
        if self.try_inline_element(depth, index, elem) {
            return VisitControl::Skip;
        }

        if !self.filters.is_active() {
            // depth here is the element's depth (+1 past the enclosing
            // block/array), so subtract one to align the bracket with
            // the container's child column.
            println!("{}[{index}]", self.indent(depth.saturating_sub(1)));
        }
        VisitControl::Descend
    }

    fn visit_resource(&mut self, _path: &str, depth: usize, field: TagField<'_>) {
        if !self.filters.is_active() || self.filters.leaf_matches(field.name(), None) {
            println!("{}{}: pageable_resource", self.indent(depth), field.name());
        }
    }

    fn visit_leaf(&mut self, _path: &str, depth: usize, field: TagField<'_>) {
        let name = field.name();
        let type_name = field.type_name();
        match field.value() {
            Some(value) => {
                let formatted = format_value(self.ctx, &value, false);
                if !self.filters.is_active() || self.filters.leaf_matches(name, Some(&formatted)) {
                    println!("{}{name}: {type_name} = {formatted}", self.indent(depth));
                }
            }
            None => {
                if !self.filters.is_active() || self.filters.leaf_matches(name, None) {
                    println!("{}{name}: {type_name}", self.indent(depth));
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn print_block(ctx: &CliContext, block: TagBlock<'_>, label: &str, depth: usize, json_output: bool, filters: &InspectFilters, show_all: bool, expand_blocks: bool) -> Result<()> {
    if json_output {
        if expand_blocks {
            let values: Vec<Value> = block.iter().map(|s| struct_to_json(ctx, s, depth, filters, show_all, expand_blocks)).collect();
            println!("{}", serde_json::to_string_pretty(&values)?);
        } else {
            println!("{}", serde_json::to_string_pretty(&json!({
                "kind": "block",
                "count": block.len(),
            }))?);
        }
    } else {
        println!("{}: block [{} elements]", label, block.len());
        if expand_blocks {
            for (i, s) in block.iter().enumerate() {
                let mut v = InspectText { ctx, max_depth: depth, filters, show_all, expand_blocks };
                if !v.try_inline_element(1, i, s) {
                    println!("  [{}]", i);
                    walk(s, &mut v);
                }
            }
        } else {
            println!("  (pass --full to expand, or inspect a single element with `{}[<index>]`)", label);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn print_array(ctx: &CliContext, array: TagArray<'_>, label: &str, depth: usize, json_output: bool, filters: &InspectFilters, show_all: bool, expand_blocks: bool) -> Result<()> {
    // Arrays are fixed-count from the schema, so they're not gated
    // by `--full` — they're always expanded.
    if json_output {
        let values: Vec<Value> = array.iter().map(|s| struct_to_json(ctx, s, depth, filters, show_all, expand_blocks)).collect();
        println!("{}", serde_json::to_string_pretty(&values)?);
    } else {
        println!("{}: array [{} elements]", label, array.len());
        for (i, s) in array.iter().enumerate() {
            let mut v = InspectText { ctx, max_depth: depth, filters, show_all, expand_blocks };
            if !v.try_inline_element(1, i, s) {
                println!("  [{}]", i);
                walk(s, &mut v);
            }
        }
    }
    Ok(())
}

//================================================================================
// JSON mode stays bespoke — its output shape is naturally a
// recursive tree, and shoe-horning that through the visitor adds
// more complexity than it removes.
//================================================================================

fn struct_to_json(ctx: &CliContext, s: TagStruct<'_>, depth: usize, filters: &InspectFilters, show_all: bool, expand_blocks: bool) -> Value {
    let iter: Box<dyn Iterator<Item = TagField<'_>>> =
        if show_all { Box::new(s.fields_all()) } else { Box::new(s.fields()) };
    let fields: Vec<Value> = iter
        .filter_map(|field| field_to_json(ctx, field, depth, filters, show_all, expand_blocks))
        .collect();
    json!(fields)
}

fn field_to_json(ctx: &CliContext, field: TagField<'_>, depth: usize, filters: &InspectFilters, show_all: bool, expand_blocks: bool) -> Option<Value> {
    let name = field.name();
    let mut obj = serde_json::Map::new();
    obj.insert("name".into(), json!(name));
    obj.insert("type".into(), json!(field.type_name()));

    if let Some(nested) = field.as_struct() {
        if depth > 0 {
            obj.insert("fields".into(), struct_to_json(ctx, nested, depth - 1, filters, show_all, expand_blocks));
        }
    } else if let Some(block) = field.as_block() {
        obj.insert("count".into(), json!(block.len()));
        if expand_blocks && depth > 0 {
            let elements: Vec<Value> = block.iter().map(|s| struct_to_json(ctx, s, depth - 1, filters, show_all, expand_blocks)).collect();
            obj.insert("elements".into(), json!(elements));
        }
    } else if let Some(array) = field.as_array() {
        // Arrays are fixed-count and not gated by `--full`.
        obj.insert("count".into(), json!(array.len()));
        if depth > 0 {
            let elements: Vec<Value> = array.iter().map(|s| struct_to_json(ctx, s, depth - 1, filters, show_all, expand_blocks)).collect();
            obj.insert("elements".into(), json!(elements));
        }
    } else if field.as_resource().is_some() {
        if filters.is_active() && !filters.leaf_matches(name, None) {
            return None;
        }
        obj.insert("kind".into(), json!("pageable_resource"));
    } else if let Some(value) = field.value() {
        let formatted = format_value(ctx, &value, false);
        if filters.is_active() && !filters.leaf_matches(name, Some(&formatted)) {
            return None;
        }
        obj.insert("value".into(), value_to_json(ctx, &value));
    } else if filters.is_active() && !filters.leaf_matches(name, None) {
        return None;
    }

    Some(Value::Object(obj))
}
