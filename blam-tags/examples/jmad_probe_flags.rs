//! Search for the actual flag-bitarray offset by scanning the blob
//! for u32 windows whose popcounts match the codec header's
//! `total_*_nodes`. Usage: jmad_probe_flags <FILE> <ANIM_INDEX>

use std::error::Error;
use blam_tags::{Animation, BitArray, TagFile};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let file = args.next().ok_or("usage: jmad_probe_flags <FILE> <ANIM_INDEX>")?;
    let idx: usize = args.next().ok_or("missing anim index")?.parse()?;
    let tag = TagFile::read(&file)?;
    let animation = Animation::new(&tag)?;
    let group = animation.get(idx).ok_or("anim out of range")?;
    let blob = group.blob;
    let ds = group.data_sizes.as_ref().ok_or("no data sizes")?;
    let static_total = ds.get("static_node_flags") as usize;
    let animated_total = ds.get("animated_node_flags") as usize;
    let movement = ds.get("movement_data") as usize;
    let pill = ds.get("pill_offset_data") as usize;
    let default_data = ds.get("default_data") as usize;
    let uncompressed = ds.get("uncompressed_data") as usize;
    let compressed = ds.get("compressed_data") as usize;
    println!("blob_len={}", blob.len());
    println!("default_data={default_data}, uncompressed={uncompressed}, compressed={compressed}");
    println!("static_flags={static_total}, animated_flags={animated_total}, movement={movement}, pill={pill}");

    let clip = group.decode()?;
    let s = &clip.static_tracks;
    println!();
    println!("static codec: {}r/{}t/{}s",
        s.rotations.len(), s.translations.len(), s.scales.len());
    if let Some(a) = &clip.animated_tracks {
        println!("animated codec: {}r/{}t/{}s",
            a.rotations.len(), a.translations.len(), a.scales.len());
    }

    // Try several layouts and check popcount.
    let static_per = static_total / 3;
    let animated_per = animated_total / 3;
    println!();
    println!("Trying candidate offsets (popcount needs to match codec totals):");
    let codec_end = default_data + uncompressed + compressed;
    let candidates = [
        ("flags at start [sf|af|def|cod|...]", 0),
        ("flags after default [def|sf|af|cod|...]", default_data),
        ("flags before move [trail-mv-pill]", blob.len() - movement - pill - animated_total - static_total),
        ("flags after codec [def|cod|sf|af|...]", codec_end),
        ("flags after codec+pill [def|cod|pill|sf|af]", codec_end + pill),
        ("flags after codec+mv [def|cod|mv|sf|af]", codec_end + movement),
    ];
    for (label, off) in candidates {
        if off + static_total + animated_total <= blob.len() {
            let s_rot = BitArray::from_bytes(&blob[off..off + static_per]);
            let s_trans = BitArray::from_bytes(&blob[off + static_per..off + 2 * static_per]);
            let s_scale = BitArray::from_bytes(&blob[off + 2 * static_per..off + 3 * static_per]);
            let a_off = off + static_total;
            let a_rot = BitArray::from_bytes(&blob[a_off..a_off + animated_per]);
            let a_trans = BitArray::from_bytes(&blob[a_off + animated_per..a_off + 2 * animated_per]);
            let a_scale = BitArray::from_bytes(&blob[a_off + 2 * animated_per..a_off + 3 * animated_per]);
            let p = |b: &BitArray, n: usize| (0..n).filter(|&i| b.bit(i)).count();
            let n = 256;
            println!("  off=0x{off:x} ({label})");
            println!("    static  popcount: r={} t={} s={}",
                p(&s_rot, n), p(&s_trans, n), p(&s_scale, n));
            println!("    animated popcount: r={} t={} s={}",
                p(&a_rot, n), p(&a_trans, n), p(&a_scale, n));
        }
    }
    Ok(())
}
