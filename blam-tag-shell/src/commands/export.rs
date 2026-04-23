//! `export` — emit a tag's current state as a sequence of `set`
//! commands that reproduce it.
//!
//! Primarily useful for:
//!   - diffing tag states (`export a > a.cmds; export b > b.cmds; diff a.cmds b.cmds`)
//!   - committing tag edits as reviewable patches
//!   - reproducible tag authoring pipelines
//!
//! Non-settable leaf types (data blobs, math composites, colors,
//! bounds, api-interop, vertex-buffer) are skipped with a comment so
//! callers can see what wasn't captured.

use std::fs::File;
use std::io::{BufWriter, Write};

use anyhow::{Context, Result};
use blam_tags::{TagField, TagFieldData};

use crate::context::CliContext;
use crate::walk::{walk, FieldVisitor};

pub fn run(ctx: &mut CliContext, subtree: Option<&str>, output: Option<&str>) -> Result<()> {
    let loaded = ctx.loaded("export")?;
    let tag_path = loaded.path.to_string_lossy().into_owned();
    let root = loaded.tag.root();

    let start = if let Some(sub) = subtree {
        root.descend(sub)
            .with_context(|| format!("subtree '{}' does not resolve to a struct", sub))?
    } else {
        root
    };

    // Collect lines first so we can route to a file or stdout.
    let mut visitor = ExportVisitor {
        prefix: subtree.map(String::from).unwrap_or_default(),
        tag_path: &tag_path,
        lines: Vec::new(),
        skipped: Vec::new(),
    };
    walk(start, &mut visitor);

    let mut out: Box<dyn Write> = match output {
        Some(path) => Box::new(BufWriter::new(
            File::create(path).with_context(|| format!("failed to open '{}' for writing", path))?,
        )),
        None => Box::new(std::io::stdout().lock()),
    };

    writeln!(out, "# exported from {tag_path}")?;
    if let Some(sub) = subtree {
        writeln!(out, "# subtree: {sub}")?;
    }
    writeln!(out)?;
    for line in &visitor.lines {
        writeln!(out, "{line}")?;
    }
    if !visitor.skipped.is_empty() {
        writeln!(out)?;
        writeln!(out, "# {} field(s) skipped (type not round-trippable via set):", visitor.skipped.len())?;
        for (path, reason) in &visitor.skipped {
            writeln!(out, "#   {path}: {reason}")?;
        }
    }
    out.flush()?;

    Ok(())
}

struct ExportVisitor<'a> {
    prefix: String,
    tag_path: &'a str,
    lines: Vec<String>,
    skipped: Vec<(String, &'static str)>,
}

impl<'a> ExportVisitor<'a> {
    /// `path` from the walker is relative to `start`. When exporting
    /// a subtree, we prepend the user-supplied subtree path so the
    /// emitted commands remain valid against the root tag.
    fn absolute(&self, path: &str) -> String {
        if self.prefix.is_empty() {
            path.to_string()
        } else if path.is_empty() {
            self.prefix.clone()
        } else {
            format!("{}/{}", self.prefix, path)
        }
    }
}

impl<'a> FieldVisitor for ExportVisitor<'a> {
    fn visit_leaf(&mut self, path: &str, _depth: usize, field: TagField<'_>) {
        let Some(value) = field.value() else { return };

        match export_value(&value) {
            Some(v) => {
                let abs = self.absolute(path);
                self.lines.push(format!(
                    "set {file} {path} {value}",
                    file = shell_quote(self.tag_path),
                    path = shell_quote(&abs),
                    value = shell_quote(&v),
                ));
            }
            // Unnamed fields are schema-level filler / engine-computed
            // cells that the user has no way to address in a `set`
            // anyway — excluding them keeps the skipped-report
            // focused on things someone might want to edit but can't
            // yet.
            None if field.name().is_empty() => {}
            None => {
                self.skipped
                    .push((self.absolute(path), non_settable_reason(&value)));
            }
        }
    }
}

/// Render a `TagFieldData` as a string `parse_and_set` will accept,
/// or `None` if the type isn't CLI-settable.
fn export_value(v: &TagFieldData) -> Option<String> {
    use TagFieldData::*;
    match v {
        CharInteger(x) => Some(x.to_string()),
        ShortInteger(x) => Some(x.to_string()),
        LongInteger(x) => Some(x.to_string()),
        Int64Integer(x) => Some(x.to_string()),
        Tag(x) => Some(blam_tags::format_group_tag(*x)),

        Angle(x) | Real(x) | RealSlider(x) | RealFraction(x) => Some(x.to_string()),

        CharEnum { value, .. } => Some(value.to_string()),
        ShortEnum { value, .. } => Some(value.to_string()),
        LongEnum { value, .. } => Some(value.to_string()),

        ByteFlags { value, .. } => Some(format!("0x{:02X}", value)),
        WordFlags { value, .. } => Some(format!("0x{:04X}", value)),
        LongFlags { value, .. } => Some(format!("0x{:08X}", *value as u32)),

        ByteBlockFlags(x) => Some(format!("0x{:02X}", x)),
        WordBlockFlags(x) => Some(format!("0x{:04X}", x)),
        LongBlockFlags(x) => Some(format!("0x{:08X}", *x as u32)),

        CharBlockIndex(x) | CustomCharBlockIndex(x) => Some(block_index(*x as i64)),
        ShortBlockIndex(x) | CustomShortBlockIndex(x) => Some(block_index(*x as i64)),
        LongBlockIndex(x) | CustomLongBlockIndex(x) => Some(block_index(*x as i64)),

        String(s) | LongString(s) => Some(s.clone()),
        StringId(s) | OldStringId(s) => Some(s.string.clone()),

        TagReference(r) => Some(match &r.group_tag_and_name {
            None => "none".into(),
            Some((tag, path)) => format!("{}:{}", blam_tags::format_group_tag(*tag), path),
        }),

        // Anything else isn't round-trippable via `parse_and_set`.
        _ => None,
    }
}

fn block_index(v: i64) -> String {
    if v == -1 { "none".into() } else { v.to_string() }
}

fn non_settable_reason(v: &TagFieldData) -> &'static str {
    use TagFieldData::*;
    match v {
        Data(_) => "data blob",
        Custom(_) => "custom bytes",
        ApiInterop(_) => "runtime handle (use 'set <path> reset' to scrub)",
        Point2d(_) | Rectangle2d(_) | RealPoint2d(_) | RealPoint3d(_)
        | RealVector2d(_) | RealVector3d(_) | RealQuaternion(_)
        | RealEulerAngles2d(_) | RealEulerAngles3d(_)
        | RealPlane2d(_) | RealPlane3d(_) => "math composite",
        RgbColor(_) | ArgbColor(_) | RealRgbColor(_)
        | RealArgbColor(_) | RealHsvColor(_) | RealAhsvColor(_) => "color",
        ShortIntegerBounds(_) | AngleBounds(_) | RealBounds(_) | FractionBounds(_) => "bounds",
        _ => "type not supported by parse_and_set",
    }
}

fn shell_quote(s: &str) -> String {
    shlex::try_quote(s).map(|c| c.into_owned()).unwrap_or_else(|_| format!("\"{}\"", s.replace('"', "\\\"")))
}
