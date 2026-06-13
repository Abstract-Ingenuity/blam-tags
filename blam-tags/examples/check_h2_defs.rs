use blam_tags::layout::TagLayout;
use std::path::PathBuf;
fn main() {
    let dir = PathBuf::from("definitions/halo2_mcc");
    let mut fail = Vec::new();
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let p = e.path();
        if p.extension().map_or(true, |x| x != "json") { continue; }
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        if name == "_meta" { continue; }
        if let Err(err) = TagLayout::from_json(&p) { fail.push(format!("{name}: {err}")); }
    }
    fail.sort();
    println!("{} defs FAILED to load:", fail.len());
    for f in &fail { println!("  {f}"); }
}
