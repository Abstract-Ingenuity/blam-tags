//! `extract-animation` — decode animations from a `model_animation_graph`
//! tag and dump them. Two output formats:
//!
//! - `--format json` — full per-frame transform table for both
//!   static and animated codec streams; useful for diagnostics.
//! - `--format jma` (default) — JMA-family text file (`.JMM/.JMA/.JMT/
//!   .JMZ/.JMO/.JMR/.JMW`) re-importable by Halo content tooling.
//!   The kind is picked from the animation's `animation type` ×
//!   `frame info type` per Bungie's convention. Movement-bearing
//!   variants (JMA/JMT/JMZ) emit per-frame world-space dx/dy(/dz/dyaw)
//!   composed from the decoded movement data.
//!
//! `<anim>` is optional. When omitted, every animation in the tag is
//! extracted. Otherwise it is an integer index into
//! `definitions/animations[]` or a string-id name.
//!
//! Output layout matches Tool's `model-animations` source-tree
//! convention (`<source-directory>/animations/`) so the result drops
//! straight into an H3EK source tree alongside `extract-jms`'s
//! `render/` output. Files land as
//! `<root>/<tag_stem>/animations/<anim_name>.<EXT>`.
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

use blam_tags::{Animation, AnimationGroup, JmaKind, Skeleton};

use crate::context::CliContext;
use crate::paths::tag_stem;

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
    let animation = Animation::new(&loaded.tag)
        .with_context(|| format!("failed to walk animations in {}", loaded.path.display()))?;

    if animation.is_empty() {
        anyhow::bail!(
            "tag has no local animations (parent: {:?}) — nothing to extract",
            animation.parent(),
        );
    }

    let skeleton = Skeleton::from_tag(&loaded.tag);
    if matches!(format, Format::Jma) && skeleton.is_empty() {
        anyhow::bail!("tag has no skeleton nodes — JMA export needs a skeleton");
    }

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
            Format::Jma => write_jma(group, &clip, &skeleton, &loaded.path, &destinations[i])?,
        }
    }
    Ok(())
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
    tag_path: &Path,
    path: &Path,
) -> Result<()> {
    let kind = jma_kind_for(group);
    let pose = clip.pose(skeleton);

    let actor_name = tag_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unnamedActor")
        .to_owned();

    ensure_parent_dir(path)?;
    let mut writer = BufWriter::new(
        File::create(path).with_context(|| format!("create {}", path.display()))?,
    );
    pose.write_jma(
        &mut writer,
        skeleton,
        group.node_list_checksum,
        kind,
        &actor_name,
        Some(&clip.movement),
    )?;
    writer.flush()?;

    println!(
        "{}: {} frames × {} bones [{}]  movement={:?} ({} frames)",
        path.display(),
        clip.frame_count,
        skeleton.len(),
        kind.extension(),
        clip.movement.kind,
        clip.movement.frames.len(),
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
