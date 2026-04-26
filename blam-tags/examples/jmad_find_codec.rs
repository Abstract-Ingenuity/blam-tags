//! Find an example animation whose animated codec matches the given byte.
//! Usage: jmad_find_codec <ANIM_CODEC_BYTE> <DIR> [<DIR>...]

use std::error::Error;
use std::path::{Path, PathBuf};
use blam_tags::{Animation, TagFile};

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
    let want: u8 = std::env::args().nth(1).ok_or("usage: jmad_find_codec <BYTE> <DIR>...")?
        .parse()?;
    let dirs: Vec<PathBuf> = std::env::args().skip(2).map(PathBuf::from).collect();
    let mut paths = Vec::new();
    for d in &dirs { collect(d, &mut paths)?; }
    let mut shown = 0;
    for path in &paths {
        if shown >= 5 { break; }
        let tag = match TagFile::read(path) { Ok(t) => t, Err(_) => continue };
        let anim = match Animation::new(&tag) { Ok(a) => a, Err(_) => continue };
        for g in anim.iter() {
            if shown >= 5 { break; }
            if g.animated_codec_byte() == Some(want) {
                println!("{} :: anim {} '{}' frame_count={}",
                    path.display(), g.index, g.name.as_deref().unwrap_or(""), g.frame_count);
                shown += 1;
            }
        }
    }
    Ok(())
}
