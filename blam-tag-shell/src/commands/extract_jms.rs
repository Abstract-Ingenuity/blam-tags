//! `extract-jms` — extract a `.model` (hlmt) tag's render / collision
//! / physics children as JMS files in the source-tree layout the H3
//! Editing Kit expects (`render/`, `collision/`, `physics/`
//! subdirectories under the model's name).
//!
//! Why .model only: the JMS files are per-purpose (render JMS has no
//! collision data; collision/physics JMSes need the skeleton from
//! the render_model for world-space placement). Routing through the
//! .model gives us the full bundle authoritatively, no
//! sibling-discovery heuristic needed.
//!
//! Filters: positional `render` / `collision` / `physics`, multi-
//! select. `all` = explicit "everything". Omitting filters defaults
//! to all. Missing references in the .model are silently skipped
//! with a status note.
//!
//! Layout:
//! ```text
//! <DIR>/<stem>/render/<stem>.JMS
//! <DIR>/<stem>/collision/<stem>.JMS
//! <DIR>/<stem>/physics/<stem>.JMS
//! ```
//! With `--flat`: `<DIR>/<stem>.<kind>.jms` in a single dir.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use blam_tags::{JmsFile, TagFieldData, TagFile};

use crate::context::CliContext;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind { Render, Collision, Physics }

impl Kind {
    fn as_str(self) -> &'static str {
        match self { Self::Render => "render", Self::Collision => "collision", Self::Physics => "physics" }
    }
    fn extension(self) -> &'static str {
        match self {
            Self::Render => "render_model",
            Self::Collision => "collision_model",
            Self::Physics => "physics_model",
        }
    }

    /// Schema field name on `.model` tag — note the inconsistent
    /// underscore on `physics_model` (others use spaces).
    fn model_field(self) -> &'static str {
        match self {
            Self::Render => "render model",
            Self::Collision => "collision model",
            Self::Physics => "physics_model",
        }
    }
}

pub fn run(ctx: &mut CliContext, kinds: &[String], output: Option<&str>, flat: bool) -> Result<()> {
    let loaded = ctx.loaded("extract-jms")?;

    // Reject non-.model input — the whole point of routing through
    // hlmt is to have authoritative refs to all three children.
    let group = loaded.tag.header.group_tag.to_be_bytes();
    if &group != b"hlmt" {
        anyhow::bail!(
            "extract-jms requires a `.model` (hlmt) input — got group `{}`. \
             Tag-direct extraction is available via the library functions \
             `JmsFile::from_render_model` / `from_collision_model_with_skeleton` / \
             `from_physics_model_with_skeleton`.",
            std::str::from_utf8(&group).unwrap_or("?"),
        );
    }

    let selected: HashSet<Kind> = if kinds.is_empty() || kinds.iter().any(|k| k == "all") {
        [Kind::Render, Kind::Collision, Kind::Physics].into_iter().collect()
    } else {
        kinds.iter().filter_map(|k| match k.as_str() {
            "render" => Some(Kind::Render),
            "collision" => Some(Kind::Collision),
            "physics" => Some(Kind::Physics),
            _ => None,
        }).collect()
    };

    let tags_root = derive_tags_root(&loaded.path)
        .context("failed to derive tags root from input path — input must live under a `tags/` directory")?;
    let stem = tag_stem(&loaded.path);
    let out_root = output.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));

    // Resolve all three child refs up-front; we always need to load
    // the render_model when ANY of the three is selected, since it
    // provides the world-space skeleton coll and phmo need.
    let render_path = resolve_child_ref(&loaded.tag, Kind::Render, &tags_root);
    let collision_path = resolve_child_ref(&loaded.tag, Kind::Collision, &tags_root);
    let physics_path = resolve_child_ref(&loaded.tag, Kind::Physics, &tags_root);

    // Load the render_model first to derive the skeleton (used by
    // coll and phmo to place vertices/shapes in world space).
    let render_tag = match &render_path {
        Some(p) => Some(TagFile::read(p)
            .with_context(|| format!("read render_model {}", p.display()))?),
        None => None,
    };
    let render_jms = match &render_tag {
        Some(t) => Some(JmsFile::from_render_model(t)
            .context("build render_model JMS")?),
        None => None,
    };
    let skeleton = render_jms.as_ref().map(|j| j.nodes.as_slice());

    let mut emitted = Vec::new();
    let mut skipped = Vec::new();

    for kind in [Kind::Render, Kind::Collision, Kind::Physics] {
        if !selected.contains(&kind) { continue; }
        let result = match kind {
            Kind::Render => match &render_jms {
                Some(j) => Ok(j.clone()),
                None => Err("no render_model reference"),
            },
            Kind::Collision => match (&collision_path, skeleton) {
                (Some(p), Some(skel)) => {
                    let t = TagFile::read(p)
                        .with_context(|| format!("read collision_model {}", p.display()))?;
                    JmsFile::from_collision_model_with_skeleton(&t, skel)
                        .context("build collision_model JMS")
                        .map_err(|_| "collision_model build failed")
                }
                (Some(_), None) => Err("collision needs render_model for skeleton"),
                (None, _) => Err("no collision_model reference"),
            },
            Kind::Physics => match (&physics_path, skeleton) {
                (Some(p), Some(skel)) => {
                    let t = TagFile::read(p)
                        .with_context(|| format!("read physics_model {}", p.display()))?;
                    JmsFile::from_physics_model_with_skeleton(&t, skel)
                        .context("build physics_model JMS")
                        .map_err(|_| "physics_model build failed")
                }
                (Some(_), None) => Err("physics needs render_model for skeleton"),
                (None, _) => Err("no physics_model reference"),
            },
        };
        match result {
            Ok(jms) => {
                let path = output_path_for(&out_root, &stem, kind, flat);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("create {}", parent.display()))?;
                }
                let mut writer = BufWriter::new(File::create(&path)
                    .with_context(|| format!("create {}", path.display()))?);
                jms.write(&mut writer)?;
                emitted.push((kind, path, jms_summary(&jms)));
            }
            Err(reason) => skipped.push((kind, reason)),
        }
    }

    for (kind, path, summary) in &emitted {
        println!("{}: [{}] {}", path.display(), kind.as_str(), summary);
    }
    for (kind, reason) in &skipped {
        eprintln!("skipped {}: {}", kind.as_str(), reason);
    }
    if emitted.is_empty() {
        anyhow::bail!("nothing emitted — all selected kinds were skipped");
    }
    Ok(())
}

