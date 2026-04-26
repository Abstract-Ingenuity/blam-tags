//! Reconstruct a JMS for an H3 render_model from its inline
//! `per_mesh_temporary` geometry — without touching the embedded
//! source JMS in `import_info`. Used to validate that the
//! geometry-walk approach matches what the artist's original JMS
//! would have looked like.
//!
//! Usage: reconstruct_render_model_jms <TAG_FILE> <OUT.JMS>

use std::fs::File;
use std::io::BufWriter;

use blam_tags::{JmsFile, TagFile};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let tag_path = args.next().ok_or("usage: reconstruct_render_model_jms <TAG_FILE> <OUT.JMS>")?;
    let out_path = args.next().ok_or("usage: reconstruct_render_model_jms <TAG_FILE> <OUT.JMS>")?;
    let tag = TagFile::read(&tag_path)?;
    let jms = JmsFile::from_render_model(&tag)?;

    let mut w = BufWriter::new(File::create(&out_path)?);
    jms.write(&mut w)?;
    println!(
        "{}: {} nodes, {} materials, {} markers, {} vertices, {} triangles",
        out_path, jms.nodes.len(), jms.materials.len(), jms.markers.len(),
        jms.vertices.len(), jms.triangles.len(),
    );
    Ok(())
}
