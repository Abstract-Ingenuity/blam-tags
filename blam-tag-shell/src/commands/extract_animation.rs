//! `extract-animation` — decode animations from a
//! `.model_animation_graph`, the bundle `.model` (hlmt) that owns
//! one, or any object-inheriting tag (.biped, .vehicle, .scenery,
//! .weapon, .equipment, …) that points at a .model. Two output
//! formats:
//!
//! - `--format json` — full per-frame transform table for both
//!   static and animated codec streams; useful for diagnostics.
//! - `--format jma` (default) — JMA-family text file (`.JMM/.JMA/.JMT/
//!   .JMZ/.JMO/.JMR/.JMW`) re-importable by Halo content tooling.
//!   The kind is picked from the animation's `animation type` ×
//!   `frame info type` × `internal flags / world relative` (JMW = base
//!   + the world-relative bit). Movement deltas are folded into the
//!   root bone — H3 JMA has no separate movement section. See
//!   [`blam_tags::animation::jma`] for the full layout convention.
//!
//! `<anim>` is optional. When omitted, every animation in the tag is
//! extracted. Otherwise it is an integer index into
//! `definitions/animations[]` or a string-id name.
//!
//! Per-bone rest pose source priority:
//!   1. `render_model.nodes[i]/default {translation, rotation}` —
//!      authoritative; used when we can resolve a render_model.
//!   2. `jmad.additional node data[i]` — denormalized cache inside
//!      the jmad. Populated at jmad-build time from the source
//!      render_model. Per the Foundry maintainer there are rare
//!      discrepancies vs the render_model, so we prefer (1) when
//!      available and fall back to (2) per-bone for synthetic nodes
//!      that the render_model lacks (e.g. `camera_control`).
//!   3. Identity — last resort for bones not found in either source.
//!
//! Resolution by input group:
//!   - `.model_animation_graph` (jmad) → only (2) is reachable.
//!   - `.model` (hlmt) → follow `animation` + `render model` refs,
//!     get both sources.
//!   - object-inheriting (biped/scenery/vehicle/weapon/equipment/…)
//!     → follow `model` ref to a hlmt, then the hlmt case.
//!
//! Output layout matches Tool's `model-animations` source-tree
//! convention (`<source-directory>/animations/`) so the result drops
//! straight into an H3EK source tree alongside `extract-jms`'s
//! `render/` output. Files land as
//! `<root>/<jmad_stem>/animations/<anim_name>.<EXT>`.
//!
//! `--output` semantics:
//!   - omitted → `<root>` = `.` (cwd). Single-anim `json` with no
//!     `--output` still prints to stdout for piping.
//!   - ends in a JMA-family extension or `.json` → exact filename
//!     (single-anim only); skips the `<stem>/animations/` nesting.
//!   - any other path → that path becomes `<root>`.
//!
//! `--flat` flattens to `<root>/<tag_stem>.<anim_name>.<EXT>` (no
//! nested subdirs), matching `extract-jms --flat`. Ignored when
//! `--output` is an exact filename — that path is taken verbatim.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::json;

use blam_tags::{Animation, AnimationGroup, JmaKind, NodeTransform, Skeleton, TagFile};

use crate::context::CliContext;
use crate::paths::{derive_tags_root, resolve_tag_path, tag_ref_path, tag_stem};

/// Output format selector for [`run`]. `Jma` writes a JMA-family
/// text file (kind picked from the animation's metadata); `Json`
/// dumps the decoded transforms for diagnostics.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum Format {
    /// Bungie JMA-family text (`.JMM` / `.JMA` / `.JMT` / …).
    Jma,
    /// JSON dump of decoded static + animated tracks.
    Json,
}

