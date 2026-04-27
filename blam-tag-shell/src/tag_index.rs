//! group_tag ↔ group-name lookup loaded from a single game's
//! `_meta.json`.
//!
//! The library never sees full group names — every reference on disk
//! is a 4-byte group tag (`bipd`, `weap`, etc.). Rendering a friendly
//! filename like `objects/elite/elite.biped` is a shell concern, so
//! this module owns the map between the two sides.
//!
//! Loaded eagerly at command start from `definitions/<game>/_meta.json`.
//! No global state — the resulting [`TagIndex`] is stored on the
//! [`crate::context::CliContext`] for the command's lifetime.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use blam_tags::parse_group_tag;
use serde_json::Value;

/// Bidirectional `group_tag` ↔ group-name map. Built once at command
/// start from `definitions/<game>/_meta.json` and stashed on the
/// [`crate::context::CliContext`].
#[derive(Debug, Default)]
pub struct TagIndex {
    name_for_group_tag: BTreeMap<u32, String>,
    group_tag_for_name: BTreeMap<String, u32>,
}

impl TagIndex {
    /// Read `<defs_root>/<game>/_meta.json` and build the index.
    /// Errors loudly if the file is missing or malformed — we want
    /// the operator to know they pointed at the wrong place rather
    /// than silently rendering tag references in the legacy
    /// group-tag-prefixed form.
    pub fn load(defs_root: &Path, game: &str) -> Result<Self> {
        let meta_path = defs_root.join(game).join("_meta.json");
        let bytes = std::fs::read(&meta_path)
            .with_context(|| format!("failed to read {}", meta_path.display()))?;
        let value: Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse {} as JSON", meta_path.display()))?;
        let map = value
            .get("tag_index")
            .and_then(|v| v.as_object())
            .with_context(|| format!("{} missing `tag_index` object", meta_path.display()))?;

        let mut idx = TagIndex::default();
        for (group_tag_str, name_value) in map {
            let Some(name) = name_value.as_str() else { continue };
            let group_tag = parse_group_tag(group_tag_str).with_context(|| {
                format!(
                    "invalid group tag {group_tag_str:?} in {}",
                    meta_path.display(),
                )
            })?;
            idx.name_for_group_tag.insert(group_tag, name.to_owned());
            idx.group_tag_for_name.insert(name.to_owned(), group_tag);
        }
        Ok(idx)
    }

    /// Look up the full group name (e.g. `"biped"`) for a group tag.
    pub fn name_for(&self, group_tag: u32) -> Option<&str> {
        self.name_for_group_tag.get(&group_tag).map(String::as_str)
    }

    /// Look up the group tag for a full group name (e.g. `"biped"`).
    pub fn group_tag_for(&self, name: &str) -> Option<u32> {
        self.group_tag_for_name.get(name).copied()
    }
}
