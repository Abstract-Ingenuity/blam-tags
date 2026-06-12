//! Classic (Halo CE / Halo 2) loose-tag decoder + encoder.
//!
//! Gen-1/2 tags are *not* MCC self-describing containers: there is no
//! embedded `blay`/`tgly` layout stream and no `tgbl`/`tgst` chunking.
//! The body is flat data laid out depth-first, interpreted entirely by
//! an *external* field definition (synthesized here from a JSON schema
//! via [`crate::layout::TagLayout::from_json`], ported from the HABT
//! XML layouts).
//!
//! This module owns the classic *encoding*; it borrows the synthesized
//! layout only for structure + field types + nesting. The decoded form
//! is the same [`TagBlockData`]/[`TagStructData`] model the MCC reader
//! produces, so the entire downstream API + extractors work unchanged.
//!
//! ## On-disk shape (verified against real MCC CE/H2 bytes + HABT)
//! - 64-byte file header: `group_tag` at offset 36, `engine` word at
//!   offset 60. Every signature is read as a **u32**: CE stores them
//!   big-endian (`blam`), H2 little-endian (`!MLB` == `BLM!` reversed).
//! - The body (everything after the 64-byte header) begins with the
//!   root struct's fixed bytes (`sizeofValue`), followed by all variable
//!   data depth-first.
//! - **tag block** (Bungie's term — never "reflexive"): 12 inline bytes
//!   `count(i32) + pointer(i32) + pointer(i32)` (pointers are runtime
//!   garbage in loose tags). When `count>0`, child elements follow as
//!   `count * element_size` contiguous fixed bytes, *then* each element's
//!   own nested variable data in element order. Halo CE has **no**
//!   per-block tag-block header (that is H2+).
//! - **data reference**: 20 inline bytes (`size` + 4 runtime words),
//!   then `size` blob bytes trailing.
//! - **tag reference**: 16 inline bytes (`group` + ptr + `length` +
//!   tag_id); `group == -1` ⇒ null (no path), else `length+1` trailing
//!   bytes (path + NUL).
//! - **string**: 32 inline bytes, in place (no trailing).
//! - **string id** (H2 only): 4 inline bytes (`pad:u16` + `length:u16`,
//!   big-endian) then `length` trailing bytes.
//!
//! Decoder and encoder share one depth-first traversal and preserve
//! every byte verbatim (fixed bytes in `raw_data`, blobs/paths/children
//! in `sub_chunks`), so read→write is byte-exact by construction.

use crate::data::{TagBlockData, TagStructData, TagSubChunkContent, TagSubChunkEntry};
use crate::fields::TagFieldType;
use crate::file::{TagContainer, TagFile, TagFileHeader};
use crate::io::Endian;
use crate::layout::TagLayout;
use crate::stream::TagStream;

/// Which classic engine a tag belongs to. Selects signature byte order
/// and a couple of per-field encoding quirks (string_id, group reversal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassicEngine {
    /// Halo 1 / Halo: Combat Evolved (Anniversary). Big-endian signature
    /// words, inline 32-byte strings, no string_id table.
    HaloCe,
    /// Halo 2. Little-endian (reversed) signature words, length-prefixed
    /// string_id values with trailing bytes.
    Halo2,
}

impl ClassicEngine {
    /// Numeric body fields (ints/floats/counts/lengths) byte order.
    /// **CE is big-endian** (MCC's CE tag set is the CEA / Xbox-360
    /// PowerPC-derived form — HABT reads H1 with `file_endian=">"`).
    /// **H2 is little-endian** (x86).
    pub fn body_endian(self) -> Endian {
        match self {
            ClassicEngine::HaloCe => Endian::Be,
            ClassicEngine::Halo2 => Endian::Le,
        }
    }
}

/// Errors raised while decoding a classic tag body.
#[derive(Debug)]
pub enum ClassicError {
    /// The file is shorter than the 64-byte classic header.
    ShortHeader,
    /// The offset-60 engine word isn't a recognized classic engine —
    /// the caller should route to the MCC reader instead.
    NotClassic,
    /// Ran off the end of the body while reading a fixed/variable region.
    UnexpectedEof {
        context: &'static str,
        need: usize,
        have: usize,
    },
    /// Body had trailing bytes the layout-driven walk never consumed.
    TrailingBytes { consumed: usize, total: usize },
}

