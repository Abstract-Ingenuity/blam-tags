use blam_tags::{Bitmap, TagFile};
use std::path::PathBuf;
fn collect(d: &std::path::Path, out: &mut Vec<PathBuf>) {
    if let Ok(es) = std::fs::read_dir(d) { for e in es.flatten() {
        let p = e.path();
        if p.is_dir() { collect(&p, out); }
        else if p.extension().and_then(|s| s.to_str()) == Some("bitmap") { out.push(p); }
    }}
}
fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let mut paths = Vec::new();
    collect(std::path::Path::new(&dir), &mut paths);
    for p in &paths {
        let Ok(tag) = TagFile::read(p) else { continue };
        let Ok(bitmap) = Bitmap::new(&tag) else { continue };
        if bitmap.len() != 1 { continue; }
        let Some(img) = bitmap.image(0) else { continue };
        if img.type_name().as_deref() != Some("2D texture") { continue; }
        let mf = tag.root().field_path("bitmaps[0]/more flags").and_then(|f| f.value());
        let flagged = matches!(mf, Some(blam_tags::TagFieldData::ByteFlags { value, .. }) if value & 0x04 != 0);
        if flagged {
            println!("flagged {} {}x{}: {}", img.format_name().unwrap_or_default(), img.width(), img.height(), p.display());
        }
    }
}
