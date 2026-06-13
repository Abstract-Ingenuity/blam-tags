//! Probe CE model_animations (antr): dump per-animation header + blob sizes
//! to derive the node-state encoding. Usage: ce_antr_probe <defs-dir> <tag>

use std::path::PathBuf;

use blam_tags::classic::read_classic_tag_file;
use blam_tags::layout::TagLayout;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let (defs, tag_path) = (&a[1], &a[2]);
    let bytes = std::fs::read(tag_path)?;
    let layout = TagLayout::from_json(PathBuf::from(defs).join("model_animations.json"))?;
    let tag = read_classic_tag_file(&bytes, layout)?;
    let root = tag.root();

    let nodes = root.field_path("nodes").and_then(|f| f.as_block());
    let node_count_tag = nodes.as_ref().map(|b| b.len()).unwrap_or(0);
    println!("nodes(block)={node_count_tag}");
    if let Some(nb) = &nodes {
        for i in 0..nb.len().min(8) {
            if let Some(n) = nb.element(i) {
                println!("  node[{i}] name={:?} parent={:?}",
                    n.read_string("name"),
                    n.read_int_any("parent node"));
            }
        }
    }

    let anims = root.field_path("animations").and_then(|f| f.as_block());
    let Some(ab) = anims else { println!("no animations block"); return Ok(()); };
    println!("animations={}", ab.len());
    for i in 0..ab.len().min(12) {
        let Some(e) = ab.element(i) else { continue };
        let name = e.read_string("name");
        let ty = e.read_enum_name("type");
        let fc = e.read_int_any("frame count").unwrap_or(0);
        let fs = e.read_int_any("frame size").unwrap_or(0);
        let nc = e.read_int_any("node count").unwrap_or(0);
        let fit = e.read_enum_name("frame info type");
        let flags = e.read_int_any("flags").unwrap_or(0);
        let frame_info = e.field("frame info").and_then(|f| f.as_data()).map(|d| d.len()).unwrap_or(0);
        let default_data = e.field("default data").and_then(|f| f.as_data()).map(|d| d.len()).unwrap_or(0);
        let frame_data = e.field("frame data").and_then(|f| f.as_data()).map(|d| d.len()).unwrap_or(0);
        // flag long words
        let tr = e.read_int_any("node transform flag data").unwrap_or(0);
        println!(
            "[{i:2}] {name:?} type={ty:?} fc={fc} fs={fs} nc={nc} fit={fit:?} flags={flags:#x} \
             frame_info={frame_info} default={default_data} frame={frame_data} xfflag0={tr:#x}",
        );
        // sanity: frame_data / fc should equal fs (uncompressed)
        if fc > 0 && fs > 0 {
            println!("       frame_data/fc = {} (vs fs={fs})", frame_data as i128 / fc);
        }
    }
    Ok(())
}
