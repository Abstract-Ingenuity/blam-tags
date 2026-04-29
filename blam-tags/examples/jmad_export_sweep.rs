//! Sweep `.model_animation_graph` tags and JMA-export every
//! animation. Tally success / failure. Doesn't write the JMA bytes —
//! routes them through a sink so we exercise the writer without
//! filling disk.
//!
//! Usage: jmad_export_sweep <DIR> [<DIR>...]

use std::error::Error;
use std::path::{Path, PathBuf};

use blam_tags::{Animation, JmaKind, Skeleton, TagFile};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() { return Ok(()); }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() { collect(&path, out)?; }
        else if path.extension().and_then(|s| s.to_str()) == Some("model_animation_graph") {
            out.push(path);
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let dirs: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if dirs.is_empty() {
        eprintln!("usage: jmad_export_sweep <DIR> [<DIR>...]");
        std::process::exit(2);
    }
    let mut paths = Vec::new();
    for d in &dirs { eprintln!("scanning {}", d.display()); collect(d, &mut paths)?; }
    eprintln!("found {} tags", paths.len());

    let mut tags = 0u64;
    let mut anims = 0u64;
    let mut exported = 0u64;
    let mut failed = 0u64;
    let mut io_failed = 0u64;
    for (i, path) in paths.iter().enumerate() {
        if i % 200 == 0 && i > 0 { eprintln!("  progress {}/{}", i, paths.len()); }
        tags += 1;
        let tag = match TagFile::read(path) { Ok(t) => t, Err(_) => continue };
        let animation = match Animation::new(&tag) { Ok(a) => a, Err(_) => continue };
        let skeleton = Skeleton::from_tag(&tag);
        if skeleton.is_empty() { continue; }
        for g in animation.iter() {
            anims += 1;
            let clip = match g.decode() { Ok(c) => c, Err(_) => { failed += 1; continue; } };
            let pose = clip.pose(&skeleton, None);
            let kind = JmaKind::from_metadata(
                g.animation_type.as_deref(),
                g.frame_info_type.as_deref(),
                g.world_relative,
            );
            let defaults: Vec<_> = (0..skeleton.len()).map(|_| blam_tags::NodeTransform::IDENTITY).collect();
            let mut sink = std::io::sink();
            match pose.write_jma(&mut sink, &skeleton, &defaults, g.node_list_checksum, kind, "actor", Some(&clip.movement)) {
                Ok(()) => exported += 1,
                Err(_) => io_failed += 1,
            }
        }
    }
    println!("tags:     {tags}");
    println!("anims:    {anims}");
    println!("exported: {exported}");
    println!("decode failed: {failed}");
    println!("io failed:     {io_failed}");
    Ok(())
}
