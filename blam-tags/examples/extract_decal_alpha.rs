//! Dump the alpha channel of riverworld_rockblend_decal as a PNG to
//! inspect whether the bitmap has soft falloff or hard edges.

use blam_tags::{Bitmap, TagFile};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(
        "/Users/camden/Halo/halo3_mcc/tags/levels/shared/decals/multi/riverworld/riverworld_rockblend_decal.bitmap"
    );
    let tag = TagFile::read(&path)?;
    let bitmap = Bitmap::new(&tag)?;
    let img = bitmap.image(0).ok_or("no img 0")?;
    println!("dims: {}x{} fmt={:?} mips={}", img.width(), img.height(), img.format()?, img.mipmap_levels());

    // Dump as DDS for now (we can convert to PNG separately if needed).
    let mut dds = Vec::new();
    img.write_dds(&mut dds)?;
    std::fs::write("/tmp/rockblend_decal.dds", &dds)?;
    println!("wrote /tmp/rockblend_decal.dds ({} bytes)", dds.len());
    Ok(())
}
