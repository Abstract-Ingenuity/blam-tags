//! Dump the zipped-data field of a render_model's import_info file entry.
use blam_tags::TagFile;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap();
    let idx: usize = args.next().map(|s| s.parse().unwrap()).unwrap_or(0);
    let out = args.next().unwrap_or_else(|| format!("/tmp/zipped_{idx}.bin"));
    let tag = TagFile::read(&path)?;
    let info = tag.import_info().ok_or("no import_info")?;
    let files = info.field_path("files").and_then(|f| f.as_block()).ok_or("no files")?;
    let elem = files.element(idx).ok_or("idx oob")?;
    let bytes = elem.field("zipped data").and_then(|f| f.as_data()).ok_or("no zipped data")?;
    std::fs::write(&out, bytes)?;
    println!("wrote {} bytes to {out}", bytes.len());
    println!("first 4 bytes: {:02x?}", &bytes[..4.min(bytes.len())]);
    Ok(())
}
