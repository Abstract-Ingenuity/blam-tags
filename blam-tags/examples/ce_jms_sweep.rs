//! Robustness sweep for the H2 gbxmodel → JMS reader. Walks a tag
//! tree, decodes every `.gbxmodel` via the classic path, builds the
//! JMS, and tallies ok / empty / error plus total geometry.
//! Usage: ce_jms_sweep <defs-dir> <tags-root>

use std::path::PathBuf;

use blam_tags::classic::{read_classic_tag_file, ClassicHeader};
use blam_tags::jms::JmsFile;
use blam_tags::layout::TagLayout;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let defs = PathBuf::from(&a[1]);
    let root = &a[2];
    let (mut tot, mut ok, mut empty, mut err) = (0usize, 0, 0, 0);
    let (mut verts, mut tris) = (0u64, 0u64);
    let mut samples: Vec<String> = Vec::new();

    let mut stack = vec![PathBuf::from(root)];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() { stack.push(p); continue; }
            if !p.to_string_lossy().ends_with(".gbxmodel") { continue; }
            let Ok(bytes) = std::fs::read(&p) else { continue };
            if ClassicHeader::parse(&bytes).is_none() { continue; }
            let Ok(layout) = TagLayout::from_json(defs.join("gbxmodel.json")) else { continue };
            let Ok(tag) = read_classic_tag_file(&bytes, layout) else { err += 1; tot += 1; continue };
            tot += 1;
            match JmsFile::from_gbxmodel(&tag) {
                Ok(j) if j.triangles.is_empty() => {
                    empty += 1;
                    if samples.len() < 12 {
                        let r = tag.root();
                        let nr = r.field_path("regions").and_then(|f| f.as_block()).map(|b| b.len()).unwrap_or(0);
                        let ns = r.field_path("sections").and_then(|f| f.as_block()).map(|b| b.len()).unwrap_or(0);
                        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("?");
                        samples.push(format!("EMPTY {name} (regions={nr} sections={ns})"));
                    }
                }
                Ok(j) => {
                    ok += 1;
                    verts += j.vertices.len() as u64;
                    tris += j.triangles.len() as u64;
                }
                Err(e) => {
                    err += 1;
                    if samples.len() < 10 {
                        samples.push(format!("{} :: {e}", p.strip_prefix(root).unwrap().display()));
                    }
                }
            }
        }
    }
    println!("=== {tot} gbxmodels | {ok} ok | {empty} empty | {err} error ===");
    println!("    {verts} vertices, {tris} triangles total");
    for s in &samples { println!("  ERR {s}"); }
}