pub fn run(
    ctx: &mut CliContext,
    anim: Option<&str>,
    output: Option<&str>,
    flat: bool,
    format: Format,
) -> Result<()> {
    let loaded = ctx.loaded("extract-animation")?;
    let tags_root = derive_tags_root(&loaded.path)
        .context("failed to derive tags root from input path — input must live under a `tags/` directory")?;

    // Resolve the input to an owned (jmad, optional render_model) pair.
    // For a direct jmad input, we don't load a fresh copy — we reuse
    // `loaded.tag` via the `Option::None` branch.
    let resolved = resolve_inputs(&loaded.tag, &tags_root)?;
    let jmad_tag: &TagFile = resolved.jmad.as_ref().unwrap_or(&loaded.tag);
    let render_model: Option<&TagFile> = resolved.render_model.as_ref();

    let animation = Animation::new(jmad_tag)
        .with_context(|| format!("failed to walk animations in {}", loaded.path.display()))?;

    if animation.is_empty() {
        anyhow::bail!(
            "tag has no local animations (parent: {:?}) — nothing to extract",
            animation.parent(),
        );
    }

    let skeleton = Skeleton::from_tag(jmad_tag);
    if matches!(format, Format::Jma) && skeleton.is_empty() {
        anyhow::bail!("jmad has no skeleton nodes — JMA export needs a skeleton");
    }

    // Build per-bone defaults from render_model first (authoritative)
    // and fill gaps with the jmad's `additional node data`. Bones
    // missing from both fall back to identity.
    let defaults = build_defaults(&skeleton, jmad_tag, render_model);

    let target = OutputTarget::from_args(output);
    let stem = tag_stem(&loaded.path, "animation");

    let groups: Vec<&AnimationGroup<'_>> = match anim {
        Some(a) => vec![pick_animation(&animation, a)?],
        None => animation.iter().collect(),
    };

    if matches!(target, OutputTarget::ExactFile(_)) && groups.len() > 1 {
        anyhow::bail!(
            "{} animations selected; --output as a filename only works for a single \
             animation. Pass a directory path or omit --output.",
            groups.len(),
        );
    }

    // Single-anim json with no --output keeps the legacy stdout
    // behavior so callers can pipe into jq.
    let json_to_stdout = matches!(format, Format::Json)
        && matches!(target, OutputTarget::Default)
        && groups.len() == 1;

    // Resolve every destination up front so we can fail loudly on
    // post-sanitize name collisions (e.g. `walk fast` vs `walk-fast`
    // both → `walk_fast.JMA`) instead of silently overwriting.
    let destinations: Vec<PathBuf> = if json_to_stdout {
        Vec::new()
    } else {
        let resolved: Vec<PathBuf> = groups
            .iter()
            .map(|g| resolve_destination(&target, &stem, g, format, flat))
            .collect();
        check_unique_destinations(&resolved, &groups)?;
        resolved
    };

    for (i, group) in groups.iter().enumerate() {
        let clip = group
            .decode()
            .with_context(|| format!("decode animation '{}'", display_name(group)))?;

        match format {
            Format::Json if json_to_stdout => write_json_stdout(group, &clip)?,
            Format::Json => write_json_file(group, &clip, &destinations[i])?,
            Format::Jma => write_jma(group, &clip, &skeleton, &defaults, &stem, &destinations[i])?,
        }
    }
    Ok(())
}

/// Owned tags resolved from the input. Direct-jmad inputs leave
/// `jmad` as `None` and the caller reuses the `loaded.tag` borrow;
/// .model / object-inheriting inputs populate both `jmad` (loaded
/// from the `animation` ref) and optionally `render_model` (from the
/// `render model` ref).
struct ResolvedInputs {
    jmad: Option<TagFile>,
    render_model: Option<TagFile>,
}

/// Dispatch on input group_tag to find the jmad + (optional)
/// render_model. Three accepted shapes:
///   - `jmad` → use loaded tag as-is; no render_model.
///   - `hlmt` → follow `animation` + `render model` refs.
///   - any other tag with a `model` field (object-inheriting bipd /
///     vehi / scen / weap / eqip / …) → follow `model` to a hlmt,
///     then recurse the hlmt case.
fn resolve_inputs(tag: &TagFile, tags_root: &Path) -> Result<ResolvedInputs> {
    let group = tag.header.group_tag.to_be_bytes();
    match &group {
        b"jmad" => Ok(ResolvedInputs { jmad: None, render_model: None }),
        b"hlmt" => resolve_from_model(tag, tags_root),
        _ => {
            let model_rel = find_object_model_ref(tag).with_context(|| format!(
                "input group `{}` has no `model` ref — pass a .model_animation_graph, a .model, \
                 or any object-inheriting tag (.biped, .scenery, .weapon, …)",
                std::str::from_utf8(&group).unwrap_or("?"),
            ))?;
            let model_path = resolve_tag_path(tags_root, &model_rel, "model");
            let model_tag = TagFile::read(&model_path)
                .with_context(|| format!("read .model {}", model_path.display()))?;
            resolve_from_model(&model_tag, tags_root)
        }
    }
}