impl std::fmt::Display for ClassicError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClassicError::ShortHeader => write!(f, "file shorter than 64-byte classic header"),
            ClassicError::NotClassic => write!(f, "not a classic (CE/H2) tag header"),
            ClassicError::UnexpectedEof { context, need, have } => {
                write!(f, "unexpected EOF reading {context}: need {need} bytes, have {have}")
            }
            ClassicError::TrailingBytes { consumed, total } => {
                write!(f, "layout walk consumed {consumed} of {total} body bytes")
            }
        }
    }
}

impl std::error::Error for ClassicError {}

/// The 64-byte classic tag-file header.
#[derive(Debug, Clone)]
pub struct ClassicHeader {
    /// Logical group tag (e.g. `b"bitm"`), un-reversed.
    pub group_tag: [u8; 4],
    /// Logical engine signature (e.g. `b"blam"`, `b"BLM!"`), un-reversed.
    pub engine: [u8; 4],
    /// Tag-file format version word at offset 56.
    pub version: u16,
    /// Stored body checksum (offset 40). `0xFFFFFFFF` is the
    /// "unchecksummed" sentinel some HEK tags ship with.
    pub checksum: u32,
}

impl ClassicHeader {
    /// Parse the 64-byte header and classify the engine from the offset-60
    /// signature word. Returns `None` if the signature isn't a known
    /// classic engine (so the caller can fall through to the MCC reader).
    pub fn parse(bytes: &[u8]) -> Option<(ClassicHeader, ClassicEngine)> {
        if bytes.len() < 64 {
            return None;
        }
        let raw_engine: [u8; 4] = bytes[60..64].try_into().ok()?;
        let raw_group: [u8; 4] = bytes[36..40].try_into().ok()?;

        // Read the engine word as a u32 in both orders and match against
        // the known classic engines. The matching order tells us how the
        // engine stores all its signature words.
        let be = u32::from_be_bytes(raw_engine);
        let le = u32::from_le_bytes(raw_engine);

        // CE: big-endian "blam". H2: little-endian "BLM!"/"LAMB"/"MLAB"/"BALM".
        let (engine, group_tag, eng_le) = if be == u32::from_be_bytes(*b"blam") {
            (ClassicEngine::HaloCe, raw_group, false)
        } else if matches!(
            &le.to_be_bytes(),
            b"BLM!" | b"LAMB" | b"MLAB" | b"BALM"
        ) {
            // Reverse the on-disk (little-endian) bytes to get logical order.
            let mut g = raw_group;
            g.reverse();
            (ClassicEngine::Halo2, g, true)
        } else {
            return None;
        };

        let engine_logical = if eng_le {
            let mut e = raw_engine;
            e.reverse();
            e
        } else {
            raw_engine
        };

        // Version + checksum word byte order matches the engine's
        // signature order: CE big-endian, H2 little-endian.
        let vb = [bytes[56], bytes[57]];
        let cb: [u8; 4] = bytes[40..44].try_into().ok()?;
        let (version, checksum) = match engine {
            ClassicEngine::HaloCe => (u16::from_be_bytes(vb), u32::from_be_bytes(cb)),
            ClassicEngine::Halo2 => (u16::from_le_bytes(vb), u32::from_le_bytes(cb)),
        };
        Some((
            ClassicHeader { group_tag, engine: engine_logical, version, checksum },
            engine,
        ))
    }
}

