//! Full CE export sweep: decode + pose + write_jma every animation to a
//! sink. Usage: <defs-dir> <tags-root>
use std::path::PathBuf;
use blam_tags::animation::classic::CeAnimations;
use blam_tags::animation::{JmaKind, NodeTransform, Skeleton};
use blam_tags::classic::read_classic_tag_file;
use blam_tags::layout::TagLayout;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let lp = PathBuf::from(&a[1]).join("model_animations.json");
    let mut stack = vec![PathBuf::from(&a[2])];
    let (mut tags, mut exported, mut errs) = (0u64,0u64,0u64);
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() { stack.push(p); continue; }
            if p.extension().and_then(|x|x.to_str()) != Some("model_animations") { continue; }
            let Ok(bytes)=std::fs::read(&p) else {continue};
            let Ok(layout)=TagLayout::from_json(&lp) else {continue};
            let Ok(tag)=read_classic_tag_file(&bytes,layout) else {continue};
            tags+=1;
            let skel=Skeleton::from_tag(&tag);
            let defaults:Vec<NodeTransform>=(0..skel.len()).map(|_|NodeTransform::IDENTITY).collect();
            for g in CeAnimations::new(&tag).iter() {
                let clip=g.decode();
                let kind=JmaKind::from_metadata(g.animation_type.as_deref(),g.frame_info_type.as_deref(),g.world_relative);
                let (pose,leading)=match kind {
                    JmaKind::Jmo => { let (r,b)=clip.overlay_pose(&skel,&defaults); (b,r) }
                    JmaKind::Jmr => (clip.replacement_pose(&skel,&defaults), defaults.clone()),
                    _ => (clip.pose(&skel,Some(&defaults)), defaults.clone()),
                };
                let mut sink=Vec::new();
                match pose.write_jma(&mut sink,&skel,&leading,g.node_list_checksum,kind,"actor",Some(&clip.movement)) {
                    Ok(())=>exported+=1, Err(_)=>errs+=1,
                }
            }
        }
    }
    println!("tags={tags} exported={exported} errs={errs}");
    Ok(())
}