/// Find the inherited `model` tag_reference on an object-inheriting
/// tag. The path depends on the inheritance chain — every
/// object-inheriting group uses one of these four:
///   - `unit/object/model` — biped, vehicle, giant
///     (extends unit extends object).
///   - `item/object/model` — weapon, equipment
///     (extends item extends object).
///   - `device/object/model` — device_control, device_machine,
///     device_terminal (extends device extends object).
///   - `object/model` — scenery, crate, creature, projectile,
///     effect_scenery, sound_scenery, item, unit, device
///     (extends object directly, or is itself an abstract base).
/// We probe in that order and use the first match.
fn find_object_model_ref(tag: &TagFile) -> Option<String> {
    use blam_tags::TagFieldData;
    const PATHS: &[&str] = &[
        "unit/object/model",
        "item/object/model",
        "device/object/model",
        "object/model",
    ];
    let root = tag.root();
    PATHS.iter().find_map(|p| match root.field_path(p)?.value()? {
        TagFieldData::TagReference(r) => r.group_tag_and_name
            .map(|(_, name)| name)
            .filter(|s| !s.is_empty()),
        _ => None,
    })
}

/// Pull `animation` + `render model` refs off a hlmt tag. The
/// render_model ref may be null/missing on tags that ship without a
/// rendered representation (rare); in that case we drop back to
/// additional_node_data only.
fn resolve_from_model(model_tag: &TagFile, tags_root: &Path) -> Result<ResolvedInputs> {
    let jmad_rel = tag_ref_path(&model_tag.root(), "animation")
        .context("`.model` has no `animation` ref — nothing to extract")?;
    let jmad_path = resolve_tag_path(tags_root, &jmad_rel, "model_animation_graph");
    let jmad = TagFile::read(&jmad_path)
        .with_context(|| format!("read model_animation_graph {}", jmad_path.display()))?;

    let render_model = if let Some(render_rel) = tag_ref_path(&model_tag.root(), "render model") {
        let path = resolve_tag_path(tags_root, &render_rel, "render_model");
        Some(TagFile::read(&path)
            .with_context(|| format!("read render_model {}", path.display()))?)
    } else {
        None
    };

    Ok(ResolvedInputs { jmad: Some(jmad), render_model })
}

/// Build a per-skeleton-bone defaults table. Render_model entries
/// (when supplied) take priority; gaps fall through to the jmad's
/// `additional node data` block; bones absent from both fall back
/// to identity. Per Foundry maintainer: render_model values are
/// authoritative; additional_node_data is a denormalized cache that
/// can drift on rare occasions but is the only source available
/// when extracting from a jmad directly.
fn build_defaults(
    skeleton: &Skeleton,
    jmad: &TagFile,
    render_model: Option<&TagFile>,
) -> Vec<NodeTransform> {
    let mut by_name: HashMap<String, NodeTransform> = HashMap::new();

    // Lower priority first: jmad's `additional node data`.
    if let Some(block) = jmad.root().field_path("additional node data").and_then(|f| f.as_block()) {
        for i in 0..block.len() {
            let Some(elem) = block.element(i) else { continue };
            let Some(name) = elem.read_string_id("node name") else { continue };
            if name.is_empty() { continue; }
            by_name.insert(name, NodeTransform {
                translation: elem.read_point3d("default translation"),
                rotation: elem.read_quat("default rotation"),
                scale: elem.read_real("default scale").unwrap_or(1.0),
            });
        }
    }

    // Higher priority: render_model `nodes[]`. Overwrites the
    // additional_node_data entry when both exist for a bone name.
    if let Some(rm) = render_model
        && let Some(block) = rm.root().field_path("nodes").and_then(|f| f.as_block())
    {
        for i in 0..block.len() {
            let Some(elem) = block.element(i) else { continue };
            let Some(name) = elem.read_string_id("name") else { continue };
            if name.is_empty() { continue; }
            by_name.insert(name, NodeTransform {
                translation: elem.read_point3d("default translation"),
                rotation: elem.read_quat("default rotation"),
                // Render_model's `default scale` is buried inside
                // the inverse matrix per the schema's "Old Mistakes
                // Die Hard" warning. Animation rest poses have
                // scale=1.0 in practice.
                scale: 1.0,
            });
        }
    }

    skeleton.nodes
        .iter()
        .map(|node| by_name.get(&node.name).copied().unwrap_or(NodeTransform::IDENTITY))
        .collect()
}

