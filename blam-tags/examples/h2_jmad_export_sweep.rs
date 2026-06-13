//! Full H2 jmad export sweep: decode + pose + write_jma every animation
//! to an in-memory sink, counting successes/failures. Mirrors the shell's
//! extract-animation path (overlay base resolution included).
//! Usage: <defs-dir> <tags-root>

use std::path::PathBuf;
use blam_tags::animation::{Animation, AnimationGraph, JmaKind, NodeTransform, Skeleton};
use blam_tags::classic::read_classic_tag_file;
use blam_tags::layout::TagLayout;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let layout_path = PathBuf::from(&a[1]).join("model_animation_graph.json");
    let mut stack = vec![PathBuf::from(&a[2])];
    let (mut tags, mut exported, mut empty, mut errs) = (0u64, 0u64, 0u64, 0u64);
    let mut err_examples = Vec::new();

    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() { stack.push(p); continue; }
            if p.extension().and_then(|x| x.to_str()) != Some("model_animation_graph") { continue; }
            let Ok(bytes) = std::fs::read(&p) else { continue };
            let Ok(layout) = TagLayout::from_json(&layout_path) else { continue };
            let Ok(tag) = read_classic_tag_file(&bytes, layout) else { continue };
            let Ok(animation) = Animation::new(&tag) else { continue };
            tags += 1;
            let skeleton = Skeleton::from_tag(&tag);
            let graph = AnimationGraph::from_tag(&tag);
            let defaults: Vec<NodeTransform> = (0..skeleton.len()).map(|_| NodeTransform::default()).collect();

            for group in animation.iter() {
                if group.blob.is_empty() { empty += 1; continue; }
                let clip = match group.decode() { Ok(c) => c, Err(e) => {
                    errs += 1;
                    if err_examples.len() < 10 { err_examples.push(format!("decode {} {:?}: {e}", p.file_name().unwrap().to_string_lossy(), group.name)); }
                    continue;
                }};
                let kind = JmaKind::from_metadata(
                    group.animation_type.as_deref(), group.frame_info_type.as_deref(), group.world_relative);
                let (pose, leading): (blam_tags::Pose, Vec<NodeTransform>) = match kind {
                    JmaKind::Jmo => {
                        let base = animation.overlay_base_pose(&graph, group, &skeleton, &defaults)
                            .unwrap_or_else(|| defaults.clone());
                        let (reference, body) = clip.overlay_pose(&skeleton, &base);
                        (body, reference)
                    }
                    JmaKind::Jmr => {
                        let base = animation.overlay_base_pose(&graph, group, &skeleton, &defaults)
                            .unwrap_or_else(|| defaults.clone());
                        (clip.replacement_pose(&skeleton, &base), base)
                    }
                    _ => (clip.pose(&skeleton, Some(&defaults)), defaults.clone()),
                };
                let mut sink = Vec::new();
                match pose.write_jma(&mut sink, &skeleton, &leading, group.node_list_checksum, kind, "actor", Some(&clip.movement)) {
                    Ok(()) => exported += 1,
                    Err(e) => { errs += 1; if err_examples.len() < 10 { err_examples.push(format!("write {} {:?}: {e}", p.file_name().unwrap().to_string_lossy(), group.name)); } }
                }
            }
        }
    }
    println!("tags={tags} exported={exported} empty={empty} errs={errs}");
    for e in &err_examples { println!("  {e}"); }
    Ok(())
}