/// Resolve a child tag reference to an absolute path. Returns
/// `None` if the field is missing or the reference is null.
fn resolve_child_ref(tag: &TagFile, kind: Kind, tags_root: &Path) -> Option<PathBuf> {
    let field = tag.root().field(kind.model_field())?;
    let TagFieldData::TagReference(r) = field.value()? else { return None };
    let (_group, rel) = r.group_tag_and_name?;
    if rel.is_empty() { return None; }
    let rel_path: PathBuf = rel.split('\\').collect();
    let mut p = tags_root.join(&rel_path);
    p.set_extension(kind.extension());
    Some(p)
}

/// Find the `tags/` ancestor of `path` and return everything up to
/// and including it. Tag-reference resolution requires this so we
/// can join Halo's relative paths (`objects\...`) onto an absolute
/// root.
fn derive_tags_root(path: &Path) -> Option<PathBuf> {
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

fn output_path_for(out_root: &Path, stem: &str, kind: Kind, flat: bool) -> PathBuf {
    if flat {
        out_root.join(format!("{stem}.{}.jms", kind.as_str()))
    } else {
        out_root.join(stem).join(kind.as_str()).join(format!("{stem}.JMS"))
    }
}

fn tag_stem(path: &Path) -> String {
    path.file_stem().and_then(|s| s.to_str()).unwrap_or("model").to_owned()
}

fn jms_summary(jms: &JmsFile) -> String {
    let mut parts = Vec::new();
    if !jms.nodes.is_empty() { parts.push(format!("{} nodes", jms.nodes.len())); }
    if !jms.materials.is_empty() { parts.push(format!("{} mats", jms.materials.len())); }
    if !jms.markers.is_empty() { parts.push(format!("{} markers", jms.markers.len())); }
    if !jms.vertices.is_empty() { parts.push(format!("{} verts", jms.vertices.len())); }
    if !jms.triangles.is_empty() { parts.push(format!("{} tris", jms.triangles.len())); }
    if !jms.spheres.is_empty() { parts.push(format!("{} spheres", jms.spheres.len())); }
    if !jms.boxes.is_empty() { parts.push(format!("{} boxes", jms.boxes.len())); }
    if !jms.capsules.is_empty() { parts.push(format!("{} capsules", jms.capsules.len())); }
    if !jms.convex_shapes.is_empty() { parts.push(format!("{} convex", jms.convex_shapes.len())); }
    if !jms.ragdolls.is_empty() { parts.push(format!("{} ragdolls", jms.ragdolls.len())); }
    if !jms.hinges.is_empty() { parts.push(format!("{} hinges", jms.hinges.len())); }
    parts.join(", ")
}
