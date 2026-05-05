//! Extract the longsword_x_burn base + self-illum bitmaps to DDS so
//! we can inspect them. Used to verify whether the
//! `longsword_burn_unlit.shader` is the over-bright pelican thruster
//! material.

use std::fs::File;
use std::io::BufWriter;

use blam_tags::{Bitmap, TagFile};

fn extract(path: &str, out_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let tag = TagFile::read(path)?;
    let bm = Bitmap::new(&tag)?;
    println!(
        "{path}: {} image(s), first format={:?}",
        bm.len(),
        bm.image(0).and_then(|i| i.format_name()),
    );
    if let Some(img) = bm.image(0) {
        println!(
            "  {}x{} mips={} type={:?} curve={:?}",
            img.width(), img.height(), img.mipmap_levels(),
            img.type_name(), img.curve(),
        );
        let mut out = BufWriter::new(File::create(out_path)?);
        img.write_dds(&mut out)?;
        println!("  wrote {out_path}");
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    extract(
        "/Users/camden/Halo/halo3_mcc/tags/objects/vehicles/pelican/bitmaps/pelican_instances.bitmap",
        "/tmp/pelican_instances.dds",
    )?;
    Ok(())
}