/// A forward-only cursor over the classic tag body.
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Cursor { data, pos: 0 }
    }

    fn take(&mut self, n: usize, context: &'static str) -> Result<&'a [u8], ClassicError> {
        let end = self.pos.checked_add(n).ok_or(ClassicError::UnexpectedEof {
            context,
            need: n,
            have: self.data.len().saturating_sub(self.pos),
        })?;
        if end > self.data.len() {
            return Err(ClassicError::UnexpectedEof {
                context,
                need: n,
                have: self.data.len() - self.pos,
            });
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }
}

fn rd_u32(raw: &[u8], off: usize, endian: Endian) -> u32 {
    let b: [u8; 4] = raw[off..off + 4].try_into().unwrap();
    match endian {
        Endian::Le => u32::from_le_bytes(b),
        Endian::Be => u32::from_be_bytes(b),
    }
}

fn rd_i32(raw: &[u8], off: usize, endian: Endian) -> i32 {
    rd_u32(raw, off, endian) as i32
}

/// Decode then re-encode a classic tag body, returning the re-encoded
/// bytes. The byte-exact roundtrip gate: `classic_roundtrip(body, ..) ==
/// body` for a well-formed tag. Keeps the internal
/// [`TagBlockData`] model crate-private.
pub fn classic_roundtrip(
    body: &[u8],
    layout: &TagLayout,
    engine: ClassicEngine,
) -> Result<Vec<u8>, ClassicError> {
    let root = read_classic_body(body, layout, engine)?;
    Ok(write_classic_body(layout, &root))
}

/// Read a complete classic (Halo CE / H2) tag into a [`TagFile`], using
/// `layout` (synthesized from the group's JSON def) for structure. The
/// resulting `TagFile` behaves like any MCC-loaded tag — the same
/// navigation/inspect API, field readers, and extractors apply (field
/// byte order comes from the engine: CE big-endian, H2 little-endian).
///
/// Returns [`ClassicError::NotClassic`] if the offset-60 engine word
/// isn't a known classic engine (caller should route to the MCC reader).
pub fn read_classic_tag_file(bytes: &[u8], layout: TagLayout) -> Result<TagFile, ClassicError> {
    let (header, engine) = ClassicHeader::parse(bytes).ok_or(ClassicError::NotClassic)?;
    let body = &bytes[64..];
    let root = read_classic_body(body, &layout, engine)?;

    let file_header = TagFileHeader {
        pad: [0u8; 36],
        build_version: 0,
        build_number: 0,
        version: header.version as u32,
        group_tag: u32::from_be_bytes(header.group_tag),
        group_version: 0,
        // Preserve the original stored checksum; the writer recomputes
        // it unless it's the "unchecksummed" 0xFFFFFFFF sentinel.
        checksum: header.checksum,
        signature: u32::from_be_bytes(*b"BLAM"),
    };

    Ok(TagFile::from_parts(
        file_header,
        TagContainer::Classic { engine, header: bytes[0..64].to_vec() },
        engine.body_endian(),
        TagStream { layout, data: root },
    ))
}

/// CRC32 table (poly `0xEDB88320`). Built at compile time.
const CRC_TABLE: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut r = i as u32;
        let mut j = 0;
        while j < 8 {
            r = if r & 1 == 1 { (r >> 1) ^ 0xEDB8_8320 } else { r >> 1 };
            j += 1;
        }
        t[i] = r;
        i += 1;
    }
    t
};

/// Classic tag checksum: CRC32 (poly `0xEDB88320`, init `0xFFFFFFFF`)
/// over the body, with **no final XOR inversion** (matches HABT
/// `checksum_calculate`). Verified against real CE tags.
pub fn classic_checksum(body: &[u8]) -> u32 {
    let mut c: u32 = 0xFFFF_FFFF;
    for &b in body {
        let idx = ((c ^ b as u32) & 0xFF) as usize;
        c = CRC_TABLE[idx] ^ (c >> 8);
    }
    c
}

/// Serialize a complete classic tag: the original 64-byte header
/// preserved verbatim (with only the checksum patched), followed by the
/// re-encoded flat body. The first 36 header bytes can carry per-tag
/// build strings, so reconstructing from scratch would lose them —
/// preserving keeps every byte while still recomputing the checksum.
///
/// The checksum is recomputed from the body, except the "unchecksummed"
/// `0xFFFFFFFF` sentinel (some HEK tags) is preserved so an unmodified
/// tag round-trips byte-exact.
pub(crate) fn write_classic_tag(file: &TagFile, engine: ClassicEngine, header: &[u8]) -> Vec<u8> {
    let body = write_classic_body(&file.tag_stream.layout, &file.tag_stream.data);
    let checksum = if file.header.checksum == 0xFFFF_FFFF {
        0xFFFF_FFFF
    } else {
        classic_checksum(&body)
    };
    let checksum_bytes = match engine {
        ClassicEngine::HaloCe => checksum.to_be_bytes(),
        ClassicEngine::Halo2 => checksum.to_le_bytes(),
    };

    let mut out = Vec::with_capacity(64 + body.len());
    out.extend_from_slice(header);
    out[40..44].copy_from_slice(&checksum_bytes);
    out.extend_from_slice(&body);
    out
}

