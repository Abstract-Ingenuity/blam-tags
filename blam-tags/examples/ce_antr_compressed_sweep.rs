//! Sweep CE model_animations: count animations with the `compressed_data`
//! flag set (bit 0). Usage: ce_antr_compressed_sweep <defs-dir> <tags-root>

use std::path::PathBuf;

use blam_tags::classic::read_classic_tag_file;
use blam_tags::layout::TagLayout;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let (defs, root_dir) = (&a[1], &a[2]);
    let layout_path = PathBuf::from(defs).join("model_animations.json");

    let mut tags = 0u64;
    let mut anims = 0u64;
    let mut compressed = 0u64;
    let mut read_err = 0u64;
    let mut compressed_examples = Vec::new();

    for entry in walkdir(PathBuf::from(root_dir)) {
        if entry.extension().and_then(|e| e.to_str()) != Some("model_animations") {
            continue;
        }
        tags += 1;
        let bytes = match std::fs::read(&entry) { Ok(b) => b, Err(_) => { read_err += 1; continue } };
        let layout = match TagLayout::from_json(&layout_path) { Ok(l) => l, Err(_) => { read_err += 1; continue } };
        let tag = match read_classic_tag_file(&bytes, layout) { Ok(t) => t, Err(_) => { read_err += 1; continue } };
        let root = tag.root();
        let Some(ab) = root.field_path("animations").and_then(|f| f.as_block()) else { continue };
        for i in 0..ab.len() {
            let Some(e) = ab.element(i) else { continue };
            anims += 1;
            let flags = e.read_int_any("flags").unwrap_or(0);
            if flags & 1 == 1 {
                compressed += 1;
                if compressed_examples.len() < 10 {
                    compressed_examples.push(format!("{}#{i} {:?}", entry.display(), e.read_string("name")));
                }
            }
        }
    }
    println!("tags={tags} anims={anims} compressed={compressed} read_err={read_err}");
    for ex in &compressed_examples { println!("  {ex}"); }
    Ok(())
}

fn walkdir(root: PathBuf) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() { stack.push(p); } else { out.push(p); }
        }
    }
    out
}