/// Resolved meaning of the `--output` argument. The CLI is overloaded:
/// the flag can name a source-tree root (default-shaped) or an exact
/// file path that bypasses the source-tree layout entirely.
enum OutputTarget {
    /// `--output <dir>` — a path that becomes the source-tree root.
    /// Files land at `<dir>/<tag_stem>/animations/<anim_name>.<EXT>`,
    /// matching Tool's `model-animations` source-directory convention.
    Root(PathBuf),
    /// `--output <file>` — a path ending in a JMA-family or `.json`
    /// extension. Skips the source-tree layout; single-anim only.
    ExactFile(PathBuf),
    /// `--output` omitted. Equivalent to `Root(".")`.
    Default,
}

impl OutputTarget {
    fn from_args(output: Option<&str>) -> Self {
        let Some(raw) = output else { return Self::Default };
        let path = PathBuf::from(raw);
        let trailing_slash = raw.ends_with('/') || raw.ends_with(std::path::MAIN_SEPARATOR);
        if trailing_slash || path.is_dir() {
            return Self::Root(path);
        }
        if has_known_output_extension(&path) {
            Self::ExactFile(path)
        } else {
            Self::Root(path)
        }
    }
}

/// JMA-family + json extensions that signal "user named an exact
/// file". Anything else (no extension, or some unrelated extension)
/// gets treated as a directory, matching `extract-bitmap`'s rule.
fn has_known_output_extension(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|s| s.to_str()) else { return false };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "jmm" | "jma" | "jmt" | "jmz" | "jmo" | "jmr" | "jmw" | "json",
    )
}

fn resolve_destination(
    target: &OutputTarget,
    stem: &str,
    group: &AnimationGroup<'_>,
    format: Format,
    flat: bool,
) -> PathBuf {
    let ext = match format {
        Format::Json => "json",
        Format::Jma => jma_kind_for(group).extension(),
    };
    let nested_filename = default_filename(group, ext);
    // `--flat` prefixes the stem onto the filename (e.g. multiple
    // tags' anims dropped into the same dir don't collide), matching
    // `extract-jms --flat`'s `<stem>.<kind>.jms` shape.
    let flat_filename = format!("{stem}.{nested_filename}");
    match (target, flat) {
        (OutputTarget::ExactFile(p), _) => p.clone(),
        (OutputTarget::Root(dir), true) => dir.join(flat_filename),
        (OutputTarget::Root(dir), false) => dir.join(stem).join("animations").join(nested_filename),
        (OutputTarget::Default, true) => PathBuf::from(flat_filename),
        (OutputTarget::Default, false) => PathBuf::from(stem).join("animations").join(nested_filename),
    }
}

fn jma_kind_for(group: &AnimationGroup<'_>) -> JmaKind {
    JmaKind::from_metadata(
        group.animation_type.as_deref(),
        group.frame_info_type.as_deref(),
        group.world_relative,
    )
}