/// Read a Halo 2 block header (16 bytes, little-endian): `4cc name +
/// version(i32) + count(i32) + size(i32)`. The `size` is the element
/// struct size (H2 blocks are self-describing). Returns the raw header
/// bytes (preserved for write) plus the count + element size.
fn read_h2_block_header(cur: &mut Cursor) -> Result<(Vec<u8>, usize, usize), ClassicError> {
    let h = cur.take(16, "h2 block header")?.to_vec();
    let count = u32::from_le_bytes([h[8], h[9], h[10], h[11]]) as usize;
    let size = u32::from_le_bytes([h[12], h[13], h[14], h[15]]) as usize;
    Ok((h, count, size))
}

/// Decode a classic tag body (everything after the 64-byte header) into
/// the root [`TagBlockData`] using `layout` for structure.
pub(crate) fn read_classic_body(
    body: &[u8],
    layout: &TagLayout,
    engine: ClassicEngine,
) -> Result<TagBlockData, ClassicError> {
    let endian = engine.body_endian();
    let mut cur = Cursor::new(body);

    let root_block_index = layout.header.tag_group_block_index;
    let struct_index = layout.block_layouts[root_block_index as usize].struct_index;

    // Halo 2: a 16-byte block header (count + element size) leads the
    // body. Halo CE: headerless — the single root element leads directly.
    let (header, count, elem_size) = match engine {
        ClassicEngine::HaloCe => (None, 1usize, layout.struct_layouts[struct_index as usize].size),
        ClassicEngine::Halo2 => {
            let (h, c, s) = read_h2_block_header(&mut cur)?;
            (Some(h), c, s)
        }
    };

    let raw_data = cur.take(count * elem_size, "root struct").map(<[u8]>::to_vec)?;
    let mut elements = Vec::with_capacity(count);
    for i in 0..count {
        let elem_raw = raw_data[i * elem_size..(i + 1) * elem_size].to_vec();
        let sub = decode_struct_trailing(layout, struct_index, &elem_raw, &mut cur, engine, endian)?;
        elements.push(TagStructData { struct_index, sub_chunks: sub });
    }

    if cur.pos != body.len() {
        return Err(ClassicError::TrailingBytes { consumed: cur.pos, total: body.len() });
    }

    Ok(TagBlockData {
        block_index: root_block_index,
        flags: 0,
        raw_data,
        endian,
        elements,
        classic_block_header: header,
    })
}

