//! `find` — deep value search across a directory of tags.
//!
//! Walks every tag under the given root (in parallel via rayon) and
//! prints every field whose formatted value matches the query.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use blam_tags::{format_group_tag, TagField, TagFile};
use rayon::prelude::*;
use regex::Regex;
use serde_json::json;

use crate::context::CliContext;
use crate::format::format_value;
use crate::walk::{walk, FieldVisitor};

pub struct FindFilters {
    pub group: Option<String>,
    pub field_name: Option<String>,
    pub regex: bool,
    pub json: bool,
    pub strict: bool,
}

pub fn run(ctx: &CliContext, dir: &str, query: &str, filters: FindFilters) -> Result<()> {
    let value_re = if filters.regex {
        Some(Regex::new(query).context("invalid regex query")?)
    } else {
        None
    };
    let field_name_re = filters
        .field_name
        .as_deref()
        .map(Regex::new)
        .transpose()
        .context("invalid --field-name regex")?;

    let mut tag_paths = Vec::new();
    crate_walk_dir(Path::new(dir), &mut tag_paths)?;

    let per_tag: Vec<Result<Vec<Hit>>> = tag_paths
        .par_iter()
        .map(|path| search_tag(ctx, path, query, value_re.as_ref(), field_name_re.as_ref(), filters.group.as_deref(), filters.strict))
        .collect();
    let mut hits: Vec<Hit> = Vec::new();
    for r in per_tag {
        hits.extend(r?);
    }

    if filters.json {
        let arr: Vec<_> = hits
            .iter()
            .map(|h| {
                json!({
                    "tag": h.tag.to_string_lossy(),
                    "field": h.field_path,
                    "value": h.value,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else {
        for hit in &hits {
            println!("{} :: {} = {}", hit.tag.display(), hit.field_path, hit.value);
        }
    }

    Ok(())
}

struct Hit {
    tag: PathBuf,
    field_path: String,
    value: String,
}

fn search_tag(
    ctx: &CliContext,
    path: &Path,
    query: &str,
    value_re: Option<&Regex>,
    field_name_re: Option<&Regex>,
    group_filter: Option<&str>,
    strict: bool,
) -> Result<Vec<Hit>> {
    let tag = match TagFile::read(path) {
        Ok(t) => t,
        Err(e) => {
            if strict {
                return Err(anyhow::anyhow!("failed to read '{}': {e}", path.display()));
            }
            return Ok(Vec::new());
        }
    };

    if let Some(g) = group_filter
        && format_group_tag(tag.group().tag) != *g {
            return Ok(Vec::new());
        }

    let mut visitor = FindVisitor {
        ctx,
        query,
        value_re,
        field_name_re,
        tag_path: path,
        hits: Vec::new(),
    };
    walk(tag.root(), &mut visitor);
    Ok(visitor.hits)
}

struct FindVisitor<'a> {
    ctx: &'a CliContext,
    query: &'a str,
    value_re: Option<&'a Regex>,
    field_name_re: Option<&'a Regex>,
    tag_path: &'a Path,
    hits: Vec<Hit>,
}

impl<'a> FieldVisitor for FindVisitor<'a> {
    fn visit_leaf(&mut self, path: &str, _depth: usize, field: TagField<'_>) {
        if let Some(re) = self.field_name_re
            && !re.is_match(field.name()) {
                return;
            }
        let Some(value) = field.value() else { return };
        let formatted = format_value(self.ctx, &value, false);
        let matched = match self.value_re {
            Some(re) => re.is_match(&formatted),
            None => formatted.contains(self.query),
        };
        if matched {
            self.hits.push(Hit {
                tag: self.tag_path.to_path_buf(),
                field_path: path.to_string(),
                value: formatted,
            });
        }
    }
}

fn crate_walk_dir(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            crate_walk_dir(&path, out)?;
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && name == ".DS_Store" {
                continue;
            }
        out.push(path);
    }
    Ok(())
}
