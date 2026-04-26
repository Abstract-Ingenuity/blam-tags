//! Diagnostic — dump per-bone flags for a single animation.
//! Usage: jmad_dump_flags <FILE> <ANIM_INDEX>

use std::error::Error;
use blam_tags::{Animation, Skeleton, TagFile};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let file = args.next().ok_or("usage: jmad_dump_flags <FILE> <ANIM_INDEX>")?;
    let idx: usize = args.next().ok_or("missing anim index")?.parse()?;
    let tag = TagFile::read(&file)?;
    let animation = Animation::new(&tag)?;
    let skeleton = Skeleton::from_tag(&tag);
    let group = animation.get(idx).ok_or("anim out of range")?;
    let clip = group.decode()?;
    println!("animation: '{}' frame_count={} animated_status={:?}",
        group.name.as_deref().unwrap_or(""), clip.frame_count, clip.animated_status);
    println!("static_tracks: {}r/{}t/{}s",
        clip.static_tracks.rotations.len(), clip.static_tracks.translations.len(), clip.static_tracks.scales.len());
    if let Some(a) = &clip.animated_tracks {
        println!("animated_tracks: {}r/{}t/{}s × {} frames",
            a.rotations.len(), a.translations.len(), a.scales.len(), a.frame_count);
    }
    if let Some(f) = &clip.node_flags {
        println!("static_rotation: {} bones set", count_set(&f.static_rotation, skeleton.len()));
        println!("static_translation: {} bones set", count_set(&f.static_translation, skeleton.len()));
        println!("animated_rotation: {} bones set", count_set(&f.animated_rotation, skeleton.len()));
        println!("animated_translation: {} bones set", count_set(&f.animated_translation, skeleton.len()));
        println!();
        println!("per-bone breakdown (first 10):");
        for b in 0..10.min(skeleton.len()) {
            let name = &skeleton.nodes[b].name;
            let sr = f.static_rotation.bit(b); let ar = f.animated_rotation.bit(b);
            let st = f.static_translation.bit(b); let at = f.animated_translation.bit(b);
            println!("  [{b:>2}] {name:<20} rot=[s={} a={}] trans=[s={} a={}]",
                sr as u8, ar as u8, st as u8, at as u8);
        }
    } else {
        println!("(no node flags read)");
    }
    Ok(())
}

fn count_set(b: &blam_tags::BitArray, n: usize) -> usize {
    (0..n).filter(|&i| b.bit(i)).count()
}
