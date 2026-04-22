//! Low-level readers/writers shared across every module: primitive
//! fixed-width integers, fixed-size byte arrays, and the 12-byte tag chunk
//! header plus its content helper. Everything on the tag-file wire is
//! either a primitive, a fixed-width array, or a `TagChunkHeader` + payload,
//! so callers never touch `read_exact` directly.
//!
//! **Byte order:** integers on the wire are little-endian; the `_le`
//! helpers are the common case. The `_be` helpers exist for the handful
//! of big-endian fields we may encounter in legacy/Xbox360 data.
//!
//! **Chunk signatures** are 4 ASCII bytes packed into a `u32` as if
//! read big-endian (e.g. `u32::from_be_bytes(*b"tgst")` is the canonical
//! form), so matching a signature in source reads naturally as the tag
//! it represents. On the wire the same bytes are written little-endian
//! via `to_le_bytes`, which — since LE of a BE-packed u32 just reverses
//! the memory order — re-emits the ASCII in source order.

use std::error::Error;
use std::io::{Read, Write};

/// Read a 2-byte big-endian `u16`.
pub fn read_u16_be<R: Read>(reader: &mut std::io::BufReader<R>) -> Result<u16, Box<dyn Error>> {
    let mut buffer = [0u8; size_of::<u16>()];
    reader.read_exact(&mut buffer)?;
    Ok(u16::from_be_bytes(buffer))
}

/// Read a 2-byte little-endian `u16`.
pub fn read_u16_le<R: Read>(reader: &mut std::io::BufReader<R>) -> Result<u16, Box<dyn Error>> {
    let mut buffer = [0u8; size_of::<u16>()];
    reader.read_exact(&mut buffer)?;
    Ok(u16::from_le_bytes(buffer))
}

/// Read a 4-byte big-endian `u32`.
pub fn read_u32_be<R: Read>(reader: &mut std::io::BufReader<R>) -> Result<u32, Box<dyn Error>> {
    let mut buffer = [0u8; size_of::<u32>()];
    reader.read_exact(&mut buffer)?;
    Ok(u32::from_be_bytes(buffer))
}

/// Read a 4-byte little-endian `u32`. The workhorse — most tag-file
/// fields use this byte order.
pub fn read_u32_le<R: Read>(reader: &mut std::io::BufReader<R>) -> Result<u32, Box<dyn Error>> {
    let mut buffer = [0u8; size_of::<u32>()];
    reader.read_exact(&mut buffer)?;
    Ok(u32::from_le_bytes(buffer))
}

/// Read an 8-byte big-endian `u64`.
pub fn read_u64_be<R: Read>(reader: &mut std::io::BufReader<R>) -> Result<u64, Box<dyn Error>> {
    let mut buffer = [0u8; size_of::<u64>()];
    reader.read_exact(&mut buffer)?;
    Ok(u64::from_be_bytes(buffer))
}

/// Read an 8-byte little-endian `u64`.
pub fn read_u64_le<R: Read>(reader: &mut std::io::BufReader<R>) -> Result<u64, Box<dyn Error>> {
    let mut buffer = [0u8; size_of::<u64>()];
    reader.read_exact(&mut buffer)?;
    Ok(u64::from_le_bytes(buffer))
}

/// Read exactly `N` bytes into a fixed-size array. Used for
/// byte-string fields (guids, inline pads, ascii buffers) where byte
/// order isn't meaningful.
pub fn read_u8_array<R: Read, const N: usize>(
    reader: &mut std::io::BufReader<R>,
) -> Result<[u8; N], Box<dyn Error>> {
    let mut buffer = [0u8; N];
    reader.read_exact(&mut buffer)?;
    Ok(buffer)
}

/// 12-byte on-disk header that prefixes every tag chunk.
///
/// The layout is three little-endian `u32`s: signature, version, size.
/// Each of the live writer paths uses dedicated helpers rather than
/// constructing this struct — it's what the reader produces.
#[derive(Debug)]
pub struct TagChunkHeader {
    /// Four ASCII bytes packed into a `u32`. See the module docs for the
    /// BE-pack / LE-emit convention.
    pub signature: u32,
    /// Per-chunk-type version. Usage varies: `tgly` carries the layout
    /// version (2/3), `bdat` is always 1, `tgst` has the hypothesis that
    /// it always equals `size`, and most leaf chunks are 0. Hypotheses
    /// are asserted at read time; see the individual read sites.
    pub version: u32,
    /// Payload byte count. Excludes the 12-byte header itself.
    pub size: u32,
}

/// Read a 12-byte chunk header. Pair with `write_tag_chunk_header`.
pub fn read_tag_chunk_header<R: Read>(
    reader: &mut std::io::BufReader<R>,
) -> Result<TagChunkHeader, Box<dyn Error>> {
    Ok(TagChunkHeader {
        signature: read_u32_le(reader)?,
        version: read_u32_le(reader)?,
        size: read_u32_le(reader)?,
    })
}

/// Write a 12-byte chunk header: signature (4 bytes LE) + version (4 bytes LE) +
/// size (4 bytes LE). Mirrors `read_tag_chunk_header`.
pub fn write_tag_chunk_header<W: Write>(
    writer: &mut W,
    signature: u32,
    version: u32,
    size: u32,
) -> std::io::Result<()> {
    writer.write_all(&signature.to_le_bytes())?;
    writer.write_all(&version.to_le_bytes())?;
    writer.write_all(&size.to_le_bytes())?;
    Ok(())
}

/// Write a chunk header followed by its payload bytes. Size is taken from
/// `content.len()`. Mirrors `read_tag_chunk_content`.
pub fn write_tag_chunk_content<W: Write>(
    writer: &mut W,
    signature: u32,
    version: u32,
    content: &[u8],
) -> std::io::Result<()> {
    write_tag_chunk_header(writer, signature, version, content.len() as u32)?;
    writer.write_all(content)?;
    Ok(())
}

/// Read a chunk header and verify its signature, then read the payload into a
/// `Vec<u8>`. Returns the chunk's `version` (preserved for byte-exact roundtrip)
/// and its `content`. The signature is implicit in the caller's
/// TagSubChunkContent variant, and the size is `content.len()`.
pub fn read_tag_chunk_content<R: Read>(
    reader: &mut std::io::BufReader<R>,
    expected_signature: u32,
) -> Result<(u32, Vec<u8>), Box<dyn Error>> {
    let header = read_tag_chunk_header(reader)?;

    if header.signature != expected_signature {
        return Err(format!(
            "expected chunk signature 0x{expected_signature:08X}, got 0x{:08X}",
            header.signature
        )
        .into());
    }

    let mut content = vec![0u8; header.size as usize];
    reader.read_exact(&mut content)?;

    Ok((header.version, content))
}
