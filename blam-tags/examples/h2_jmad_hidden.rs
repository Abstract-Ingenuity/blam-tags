//! Scan H2 jmads for animations whose blob is empty but which carry a
//! non-empty UNNAMED data field (the v2 version-resolution signature) —
//! i.e. data we'd be silently dropping. Usage: <defs> <root>

use std::path::PathBuf;
use blam_tags::animation::Animation;
use blam_tags::classic::read_classic_tag_file;
use blam_tags::layout::TagLayout;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let layout_path = PathBuf::from(&a[1]).join("model_animation_graph.json");
    let mut stack = vec![PathBuf::from(&a[2])];
    let (mut hidden_tags, mut hidden_anims, mut truly_empty) = (0u64, 0u64, 0u64);
    let mut examples = Vec::new();
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() { stack.push(p); continue; }
            if p.extension().and_then(|x|x.to_str()) != Some("model_animation_graph") { continue; }
            let digsite = p.to_string_lossy().contains("/digsite/");
            let Ok(bytes) = std::fs::read(&p) else { continue };
            let Ok(layout) = TagLayout::from_json(&layout_path) else { continue };
            let Ok(tag) = read_classic_tag_file(&bytes, layout) else { continue };
            let root = tag.root();
            let Some(res) = root.field("resources").and_then(|f|f.as_struct()) else { continue };
            let Some(ab) = res.field("animations").and_then(|f|f.as_block()) else { continue };
            let Ok(anim) = Animation::new(&tag) else { continue };
            let mut tag_has_hidden = false;
            for (i, g) in anim.iter().enumerate() {
                if !g.blob.is_empty() { continue; }
                // look at the raw element for an unnamed data field with bytes
                let hidden = ab.element(i).map(|el| {
                    el.fields().any(|f| f.name().is_empty()
                        && f.as_data().map(|b| !b.is_empty()).unwrap_or(false))
                }).unwrap_or(false);
                if hidden {
                    hidden_anims += 1; tag_has_hidden = true;
                    if examples.len() < 8 {
                        examples.push(format!("{}{} #{i} {:?}",
                            if digsite {"[digsite] "} else {""},
                            p.file_name().unwrap().to_string_lossy(), g.name));
                    }
                } else {
                    truly_empty += 1;
                }
            }
            if tag_has_hidden { hidden_tags += 1; }
        }
    }
    println!("anims with HIDDEN unnamed data (dropped): {hidden_anims} across {hidden_tags} tags");
    println!("anims truly empty (no data anywhere): {truly_empty}");
    for e in &examples { println!("  {e}"); }
    Ok(())
}
