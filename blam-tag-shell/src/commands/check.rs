//! `check` — integrity validator.
//!
//! Walks every field and reports the issues that are cheap to detect
//! without extra schema plumbing:
//!
//! - **Enum out of range** — the stored int didn't resolve to a
//!   named variant. A symptom of schema drift or a hand-edited tag.
//! - **Unknown flag bits** — bits in the mask that aren't covered by
//!   any declared flag name.
//! - **Non-finite reals** — `NaN` or `±inf` in any real/angle/slider
//!   /fraction field. Invariably a bug upstream.
//! - **Missing tag references** — when `--tags-root DIR` is supplied,
//!   we check that each tag_reference points at a file on disk
//!   (any extension). Without `--tags-root` this check is skipped.
//!
//! `--only <kinds>` restricts to a comma-separated list of
//! `enum,flag,real,reference`.
//!
//! `--strict` makes the process exit non-zero on any finding, for
//! CI use.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use blam_tags::{TagField, TagFieldData};
use serde_json::json;

use crate::context::CliContext;
use crate::walk::{walk, FieldVisitor};

/// One category of finding [`run`] can surface. The `--only` flag
/// narrows the report to a subset; default is all four.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CheckKind {
    /// Enum value didn't resolve to a named variant.
    Enum,
    /// Flag bits set without a declared name.
    Flag,
    /// Non-finite (`NaN` / `±inf`) real value.
    Real,
    /// Tag reference that doesn't resolve to a file under the
    /// supplied tags root.
    Reference,
}

impl CheckKind {
    fn all() -> HashSet<CheckKind> {
        [CheckKind::Enum, CheckKind::Flag, CheckKind::Real, CheckKind::Reference]
            .into_iter().collect()
    }

    fn parse(s: &str) -> Result<CheckKind> {
        Ok(match s.trim() {
            "enum" | "enums" => CheckKind::Enum,
            "flag" | "flags" => CheckKind::Flag,
            "real" | "reals" => CheckKind::Real,
            "reference" | "references" | "ref" | "refs" => CheckKind::Reference,
            other => anyhow::bail!("unknown check kind '{other}' (expected: enum, flag, real, reference)"),
        })
    }

    fn label(self) -> &'static str {
        match self {
            CheckKind::Enum => "enum",
            CheckKind::Flag => "flag",
            CheckKind::Real => "real",
            CheckKind::Reference => "reference",
        }
    }
}

fn parse_only(raw: Option<&str>) -> Result<HashSet<CheckKind>> {
    let Some(raw) = raw else { return Ok(CheckKind::all()) };
    raw.split(',').map(CheckKind::parse).collect()
}

