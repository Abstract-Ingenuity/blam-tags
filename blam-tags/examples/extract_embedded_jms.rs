//! Extract the source JMS (or other artist asset) embedded in a tag's
//! `import_info` stream. H3 render_models include the original `.jms`
//! the artist authored, zlib-compressed in the `files[]/zipped data`
//! field. Coverage: 99.36% of H3 render_models per
//! `zipped_jms_sweep`. Reach largely uses `.gr2` and doesn't embed
//! source bytes — this tool only handles entries whose `zipped data`
//! is non-empty.
//!
//! Usage: extract_embedded_jms <TAG_FILE> [<OUT_DIR>]
//!
//! Lists every `files[]` entry, and for each one with non-empty
//! `zipped data`, decompresses (zlib) and writes the result to
//! `<OUT_DIR>/<basename>` (default OUT_DIR is `.`).

use std::io::Read;
use std::path::{Path, PathBuf};

use blam_tags::{TagFieldData, TagFile};
use flate2::read::ZlibDecoder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let tag_path = args.next().ok_or("usage: extract_embedded_jms <TAG_FILE> [<OUT_DIR>]")?;
    let out_dir = PathBuf::from(args.next().unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&out_dir)?;

    let tag = TagFile::read(&tag_path)?;
    let info = tag.import_info().ok_or("tag has no import_info stream")?;
    let files = info
        .field_path("files")
        .and_then(|f| f.as_block())
        .ok_or("import_info has no files block")?;

    println!("import_info files: {} entries", files.len());
    for i in 0..files.len() {
        let elem = files.element(i).unwrap();
        let path_str = match elem.field("path").and_then(|f| f.value()) {
            Some(TagFieldData::LongString(s) | TagFieldData::String(s)) => s,
            _ => String::new(),
        };
        let zipped = elem.field("zipped data").and_then(|f| f.as_data()).unwrap_or(&[]);
        if zipped.is_empty() {
            println!("  [{i}] {path_str}  (no zipped data)");
            continue;
        }
        let basename = Path::new(&path_str.replace('\\', "/"))
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file.bin")
            .to_owned();
        let out_path = out_dir.join(&basename);
        let mut decoder = ZlibDecoder::new(zipped);
        let mut decompressed = Vec::with_capacity(zipped.len() * 4);
        decoder.read_to_end(&mut decompressed)?;
        std::fs::write(&out_path, &decompressed)?;
        println!(
            "  [{i}] {path_str}  zipped={} → {} bytes  ->  {}",
            zipped.len(),
            decompressed.len(),
            out_path.display(),
        );
    }
    Ok(())
}