/// Walk a struct's sub-chunk-bearing fields in order, pulling each
/// field's trailing/variable data from the cursor and building the
/// `sub_chunks` list. `raw` is the struct's already-read fixed bytes.
fn decode_struct_trailing(
    layout: &TagLayout,
    struct_index: u32,
    raw: &[u8],
    cur: &mut Cursor,
    engine: ClassicEngine,
    endian: Endian,
) -> Result<Vec<TagSubChunkEntry>, ClassicError> {
    let mut entries = Vec::new();
    let first = layout.struct_layouts[struct_index as usize].first_field_index as usize;
    let mut fi = first;
    loop {
        let field = &layout.fields[fi];
        if field.field_type == TagFieldType::Terminator {
            break;
        }
        let off = field.offset as usize;
        // Guard against a desynced cursor (e.g. an unhandled H2 inline
        // struct header) leaving `off` past this element's fixed bytes —
        // fail cleanly instead of panicking on an out-of-range slice.
        if matches!(
            field.field_type,
            TagFieldType::Block
                | TagFieldType::Data
                | TagFieldType::TagReference
                | TagFieldType::StringId
                | TagFieldType::Struct
                | TagFieldType::Array
        ) {
            let end = match field.field_type {
                TagFieldType::Struct => {
                    off + layout.struct_layouts[field.definition as usize].size
                }
                TagFieldType::Array => {
                    let a = &layout.array_layouts[field.definition as usize];
                    off + layout.struct_layouts[a.struct_index as usize].size * a.count as usize
                }
                TagFieldType::TagReference => off + 16,
                _ => off + 4,
            };
            if end > raw.len() {
                return Err(ClassicError::UnexpectedEof {
                    context: "struct fixed field (cursor desync?)",
                    need: end,
                    have: raw.len(),
                });
            }
        }
        match field.field_type {
            TagFieldType::Block => {
                let count = rd_u32(raw, off, endian) as usize;
                let block = decode_block(layout, field.definition, count, cur, engine, endian)?;
                entries.push(TagSubChunkEntry {
                    field_index: Some(fi as u32),
                    content: TagSubChunkContent::Block(block),
                });
            }
            TagFieldType::Data => {
                let len = rd_u32(raw, off, endian) as usize;
                let blob = cur.take(len, "data blob")?.to_vec();
                entries.push(TagSubChunkEntry {
                    field_index: Some(fi as u32),
                    content: TagSubChunkContent::Data(blob),
                });
            }
            TagFieldType::TagReference => {
                let group = rd_i32(raw, off, endian);
                let len = rd_u32(raw, off + 8, endian) as usize;
                // The MCC `TagReference` payload is `group_tag(4) +
                // null-terminated path`. The classic inline header keeps
                // the group (raw[off..off+4]); only `path + NUL` is
                // trailing. Prepend the inline group bytes so the
                // payload matches the MCC convention the field
                // readers/renderers expect — the encoder strips them
                // back off (the group stays in raw_data, so byte-exact).
                let mut payload = raw[off..off + 4].to_vec();
                if group != -1 && len != 0 {
                    // path + NUL terminator are stored trailing.
                    payload.extend_from_slice(cur.take(len + 1, "tag_reference path")?);
                }
                entries.push(TagSubChunkEntry {
                    field_index: Some(fi as u32),
                    content: TagSubChunkContent::TagReference(payload),
                });
            }
            TagFieldType::StringId => {
                // H2: inline (pad:u16, length:u16) big-endian, then
                // `length` trailing string bytes.
                let len = u16::from_be_bytes([raw[off + 2], raw[off + 3]]) as usize;
                let s = cur.take(len, "string_id value")?.to_vec();
                entries.push(TagSubChunkEntry {
                    field_index: Some(fi as u32),
                    content: TagSubChunkContent::StringId(s),
                });
            }
            TagFieldType::Struct => {
                let child_si = field.definition;
                let child_size = layout.struct_layouts[child_si as usize].size;
                let child_raw = &raw[off..off + child_size];
                let child_sub =
                    decode_struct_trailing(layout, child_si, child_raw, cur, engine, endian)?;
                entries.push(TagSubChunkEntry {
                    field_index: Some(fi as u32),
                    content: TagSubChunkContent::Struct(TagStructData {
                        struct_index: child_si,
                        sub_chunks: child_sub,
                    }),
                });
            }
            TagFieldType::Array => {
                let array = &layout.array_layouts[field.definition as usize];
                let (asi, count) = (array.struct_index, array.count as usize);
                let es = layout.struct_layouts[asi as usize].size;
                let mut elems = Vec::with_capacity(count);
                for i in 0..count {
                    let elem_raw = &raw[off + i * es..off + (i + 1) * es];
                    let sub = decode_struct_trailing(layout, asi, elem_raw, cur, engine, endian)?;
                    elems.push(TagStructData { struct_index: asi, sub_chunks: sub });
                }
                entries.push(TagSubChunkEntry {
                    field_index: Some(fi as u32),
                    content: TagSubChunkContent::Array(elems),
                });
            }
            // Everything else lives entirely in the struct's fixed bytes
            // (already captured in `raw`) — nothing trailing to read.
            _ => {}
        }
        fi += 1;
    }
    Ok(entries)
}