pub fn run(
    ctx: &mut CliContext,
    tags_root: Option<&str>,
    only: Option<&str>,
    json_output: bool,
    strict: bool,
) -> Result<()> {
    let kinds = parse_only(only)?;
    let tags_root = tags_root.map(PathBuf::from);

    let loaded = ctx.loaded("check")?;
    let root = loaded.tag.root();

    let mut visitor = CheckVisitor {
        enabled: &kinds,
        findings: Vec::new(),
    };
    walk(root, &mut visitor);
    let mut findings = visitor.findings;

    // Reference disk-existence is cheaper as a single post-pass: we
    // can dedupe candidates (many refs point at the same target) and
    // share a small `read_dir` cache across them.
    if kinds.contains(&CheckKind::Reference)
        && let Some(root) = &tags_root {
            check_references_on_disk(&loaded.tag, root, &mut findings);
        }

    emit(&findings, json_output)?;

    if strict && !findings.is_empty() {
        anyhow::bail!("{} finding(s)", findings.len());
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct Finding {
    path: String,
    kind: CheckKind,
    detail: String,
}

struct CheckVisitor<'a> {
    enabled: &'a HashSet<CheckKind>,
    findings: Vec<Finding>,
}

impl<'a> FieldVisitor for CheckVisitor<'a> {
    fn visit_leaf(&mut self, path: &str, _depth: usize, field: TagField<'_>) {
        let Some(value) = field.value() else { return };

        match &value {
            TagFieldData::CharEnum { value: v, name: None }
                if self.enabled.contains(&CheckKind::Enum) =>
            {
                self.findings.push(Finding {
                    path: path.to_string(),
                    kind: CheckKind::Enum,
                    detail: format!("{v} is not a declared variant"),
                });
            }
            TagFieldData::ShortEnum { value: v, name: None }
                if self.enabled.contains(&CheckKind::Enum) =>
            {
                self.findings.push(Finding {
                    path: path.to_string(),
                    kind: CheckKind::Enum,
                    detail: format!("{v} is not a declared variant"),
                });
            }
            TagFieldData::LongEnum { value: v, name: None }
                if self.enabled.contains(&CheckKind::Enum) =>
            {
                self.findings.push(Finding {
                    path: path.to_string(),
                    kind: CheckKind::Enum,
                    detail: format!("{v} is not a declared variant"),
                });
            }

            TagFieldData::ByteFlags { value: v, names }
                if self.enabled.contains(&CheckKind::Flag) =>
            {
                if let Some(extra) = extra_bits(*v as u64, names) {
                    self.findings.push(Finding {
                        path: path.to_string(),
                        kind: CheckKind::Flag,
                        detail: format!("bits 0x{:02X} set without a declared name", extra),
                    });
                }
            }
            TagFieldData::WordFlags { value: v, names }
                if self.enabled.contains(&CheckKind::Flag) =>
            {
                if let Some(extra) = extra_bits(*v as u64, names) {
                    self.findings.push(Finding {
                        path: path.to_string(),
                        kind: CheckKind::Flag,
                        detail: format!("bits 0x{:04X} set without a declared name", extra),
                    });
                }
            }
            TagFieldData::LongFlags { value: v, names }
                if self.enabled.contains(&CheckKind::Flag) =>
            {
                if let Some(extra) = extra_bits(*v as u32 as u64, names) {
                    self.findings.push(Finding {
                        path: path.to_string(),
                        kind: CheckKind::Flag,
                        detail: format!("bits 0x{:08X} set without a declared name", extra),
                    });
                }
            }

            TagFieldData::Angle(f)
            | TagFieldData::Real(f)
            | TagFieldData::RealSlider(f)
            | TagFieldData::RealFraction(f)
                if self.enabled.contains(&CheckKind::Real) && !f.is_finite() =>
            {
                self.findings.push(Finding {
                    path: path.to_string(),
                    kind: CheckKind::Real,
                    detail: format!("value is {}", f),
                });
            }

            _ => {}
        }
    }
}

/// Return the bits that are set in `value` but not covered by any
/// declared name, or `None` if every set bit has a name.
fn extra_bits(value: u64, names: &[(u32, String)]) -> Option<u64> {
    let declared: u64 = names.iter().map(|(bit, _)| 1u64 << bit).fold(0, |a, b| a | b);
    let extra = value & !declared;
    (extra != 0).then_some(extra)
}

/// Reference-existence pass. The library stores references as
/// `group:backslash\path` with no extension, but on disk the file has
/// a descriptive extension (`.biped`, `.model_animation_graph`, …)
/// not the 4-byte group tag. Rather than maintain a group → extension
/// map, we check that *some* file with the expected stem exists under
/// its parent directory. False negatives only happen if the tag's
/// naming diverges from standard conventions.
fn check_references_on_disk(
    tag: &blam_tags::TagFile,
    tags_root: &Path,
    findings: &mut Vec<Finding>,
) {
    struct RefCollector {
        refs: Vec<(String, String)>,
    }
    impl FieldVisitor for RefCollector {
        fn visit_leaf(&mut self, path: &str, _depth: usize, field: TagField<'_>) {
            if let Some(TagFieldData::TagReference(r)) = field.value()
                && let Some((_group, p)) = r.group_tag_and_name {
                    self.refs.push((path.to_string(), p));
                }
        }
    }
    let mut collector = RefCollector { refs: Vec::new() };
    walk(tag.root(), &mut collector);

    let mut seen_missing: BTreeSet<String> = BTreeSet::new();
    for (field_path, stem_raw) in collector.refs {
        // Normalize Halo's `\` separator.
        let rel: PathBuf = stem_raw.split('\\').collect();
        let abs = tags_root.join(&rel);
        let parent = abs.parent().unwrap_or(tags_root);
        let stem = abs.file_name().and_then(|n| n.to_str()).unwrap_or("");

        let exists = stem_exists(parent, stem);
        if !exists && seen_missing.insert(stem_raw.clone()) {
            findings.push(Finding {
                path: field_path,
                kind: CheckKind::Reference,
                detail: format!("no file with stem '{stem_raw}' under {}", tags_root.display()),
            });
        }
    }
}

/// Any file in `parent` whose filename begins with `stem.` (i.e. the
/// stem followed by an extension).
fn stem_exists(parent: &Path, stem: &str) -> bool {
    let Ok(entries) = fs::read_dir(parent) else { return false };
    let needle = format!("{stem}.");
    entries.flatten().any(|e| {
        e.file_name()
            .to_str()
            .map(|n| n.starts_with(&needle))
            .unwrap_or(false)
    })
}

fn emit(findings: &[Finding], json_output: bool) -> Result<()> {
    if json_output {
        let arr: Vec<_> = findings
            .iter()
            .map(|f| json!({
                "path": f.path,
                "kind": f.kind.label(),
                "detail": f.detail,
            }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr).context("json serialize")?);
    } else if findings.is_empty() {
        println!("clean — no issues found");
    } else {
        for f in findings {
            println!("[{}] {}: {}", f.kind.label(), f.path, f.detail);
        }
        println!();
        println!("{} finding(s)", findings.len());
    }
    Ok(())
}
