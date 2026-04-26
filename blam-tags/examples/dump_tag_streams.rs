//! Dump each top-level stream of a tag and its size.
use std::error::Error;
use std::path::Path;
fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("usage: dump_tag_streams <FILE>")?;
    let bytes = std::fs::read(&path)?;
    println!("file: {} ({} bytes)", path, bytes.len());
    // Walk chunks: 12-byte header (signature[4], version[4], size[4])
    // First read the BLAM header (signature_offset etc) — let me just dump
    // the chunk graph naively starting at offset 0x40 (after header).
    let _ = Path::new(&path);
    let mut off = 0x40usize; // tag header is 0x40 bytes
    while off + 12 <= bytes.len() {
        let sig: [u8; 4] = bytes[off..off+4].try_into()?;
        let _version = u32::from_le_bytes(bytes[off+4..off+8].try_into()?);
        let size = u32::from_le_bytes(bytes[off+8..off+12].try_into()?) as usize;
        let sig_str = String::from_utf8_lossy(&sig);
        let sig_str_be: String = sig.iter().rev().map(|b| *b as char).collect();
        println!("  off=0x{off:08x}  sig={:?} (BE:{:?})  size={size}", sig_str, sig_str_be);
        if size == 0 || off + 12 + size > bytes.len() { break; }
        off += 12 + size;
    }
    Ok(())
}
