//! Decode a CE model_animations and dump first-frame transforms of the
//! first uncompressed animation, to validate the node-state layout +
//! endianness. Usage: ce_antr_decode <defs-dir> <tag> [anim-index]

use std::path::PathBuf;
use blam_tags::animation::classic::CeAnimations;
use blam_tags::animation::Skeleton;
use blam_tags::classic::read_classic_tag_file;
use blam_tags::layout::TagLayout;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let (defs, tag_path) = (&a[1], &a[2]);
    let want: Option<usize> = a.get(3).and_then(|s| s.parse().ok());
    let bytes = std::fs::read(tag_path)?;
    let layout = TagLayout::from_json(PathBuf::from(defs).join("model_animations.json"))?;
    let tag = read_classic_tag_file(&bytes, layout)?;

    let skel = Skeleton::from_tag(&tag);
    println!("skeleton nodes: {}", skel.len());
    for (i, n) in skel.nodes.iter().take(5).enumerate() {
        println!("  node[{i}] {:?} parent={} child={} sib={}", n.name, n.parent, n.first_child, n.next_sibling);
    }

    let anims = CeAnimations::new(&tag);
    println!("animations: {}", anims.len());
    let idx = want.unwrap_or(0);
    let Some(g) = anims.get(idx) else { println!("no anim {idx}"); return Ok(()); };
    println!("[{}] {:?} type={:?} fit={:?} world={} fc={} nc={}",
        g.index, g.name, g.animation_type, g.frame_info_type, g.world_relative, g.frame_count, g.node_count);

    let clip = g.decode();
    println!("clip frame_count={} status={:?}", clip.frame_count, clip.animated_status);
    let pose = clip.pose(&skel, None);
    println!("pose frames={} bones/frame={}", pose.frames.len(), pose.frames.first().map(|r| r.len()).unwrap_or(0));
    // Show first frame's first few bones — rotations should be ~unit quats.
    if let Some(frame0) = pose.frames.first() {
        for (b, t) in frame0.iter().take(6).enumerate() {
            let q = t.rotation;
            let len = (q.i*q.i + q.j*q.j + q.k*q.k + q.w*q.w).sqrt();
            println!("  bone[{b}] rot=({:+.3},{:+.3},{:+.3},{:+.3}) |q|={:.3} trans=({:+.3},{:+.3},{:+.3}) scale={:.3}",
                q.i, q.j, q.k, q.w, len, t.translation.x, t.translation.y, t.translation.z, t.scale);
        }
    }
    // movement
    println!("movement: kind={:?} frames={}", clip.movement.kind, clip.movement.frames.len());
    Ok(())
}
