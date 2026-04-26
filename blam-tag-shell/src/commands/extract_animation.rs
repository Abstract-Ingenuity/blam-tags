//! `extract-animation` — decode a single animation from a
//! `model_animation_graph` tag and dump it. Two output formats:
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
//! `<anim>` is either an integer index into `definitions/animations[]`
//! or a string-id name. Omit `--output` to print to stdout (json) or
//! write a default `<tag_stem>.<anim_name>.<EXT>` in cwd (jma).

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::json;

use blam_tags::{Animation, AnimationGroup, JmaKind, Skeleton};

use crate::context::CliContext;

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum Format {
    Jma,
    Json,
}

pub fn run(ctx: &mut CliContext, anim: &str, output: Option<&str>, format: Format) -> Result<()> {
    let loaded = ctx.loaded("extract-animation")?;
    let animation = Animation::new(&loaded.tag)
        .with_context(|| format!("failed to walk animations in {}", loaded.path.display()))?;

    let group = pick_animation(&animation, anim)?;
    let clip = group.decode().with_context(|| format!("decode animation '{anim}'"))?;

    match format {
        Format::Json => write_json(group, &clip, output),
        Format::Jma => {
            let skeleton = Skeleton::from_tag(&loaded.tag);
            write_jma(group, &clip, &skeleton, &loaded.path, output)
        }
    }
}

fn write_json(
    group: &AnimationGroup<'_>,
    clip: &blam_tags::AnimationClip,
    output: Option<&str>,
) -> Result<()> {
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
    let out = json!({
        "name": group.name,
        "index": group.index,
        "frame_count": clip.frame_count,
        "static": tracks_json(&clip.static_tracks),
        "animated": clip.animated_tracks.as_ref().map(tracks_json),
        "animated_status": format!("{:?}", clip.animated_status),
    });
    let json_text = serde_json::to_string_pretty(&out)?;
    match output {
        Some(p) => {
            let path = PathBuf::from(p);
            ensure_parent_dir(&path)?;
            let mut writer = BufWriter::new(File::create(&path)
                .with_context(|| format!("create {}", path.display()))?);
            writer.write_all(json_text.as_bytes())?;
            writer.write_all(b"\n")?;
            writer.flush()?;
            println!("{}: {} frames, animated={:?}", path.display(), clip.frame_count, clip.animated_status);
        }
        None => println!("{json_text}"),
    }
    Ok(())
}

fn write_jma(
    group: &AnimationGroup<'_>,
    clip: &blam_tags::AnimationClip,
    skeleton: &Skeleton,
    tag_path: &std::path::Path,
    output: Option<&str>,
) -> Result<()> {
    if skeleton.is_empty() {
        anyhow::bail!("tag has no skeleton nodes — JMA export needs a skeleton");
    }
    let kind = JmaKind::from_metadata(
        group.animation_type.as_deref(),
        group.frame_info_type.as_deref(),
    );
    let pose = clip.pose(skeleton);

    let actor_name = tag_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unnamedActor")
        .to_owned();

    let path = match output {
        Some(p) => PathBuf::from(p),
        None => default_jma_path(tag_path, group, kind),
    };
    ensure_parent_dir(&path)?;
    let mut writer = BufWriter::new(File::create(&path)
        .with_context(|| format!("create {}", path.display()))?);
    pose.write_jma(
        &mut writer,
        skeleton,
        group.node_list_checksum,
        kind,
        &actor_name,
        Some(&clip.movement),
    )?;

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

fn default_jma_path(tag_path: &std::path::Path, group: &AnimationGroup<'_>, kind: JmaKind) -> PathBuf {
    let stem = tag_path.file_stem().and_then(|s| s.to_str()).unwrap_or("animation");
    let safe_name = group.name.as_deref().map(sanitize).unwrap_or_else(|| format!("anim_{}", group.index));
    PathBuf::from(format!("{stem}.{safe_name}.{}", kind.extension()))
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}

fn ensure_parent_dir(path: &std::path::Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
    }
    Ok(())
}

fn pick_animation<'a, 'b>(
    animation: &'b Animation<'a>,
    anim: &str,
) -> Result<&'b AnimationGroup<'a>> {
    if let Ok(index) = anim.parse::<usize>() {
        return animation.get(index)
            .ok_or_else(|| anyhow::anyhow!(
                "animation index {index} out of range (have {} animations)",
                animation.len(),
            ));
    }
    animation.find(anim)
        .ok_or_else(|| anyhow::anyhow!(
            "no animation named '{anim}' (use `list-animations` to see names)",
        ))
}
