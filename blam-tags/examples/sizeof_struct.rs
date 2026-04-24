//! Debug helper — print struct sizes from a tag's embedded layout.
//!
//! Usage: sizeof_struct <TAG_FILE>
//!
//! Walks the tag's `blay` chunk and prints every struct's computed size.
//! Handy for verifying what a real tag believes about struct layouts
//! vs what a dumped JSON schema claims.

use std::error::Error;
use std::path::PathBuf;

use blam_tags::TagFile;

fn main() -> Result<(), Box<dyn Error>> {
    let path = PathBuf::from(
        std::env::args().nth(1).ok_or("usage: sizeof_struct <TAG_FILE>")?,
    );
    let tag = TagFile::read(&path)?;

    let defs = tag.definitions();
    let root = defs.root_struct();
    println!("root struct: {} ({} bytes, {} visible fields)",
        root.name(), root.size(), root.fields().count());

    // Walk the whole schema graph via BFS starting at root.
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: Vec<_> = vec![root];

    while let Some(s) = queue.pop() {
        if !seen.insert(s.name().to_string()) { continue; }
        println!("  {:50}  size={:5}", s.name(), s.size());
        for f in s.fields() {
            if let Some(b) = f.as_block() {
                queue.push(b.struct_definition());
            } else if let Some(a) = f.as_array() {
                queue.push(a.struct_definition());
            } else if let Some(ss) = f.as_struct() {
                queue.push(ss);
            }
        }
    }
    Ok(())
}