/// Decode a tag block's `count` elements: all element fixed bytes are
/// contiguous, then each element's nested variable data in element order.
fn decode_block(
    layout: &TagLayout,
    block_index: u32,
    inline_count: usize,
    cur: &mut Cursor,
    engine: ClassicEngine,
    endian: Endian,
) -> Result<TagBlockData, ClassicError> {
    let struct_index = layout.block_layouts[block_index as usize].struct_index;

    // An empty block has no on-disk presence (no H2 header, no elements).
    if inline_count == 0 {
        return Ok(TagBlockData {
            block_index,
            flags: 0,
            raw_data: Vec::new(),
            endian,
            elements: Vec::new(),
            classic_block_header: None,
        });
    }

    // Halo 2: a 16-byte block header (authoritative count + element
    // size) precedes the elements. Halo CE: headerless, size from layout.
    let (header, count, elem_size) = match engine {
        ClassicEngine::HaloCe => {
            (None, inline_count, layout.struct_layouts[struct_index as usize].size)
        }
        ClassicEngine::Halo2 => {
            let (h, c, s) = read_h2_block_header(cur)?;
            (Some(h), c, s)
        }
    };

    let raw_data = cur.take(count * elem_size, "block elements")?.to_vec();

    let mut elements = Vec::with_capacity(count);
    for i in 0..count {
        let elem_raw = raw_data[i * elem_size..(i + 1) * elem_size].to_vec();
        let sub = decode_struct_trailing(layout, struct_index, &elem_raw, cur, engine, endian)?;
        elements.push(TagStructData { struct_index, sub_chunks: sub });
    }

    Ok(TagBlockData {
        block_index,
        flags: 0,
        raw_data,
        endian,
        elements,
        classic_block_header: header,
    })
}

/// Re-encode a classic tag body from the root [`TagBlockData`].
///
/// Inline counts/lengths (tag-block element count, data size, tag-ref
/// path length + group, string_id length) are **derived from the live
/// model** before each struct's fixed bytes are emitted — so structural
/// edits (add/remove block elements, resize data) serialize correctly.
/// The adjacent runtime-pointer bytes are left untouched. For an
/// unmodified tree this is a no-op, so read→write stays byte-exact.
pub(crate) fn write_classic_body(layout: &TagLayout, root: &TagBlockData) -> Vec<u8> {
    let mut out = Vec::new();
    // The root is just a block (1 element) — emit it the same way,
    // including the Halo 2 block header if present.
    encode_block(layout, root, &mut out);
    out
}

#[inline]
fn wr_u32(raw: &mut [u8], off: usize, v: u32, endian: Endian) {
    let b = match endian {
        Endian::Le => v.to_le_bytes(),
        Endian::Be => v.to_be_bytes(),
    };
    raw[off..off + 4].copy_from_slice(&b);
}

/// Rewrite a struct's inline count/length slots from the live model,
/// recursing through inline structs/arrays. Leaves runtime-pointer bytes
/// (the words after each count/size) intact.
fn sync_fixed_counts(layout: &TagLayout, raw: &mut [u8], elem: &TagStructData, endian: Endian) {
    let first = layout.struct_layouts[elem.struct_index as usize].first_field_index as usize;
    let mut fi = first;
    loop {
        let field = &layout.fields[fi];
        if field.field_type == TagFieldType::Terminator {
            break;
        }
        let off = field.offset as usize;
        let content = elem
            .sub_chunks
            .iter()
            .find(|e| e.field_index == Some(fi as u32))
            .map(|e| &e.content);
        match (field.field_type, content) {
            (TagFieldType::Block, Some(TagSubChunkContent::Block(b))) => {
                // count(i32) + 2 runtime pointers; rewrite only the count.
                wr_u32(raw, off, b.elements.len() as u32, endian);
            }
            (TagFieldType::Data, Some(TagSubChunkContent::Data(blob))) => {
                // size(i32) + 4 runtime words; rewrite only the size.
                wr_u32(raw, off, blob.len() as u32, endian);
            }
            (TagFieldType::TagReference, Some(TagSubChunkContent::TagReference(p))) => {
                // 16-byte inline header: group(4) + ptr(4) + length(4) +
                // tag_id(4). Payload is `group(4) + path + NUL`. Sync the
                // group + the path length (excluding the NUL).
                if p.len() >= 4 {
                    raw[off..off + 4].copy_from_slice(&p[0..4]);
                }
                let path_len = p.len().saturating_sub(5); // 4 group + 1 NUL
                wr_u32(raw, off + 8, path_len as u32, endian);
            }
            (TagFieldType::StringId, Some(TagSubChunkContent::StringId(s))) => {
                // H2 inline: (pad:u16, length:u16) big-endian. Sync length.
                raw[off + 2..off + 4].copy_from_slice(&(s.len() as u16).to_be_bytes());
            }
            (TagFieldType::Struct, Some(TagSubChunkContent::Struct(child))) => {
                let csz = layout.struct_layouts[child.struct_index as usize].size;
                sync_fixed_counts(layout, &mut raw[off..off + csz], child, endian);
            }
            (TagFieldType::Array, Some(TagSubChunkContent::Array(elems))) => {
                let asi = layout.array_layouts[field.definition as usize].struct_index;
                let es = layout.struct_layouts[asi as usize].size;
                for (i, child) in elems.iter().enumerate() {
                    sync_fixed_counts(layout, &mut raw[off + i * es..off + (i + 1) * es], child, endian);
                }
            }
            _ => {}
        }
        fi += 1;
    }
}

