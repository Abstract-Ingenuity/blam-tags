//! Path-related helpers shared across the shell's `extract-*`
//! commands. Tag references are stored as Halo-style backslash
//! relative paths (`objects\characters\masterchief`); resolving them
//! to filesystem paths needs (a) the input's `tags/` ancestor as a
//! root and (b) per-target extension append.

use std::path::{Path, PathBuf};

use blam_tags::TagStruct;

/// Find the `tags/` ancestor of `path` and return everything up to
/// and including it. Returns `None` if `path` doesn't canonicalize
/// or no `tags/` component is found. Tag-reference resolution
/// requires this so we can join Halo's relative paths
/// (`objects\...`) onto an absolute root.
pub fn derive_tags_root(path: &Path) -> Option<PathBuf> {
    let abs = path.canonicalize().ok()?;
    let mut acc = PathBuf::new();
    let mut found = None;
    for component in abs.components() {
        acc.push(component);
        if matches!(component, std::path::Component::Normal(s) if s == "tags") {
            found = Some(acc.clone());
        }
    }
    found
}

/// Extract a tag file's stem (filename without extension) for
/// output-path construction. Falls back to `default` for paths
/// without a usable stem.
pub fn tag_stem(path: &Path, default: &str) -> String {
    path.file_stem().and_then(|s| s.to_str()).unwrap_or(default).to_owned()
}

/// Read a `tag_reference` field's relative path, dropping null/empty
/// references. Thin shell-side wrapper around
/// [`TagStruct::read_tag_ref_path`] that filters out empty results
/// to match the shell's "no usable ref" semantics.
pub fn tag_ref_path(s: &TagStruct<'_>, field: &str) -> Option<String> {
    s.read_tag_ref_path(field).filter(|p| !p.is_empty())
}

/// Resolve a Halo-style relative tag path (`objects\foo\bar`) against
/// an absolute `tags_root` and stamp on the target group's extension.
pub fn resolve_tag_path(tags_root: &Path, rel: &str, ext: &str) -> PathBuf {
    let rel_path: PathBuf = rel.split('\\').collect();
    let mut p = tags_root.join(&rel_path);
    p.set_extension(ext);
    p
}
