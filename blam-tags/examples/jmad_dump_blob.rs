//! Dump the raw animation_data blob for one anim.
//! Usage: jmad_dump_blob <FILE> <ANIM_INDEX> <OUTPUT>
use blam_tags::{Animation, TagFile};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut a = std::env::args().skip(1);
    let file = a.next().ok_or("usage")?;
    let idx: usize = a.next().ok_or("usage")?.parse()?;
    let out = a.next().ok_or("usage")?;
    let tag = TagFile::read(&file)?;
    let anim = Animation::new(&tag)?;
    let g = anim.get(idx).ok_or("oob")?;
    println!("anim {idx} '{}' frame_count={} blob={} bytes",
        g.name.as_deref().unwrap_or(""), g.frame_count, g.blob.len());
    if let Some(d) = &g.data_sizes {
        for (k, v) in &d.fields { println!("  {}: {}", k, v); }
    }
    std::fs::write(&out, g.blob)?;
    println!("wrote {}", out);
    Ok(())
}
