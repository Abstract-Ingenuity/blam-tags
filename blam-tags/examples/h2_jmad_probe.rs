//! Probe H2 model_animation_graph (jmad) through the existing Animation
//! pipeline. Usage: h2_jmad_probe <defs-dir> <tag-or-root>

use std::path::PathBuf;

use blam_tags::animation::{Animation, Skeleton};
use blam_tags::classic::read_classic_tag_file;
use blam_tags::layout::TagLayout;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let (defs, target) = (&a[1], &a[2]);
    let layout_path = PathBuf::from(defs).join("model_animation_graph.json");

    let mut paths = Vec::new();
    let tp = PathBuf::from(target);
    if tp.is_dir() {
        let mut stack = vec![tp];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() { stack.push(p); }
                else if p.extension().and_then(|x| x.to_str()) == Some("model_animation_graph") {
                    if std::env::var("SKIP_DIGSITE").is_ok() && p.to_string_lossy().contains("/digsite/") { continue; }
                    paths.push(p);
                }
            }
        }
    } else { paths.push(tp); }
    let sweep = paths.len() > 1;

    let (mut tags, mut anims, mut decoded, mut nopayload, mut errs, mut readerr) = (0u64,0u64,0u64,0u64,0u64,0u64);
    let mut err_kinds = std::collections::BTreeMap::<String,u64>::new();

    for path in &paths {
        let bytes = match std::fs::read(path) { Ok(b)=>b, Err(_)=>{readerr+=1;continue} };
        let layout = match TagLayout::from_json(&layout_path) { Ok(l)=>l, Err(_)=>{readerr+=1;continue} };
        let tag = match read_classic_tag_file(&bytes, layout) { Ok(t)=>t, Err(_)=>{readerr+=1;continue} };
        tags += 1;
        let skel = Skeleton::from_tag(&tag);
        let anim = match Animation::new(&tag) { Ok(a)=>a, Err(e)=>{*err_kinds.entry(format!("new:{e}")).or_default()+=1; errs+=1; continue} };
        if !sweep {
            println!("{}", path.display());
            println!("  skeleton nodes: {}", skel.len());
            println!("  animations: {} (parent={:?})", anim.len(), anim.parent());
        }
        for g in anim.iter() {
            anims += 1;
            if g.blob.is_empty() { nopayload += 1; if !sweep { println!("  [{}] {:?} NO BLOB ds={:?}", g.index, g.name, g.data_sizes.as_ref().map(|d|d.fields.len())); } continue; }
            match g.decode() {
                Ok(clip) => {
                    decoded += 1;
                    if !sweep {
                        println!("  [{}] {:?} type={:?} fit={:?} fc={} codec={:?} blob={} ds_fields={:?} static={} anim={:?}",
                            g.index, g.name, g.animation_type, g.frame_info_type, g.frame_count,
                            g.codec_byte, g.blob.len(),
                            g.data_sizes.as_ref().map(|d| d.fields.iter().map(|(n,v)|format!("{n}={v}")).collect::<Vec<_>>()),
                            clip.static_tracks.rotations.len(),
                            clip.animated_tracks.as_ref().map(|t| (t.codec, t.frame_count)));
                    }
                }
                Err(e) => { *err_kinds.entry(format!("decode:{e}")).or_default()+=1; errs+=1;
                    if !sweep { println!("  [{}] {:?} DECODE ERR: {e}  codec={:?} blob={} ds={:?}", g.index, g.name, g.codec_byte, g.blob.len(), g.data_sizes.as_ref().map(|d|d.fields.clone())); } }
            }
        }
    }
    println!("\n=== tags={tags} anims={anims} decoded={decoded} nopayload={nopayload} errs={errs} readerr={readerr} ===");
    for (k,v) in &err_kinds { println!("  {v:6}  {k}"); }
    Ok(())
}
