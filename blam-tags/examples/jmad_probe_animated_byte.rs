//! Quick diagnostic — find animations where the byte at `default_data`
//! offset isn't a valid codec value, dump the surrounding bytes.

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
    let dir = std::env::args().nth(1).ok_or("usage: jmad_probe_animated_byte <DIR>")?;
    let mut paths = Vec::new();
    collect(Path::new(&dir), &mut paths)?;
    let mut shown = 0;
    for path in &paths {
        if shown >= 10 { break; }
        let tag = match TagFile::read(path) { Ok(t) => t, Err(_) => continue };
        let anim = match Animation::new(&tag) { Ok(a) => a, Err(_) => continue };
        for g in anim.iter() {
            if shown >= 10 { break; }
            let b = match g.animated_codec_byte() { Some(b) => b, None => continue };
            if b <= 11 { continue; }
            let ds = match g.data_sizes.as_ref() { Some(d) => d, None => continue };
            let off = ds.get("default_data") as usize;
            let blob = g.blob;
            let lo = off.saturating_sub(8);
            let hi = (off + 16).min(blob.len());
            println!("{}", path.display());
            println!("  anim {} '{}'", g.index, g.name.as_deref().unwrap_or(""));
            println!("  blob_len={} default_data={} byte_at_offset=0x{:02x} ({})",
                blob.len(), off, b, b);
            println!("  ds.total={} (vs blob_len {})", ds.total(), blob.len());
            println!("  fields: {:?}", ds.fields);
            println!("  bytes [{lo}..{hi}]: {:02x?}", &blob[lo..hi]);
            println!();
            shown += 1;
        }
    }
    Ok(())
}