/// Bail with a clear listing if any two animations resolved to the
/// same output path. Sanitization (non-alphanumerics → `_`) can fold
/// distinct names together; rather than silently clobber, surface it.
fn check_unique_destinations(
    paths: &[PathBuf],
    groups: &[&AnimationGroup<'_>],
) -> Result<()> {
    let mut seen: HashMap<&Path, usize> = HashMap::with_capacity(paths.len());
    for (i, p) in paths.iter().enumerate() {
        if let Some(&j) = seen.get(p.as_path()) {
            anyhow::bail!(
                "two animations resolve to the same output file `{}`: \
                 [{}] '{}' and [{}] '{}'. Rename one in the tag, or extract them \
                 individually with explicit --output paths.",
                p.display(),
                groups[j].index,
                display_name(groups[j]),
                groups[i].index,
                display_name(groups[i]),
            );
        }
        seen.insert(p.as_path(), i);
    }
    Ok(())
}

fn write_json_stdout(group: &AnimationGroup<'_>, clip: &blam_tags::AnimationClip) -> Result<()> {
    let json_text = serde_json::to_string_pretty(&json_payload(group, clip))?;
    println!("{json_text}");
    Ok(())
}

fn write_json_file(
    group: &AnimationGroup<'_>,
    clip: &blam_tags::AnimationClip,
    path: &Path,
) -> Result<()> {
    let json_text = serde_json::to_string_pretty(&json_payload(group, clip))?;
    ensure_parent_dir(path)?;
    let mut writer = BufWriter::new(
        File::create(path).with_context(|| format!("create {}", path.display()))?,
    );
    writer.write_all(json_text.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!(
        "{}: {} frames, animated={:?}",
        path.display(),
        clip.frame_count,
        clip.animated_status,
    );
    Ok(())
}

fn json_payload(group: &AnimationGroup<'_>, clip: &blam_tags::AnimationClip) -> serde_json::Value {
    let tracks_json = |t: &blam_tags::AnimationTracks| {
        json!({
            "codec": format!("{:?}", t.codec),
            "frame_count": t.frame_count,
            "rotations": t.rotations.iter().map(|frames| {
                frames.iter().map(|q| json!([q.i, q.j, q.k, q.w])).collect::<Vec<_>>()
            }).collect::<Vec<_>>(),
            "translations": t.translations.iter().map(|frames| {
                frames.iter().map(|p| json!([p.x, p.y, p.z])).collect::<Vec<_>>()
            }).collect::<Vec<_>>(),
            "scales": t.scales,
        })
    };
    json!({
        "name": group.name,
        "index": group.index,
        "frame_count": clip.frame_count,
        "static": tracks_json(&clip.static_tracks),
        "animated": clip.animated_tracks.as_ref().map(tracks_json),
        "animated_status": format!("{:?}", clip.animated_status),
    })
}

fn write_jma(
    group: &AnimationGroup<'_>,
    clip: &blam_tags::AnimationClip,
    skeleton: &Skeleton,
    defaults: &[NodeTransform],
    actor_name: &str,
    path: &Path,
) -> Result<()> {
    let kind = jma_kind_for(group);
    // Overlay anims store deltas-from-rest in the codec; unflagged
    // bones must stay at identity so `compose_overlay` (rest × delta)
    // produces the rest pose, not double-rest. Other kinds use the
    // render_model defaults as the rest-pose fallback.
    let pose_defaults: Option<&[NodeTransform]> = if kind.composes_overlay() {
        None
    } else {
        Some(defaults)
    };
    let pose = clip.pose(skeleton, pose_defaults);

    ensure_parent_dir(path)?;
    let mut writer = BufWriter::new(
        File::create(path).with_context(|| format!("create {}", path.display()))?,
    );
    pose.write_jma(
        &mut writer,
        skeleton,
        defaults,
        group.node_list_checksum,
        kind,
        actor_name,
        Some(&clip.movement),
    )?;
    writer.flush()?;

    let codec_count = clip.frame_count;
    let on_disk = codec_count.saturating_add(1);
    println!(
        "{}: {} frames ({}+1) × {} bones [{}]  movement={:?}",
        path.display(),
        on_disk,
        codec_count,
        skeleton.len(),
        kind.extension(),
        clip.movement.kind,
    );
    Ok(())
}

/// `<anim_name>.<ext>` for a group, falling back to `anim_<index>`
/// when the animation has no resolvable string-id name.
fn default_filename(group: &AnimationGroup<'_>, ext: &str) -> String {
    let safe_name = group
        .name
        .as_deref()
        .map(sanitize)
        .unwrap_or_else(|| format!("anim_{}", group.index));
    format!("{safe_name}.{ext}")
}

fn display_name(group: &AnimationGroup<'_>) -> String {
    group
        .name
        .clone()
        .unwrap_or_else(|| format!("[{}]", group.index))
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else { return Ok(()) };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))
}

fn pick_animation<'a, 'b>(
    animation: &'b Animation<'a>,
    anim: &str,
) -> Result<&'b AnimationGroup<'a>> {
    if let Ok(index) = anim.parse::<usize>() {
        return animation.get(index).ok_or_else(|| {
            anyhow::anyhow!(
                "animation index {index} out of range (have {} animations)",
                animation.len(),
            )
        });
    }
    animation.find(anim).ok_or_else(|| {
        anyhow::anyhow!("no animation named '{anim}' (use `list-animations` to see names)")
    })
}
