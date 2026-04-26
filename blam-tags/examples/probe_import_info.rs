//! Probe import_info files block on a tag.
use blam_tags::TagFile;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).unwrap();
    let tag = TagFile::read(&path)?;
    let info = tag.import_info().ok_or("no import_info stream")?;
    let files = info.field_path("files").and_then(|f| f.as_block()).ok_or("no files block")?;
    println!("import_info files: {} entries", files.len());
    for i in 0..files.len() {
        let elem = files.element(i).unwrap();
        let path = elem.field("path").and_then(|f| f.value()).map(|v| format!("{v:?}")).unwrap_or_default();
        let size = elem.field("size:bytes").and_then(|f| f.value()).map(|v| format!("{v:?}")).unwrap_or_default();
        let zipped = elem.field("zipped data").and_then(|f| f.as_data()).map(|b| b.len()).unwrap_or(0);
        let parent = elem.field("parent file").and_then(|f| f.value()).map(|v| format!("{v:?}")).unwrap_or_default();
        println!("  [{i}] path={path}  size={size}  zipped_bytes={zipped}  parent={parent}");
    }
    Ok(())
}
