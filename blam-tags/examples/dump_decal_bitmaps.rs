//! One-off: dump format/curve/dims of the decal base_maps used by riverworld.

use blam_tags::TagFile;
use blam_tags::Bitmap;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bitmaps = [
        "levels/shared/decals/multi/riverworld/riverworld_granite_decal.bitmap",
        "levels/shared/decals/multi/riverworld/riverworld_grass_decal.bitmap",
        "levels/shared/decals/multi/riverworld/riverworld_ground_decal.bitmap",
        "levels/shared/decals/multi/riverworld/riverworld_rockblend_decal.bitmap",
        "levels/shared/decals/multi/riverworld/riverworld_granite_crack_decal.bitmap",
        "levels/shared/decals/multi/riverworld/riverworld_granite_decal_bump.bitmap",
        "levels/shared/decals/multi/riverworld/riverworld_rockblend_decal_bump.bitmap",
    ];
    let tags_root = PathBuf::from("/Users/camden/Halo/halo3_mcc/tags");
    for rel in bitmaps {
        let path = tags_root.join(rel);
        match TagFile::read(&path) {
            Ok(tag) => match Bitmap::new(&tag) {
                Ok(bitmap) => {
                    for i in 0..bitmap.len() {
                        if let Some(img) = bitmap.image(i) {
                            let fmt = img.format().map(|f| format!("{f:?}")).unwrap_or_else(|_| "?".into());
                            let curve = format!("{:?}", img.curve());
                            println!(
                                "{rel} #{i}: {}x{} fmt={} curve={} mips={} layers={} cube={}",
                                img.width(), img.height(), fmt, curve,
                                img.mipmap_levels(), img.layer_count(), img.is_cube(),
                            );
                        }
                    }
                }
                Err(e) => println!("{rel}: bitmap parse error: {e:?}"),
            },
            Err(e) => println!("{rel}: tag read error: {e:?}"),
        }
    }
    Ok(())
}