fn encode_struct_trailing(layout: &TagLayout, elem: &TagStructData, out: &mut Vec<u8>) {
    let first = layout.struct_layouts[elem.struct_index as usize].first_field_index as usize;
    let mut fi = first;
    loop {
        let field = &layout.fields[fi];
        if field.field_type == TagFieldType::Terminator {
            break;
        }
        if matches!(
            field.field_type,
            TagFieldType::Block
                | TagFieldType::Data
                | TagFieldType::TagReference
                | TagFieldType::StringId
                | TagFieldType::Struct
                | TagFieldType::Array
        ) {
            let content = elem
                .sub_chunks
                .iter()
                .find(|e| e.field_index == Some(fi as u32))
                .map(|e| &e.content);
            if let Some(content) = content {
                match content {
                    TagSubChunkContent::Block(block) => encode_block(layout, block, out),
                    TagSubChunkContent::Data(blob) => out.extend_from_slice(blob),
                    // Payload is `group_tag(4) + path + NUL`; the group
                    // already lives in raw_data, so only `path + NUL`
                    // (payload[4..]) is trailing on disk.
                    TagSubChunkContent::TagReference(p) => {
                        if p.len() > 4 {
                            out.extend_from_slice(&p[4..]);
                        }
                    }
                    TagSubChunkContent::StringId(s) => out.extend_from_slice(s),
                    TagSubChunkContent::Struct(child) => encode_struct_trailing(layout, child, out),
                    TagSubChunkContent::Array(elems) => {
                        for child in elems {
                            encode_struct_trailing(layout, child, out);
                        }
                    }
                    _ => {}
                }
            }
        }
        fi += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::classic_checksum;

    #[test]
    fn checksum_matches_crc32_without_final_xor() {
        // Standard CRC32("123456789") = 0xCBF43926 (with final inversion).
        // The classic algorithm omits the final XOR, so the result is the
        // standard value XOR 0xFFFFFFFF.
        assert_eq!(classic_checksum(b"123456789"), 0xCBF4_3926 ^ 0xFFFF_FFFF);
        // Empty input returns the init value (no bytes processed).
        assert_eq!(classic_checksum(b""), 0xFFFF_FFFF);
    }
}

fn encode_block(layout: &TagLayout, block: &TagBlockData, out: &mut Vec<u8>) {
    let elem_count = block.elements.len();
    // Use the on-disk element size (raw_data is preserved verbatim) so
    // the Halo 2 header's authoritative size is honoured even when the
    // synthesized layout size disagrees.
    let sz = if elem_count > 0 { block.raw_data.len() / elem_count } else { 0 };
    let mut raw = block.raw_data.clone();
    if sz > 0 {
        for (i, elem) in block.elements.iter().enumerate() {
            sync_fixed_counts(layout, &mut raw[i * sz..(i + 1) * sz], elem, block.endian);
        }
    }
    // Halo 2: re-emit the 16-byte block header, re-syncing the count
    // (offset 8, LE i32) from the live element count. Empty H2 blocks
    // carry no header (None).
    if let Some(hdr) = &block.classic_block_header {
        let mut h = hdr.clone();
        h[8..12].copy_from_slice(&(elem_count as u32).to_le_bytes());
        out.extend_from_slice(&h);
    }
    out.extend_from_slice(&raw);
    for elem in &block.elements {
        encode_struct_trailing(layout, elem, out);
    }
}
