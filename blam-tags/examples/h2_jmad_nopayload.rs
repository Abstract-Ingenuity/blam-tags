//! Classify H2 jmad nopayload animations: are they in inheriting graphs?
//! Usage: h2_jmad_nopayload <defs-dir> <tags-root>

use std::path::PathBuf;
use blam_tags::animation::Animation;
use blam_tags::classic::read_classic_tag_file;
use blam_tags::layout::TagLayout;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let layout_path = PathBuf::from(&a[1]).join("model_animation_graph.json");
    let mut stack = vec![PathBuf::from(&a[2])];
    let (mut np_with_parent, mut np_no_parent, mut tags_all_np, mut tags_partial_np) = (0u64,0u64,0u64,0u64);
    let mut examples = Vec::new();
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() { stack.push(p); continue; }
            if p.extension().and_then(|x|x.to_str()) != Some("model_animation_graph") { continue; }
            let Ok(bytes) = std::fs::read(&p) else { continue };
            let Ok(layout) = TagLayout::from_json(&layout_path) else { continue };
            let Ok(tag) = read_classic_tag_file(&bytes, layout) else { continue };
            let Ok(anim) = Animation::new(&tag) else { continue };
            let has_parent = anim.parent().is_some();
            let np = anim.iter().filter(|g| g.blob.is_empty()).count();
            let total = anim.len();
            if np == 0 { continue; }
            if np == total { tags_all_np += 1; } else { tags_partial_np += 1; }
            if has_parent { np_with_parent += np as u64; } else { np_no_parent += np as u64; }
            if !has_parent && np > 0 && examples.len() < 12 {
                let g = anim.iter().find(|g| g.blob.is_empty()).unwrap();
                examples.push(format!("{} #{} {:?} parent={:?} np={}/{}",
                    p.file_name().unwrap().to_string_lossy(), g.index, g.name, anim.parent(), np, total));
            }
        }
    }
    println!("nopayload in PARENTED graphs: {np_with_parent}");
    println!("nopayload in NON-parented graphs: {np_no_parent}");
    println!("tags fully-nopayload: {tags_all_np}  tags partial-nopayload: {tags_partial_np}");
    println!("--- non-parented examples (potential gaps) ---");
    for e in &examples { println!("  {e}"); }
    Ok(())
}
