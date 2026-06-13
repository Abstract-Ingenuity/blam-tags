//! Probe: read one H2 render_model (via the classic decoder) and write
//! its JMS (version 8210). Usage: ce_jms_probe <defs-dir> <tag> <out.jms>

use std::io::BufWriter;
use std::path::PathBuf;

use blam_tags::classic::{read_classic_tag_file, ClassicHeader};
use blam_tags::game::Game;
use blam_tags::jms::JmsFile;
use blam_tags::layout::TagLayout;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let (defs, tag_path, out) = (&a[1], &a[2], &a[3]);
    let bytes = std::fs::read(tag_path)?;
    let (header, _engine) = ClassicHeader::parse(&bytes).ok_or("not classic")?;
    let _ = header;
    let layout = TagLayout::from_json(PathBuf::from(defs).join("gbxmodel.json"))?;
    let tag = read_classic_tag_file(&bytes, layout)?;

    let jms = JmsFile::from_gbxmodel(&tag)?;
    let version = Game::of(&tag).jms_version();
    let mut w = BufWriter::new(std::fs::File::create(out)?);
    jms.write(&mut w, version)?;

    // Also dump a plain OBJ for visual inspection (JMS doesn't share
    // vertices: triangle i owns verts 3i,3i+1,3i+2).
    use std::io::Write as _;
    let mut obj = BufWriter::new(std::fs::File::create(format!("{out}.obj"))?);
    for v in &jms.vertices {
        writeln!(obj, "v {} {} {}", v.position.x, v.position.y, v.position.z)?;
    }
    for t in &jms.triangles {
        writeln!(obj, "f {} {} {}", t.v[0] + 1, t.v[1] + 1, t.v[2] + 1)?;
    }

    println!(
        "{out}: v{version}  {} nodes, {} materials, {} markers, {} vertices, {} triangles",
        jms.nodes.len(), jms.materials.len(), jms.markers.len(), jms.vertices.len(), jms.triangles.len()
    );
    Ok(())
}
