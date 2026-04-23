//! Field-level types.
//!
//! [`TagFieldType`] is the resolved, dispatch-ready enum form of a tag
//! field's type-name string — computed once per field during layout read
//! and then used in every hot read/write path to avoid re-comparing
//! strings.
//!
//! [`TagFieldData`] is the typed, per-field value representation
//! produced by `deserialize_field` and consumed by `serialize_field`.
//! It sits on top of the raw-data + sub-chunks roundtrip path in
//! [`crate::data`] — sub-chunk payloads come in as `Vec<u8>` and already
//! stripped of their outer chunk header, so the parse functions here
//! operate on payload bytes rather than readers.

use std::fmt;

use crate::data::TagSubChunkContent;
use crate::layout::{TagFieldLayout, TagLayout};
use crate::math;

/// Resolved type of a layout field.
///
/// Computed once per [`crate::layout::TagFieldLayout`] during
/// [`crate::layout::TagLayout::read`] by [`TagFieldType::from_name`], so
/// the hot read/write paths can dispatch on an enum instead of comparing
/// type-name strings on every field.
///
/// Variant names mirror the Halo type-name strings (e.g. `"real point 3d"
/// → RealPoint3d`). `Unknown` is the fallback for any unrecognized name
/// and also the initial value before resolution.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TagFieldType {
    Unknown,
    String,
    LongString,
    StringId,
    OldStringId,
    CharInteger,
    ShortInteger,
    LongInteger,
    Int64Integer,
    Angle,
    Tag,
    CharEnum,
    ShortEnum,
    LongEnum,
    LongFlags,
    WordFlags,
    ByteFlags,
    Point2d,
    Rectangle2d,
    RgbColor,
    ArgbColor,
    Real,
    RealSlider,
    RealFraction,
    RealPoint2d,
    RealPoint3d,
    RealVector2d,
    RealVector3d,
    RealQuaternion,
    RealEulerAngles2d,
    RealEulerAngles3d,
    RealPlane2d,
    RealPlane3d,
    RealRgbColor,
    RealArgbColor,
    RealHsvColor,
    RealAhsvColor,
    ShortIntegerBounds,
    AngleBounds,
    RealBounds,
    FractionBounds,
    TagReference,
    Block,
    LongBlockFlags,
    WordBlockFlags,
    ByteBlockFlags,
    CharBlockIndex,
    CustomCharBlockIndex,
    ShortBlockIndex,
    CustomShortBlockIndex,
    LongBlockIndex,
    CustomLongBlockIndex,
    Data,
    VertexBuffer,
    Pad,
    UselessPad,
    Skip,
    Explanation,
    Custom,
    Struct,
    Array,
    PageableResource,
    ApiInterop,
    Terminator,
}

impl TagFieldType {
    /// Parse a field-type name string (as stored in the layout's
    /// `string_data`) into the dispatch enum. Unknown names are
    /// tolerated and returned as [`TagFieldType::Unknown`] — this is
    /// how we detect new or unhandled type names without panicking.
    pub fn from_name(name: &str) -> Self {
        match name {
            "string" => Self::String,
            "long string" => Self::LongString,
            "string id" => Self::StringId,
            "old string id" => Self::OldStringId,
            "char integer" => Self::CharInteger,
            "short integer" => Self::ShortInteger,
            "long integer" => Self::LongInteger,
            "int64 integer" => Self::Int64Integer,
            "angle" => Self::Angle,
            "tag" => Self::Tag,
            "char enum" => Self::CharEnum,
            "short enum" => Self::ShortEnum,
            "long enum" => Self::LongEnum,
            "long flags" => Self::LongFlags,
            "word flags" => Self::WordFlags,
            "byte flags" => Self::ByteFlags,
            "point 2d" => Self::Point2d,
            "rectangle 2d" => Self::Rectangle2d,
            "rgb color" => Self::RgbColor,
            "argb color" => Self::ArgbColor,
            "real" => Self::Real,
            "real slider" => Self::RealSlider,
            "real fraction" => Self::RealFraction,
            "real point 2d" => Self::RealPoint2d,
            "real point 3d" => Self::RealPoint3d,
            "real vector 2d" => Self::RealVector2d,
            "real vector 3d" => Self::RealVector3d,
            "real quaternion" => Self::RealQuaternion,
            "real euler angles 2d" => Self::RealEulerAngles2d,
            "real euler angles 3d" => Self::RealEulerAngles3d,
            "real plane 2d" => Self::RealPlane2d,
            "real plane 3d" => Self::RealPlane3d,
            "real rgb color" => Self::RealRgbColor,
            "real argb color" => Self::RealArgbColor,
            "real hsv color" => Self::RealHsvColor,
            "real ahsv color" => Self::RealAhsvColor,
            "short integer bounds" => Self::ShortIntegerBounds,
            "angle bounds" => Self::AngleBounds,
            "real bounds" => Self::RealBounds,
            "fraction bounds" => Self::FractionBounds,
            "tag reference" => Self::TagReference,
            "block" => Self::Block,
            "long block flags" => Self::LongBlockFlags,
            "word block flags" => Self::WordBlockFlags,
            "byte block flags" => Self::ByteBlockFlags,
            "char block index" => Self::CharBlockIndex,
            "custom char block index" => Self::CustomCharBlockIndex,
            "short block index" => Self::ShortBlockIndex,
            "custom short block index" => Self::CustomShortBlockIndex,
            "long block index" => Self::LongBlockIndex,
            "custom long block index" => Self::CustomLongBlockIndex,
            "data" => Self::Data,
            "vertex buffer" => Self::VertexBuffer,
            "pad" => Self::Pad,
            "useless pad" => Self::UselessPad,
            "skip" => Self::Skip,
            "explanation" => Self::Explanation,
            "custom" => Self::Custom,
            "struct" => Self::Struct,
            "array" => Self::Array,
            "pageable resource" => Self::PageableResource,
            "api interop" => Self::ApiInterop,
            "terminator X" => Self::Terminator,
            _ => Self::Unknown,
        }
    }
}

//================================================================================
// Sub-chunk payload wrappers.
//
// These types own the *parsed* form of a sub-chunk payload — the outer
// chunk header (signature + version + size) is consumed by the data
// layer and never reaches this module. `from_bytes` takes the chunk's
// content bytes; `to_bytes` rebuilds them for serialization.
//================================================================================

/// Parsed form of a `tgrf` tag-reference chunk payload.
///
/// A null reference (payload shorter than the 4-byte group tag) is
/// represented as `None`; a resolved reference is `Some((group_tag,
/// path))`, where `path` is the UTF-8 tag path relative to the tag
/// root. No explicit terminator — the path fills the rest of the chunk.
#[derive(Debug)]
pub struct TagReferenceData {
    pub group_tag_and_name: Option<(u32, String)>,
}

impl TagReferenceData {
    /// Parse a `tgrf` payload (header already consumed).
    pub fn from_bytes(payload: &[u8]) -> Self {
        if payload.len() < 4 {
            return Self { group_tag_and_name: None };
        }
        let group_tag = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        let name = std::str::from_utf8(&payload[4..]).unwrap().to_string();
        Self { group_tag_and_name: Some((group_tag, name)) }
    }

    /// Serialize back to a `tgrf` payload (caller writes the header).
    pub fn to_bytes(&self) -> Vec<u8> {
        match &self.group_tag_and_name {
            None => Vec::new(),
            Some((group_tag, name)) => {
                let mut bytes = Vec::with_capacity(4 + name.len());
                bytes.extend_from_slice(&group_tag.to_le_bytes());
                bytes.extend_from_slice(name.as_bytes());
                bytes
            }
        }
    }
}

/// Parsed form of a `tgsi` string-id chunk payload. Used for both
/// [`TagFieldType::StringId`] and [`TagFieldType::OldStringId`].
/// Empty content represents `string_id::NONE`.
#[derive(Debug)]
pub struct StringIdData {
    pub string: String,
}

impl StringIdData {
    /// Parse a `tgsi` payload (header already consumed).
    pub fn from_bytes(payload: &[u8]) -> Self {
        Self { string: String::from_utf8_lossy(payload).to_string() }
    }

    /// Serialize back to a `tgsi` payload (caller writes the header).
    pub fn to_bytes(&self) -> Vec<u8> {
        self.string.as_bytes().to_vec()
    }
}

//================================================================================
// TagFieldData
//================================================================================

/// Parsed per-field value.
///
/// Carries only *values* — things that parse to a single Rust datum
/// displayable/editable on its own. Container field types (struct,
/// block, array, pageable_resource) are deliberately absent: they're
/// navigated via the raw-data + sub-chunks tree directly, since
/// materializing them here would duplicate subtree storage.
/// `deserialize_field` returns `None` for container / pad / skip /
/// explanation / terminator / api_interop / vertex_buffer fields.
///
/// **Enum and flags variants** carry the raw integer *and* resolved
/// name(s). Names are informational only — `serialize_field` writes
/// just the integer back to `raw_data`.
///
/// **Sub-chunk-bearing variants** (string_id, old_string_id,
/// tag_reference, data) carry their parsed payload. Sub-chunk
/// outer-header handling stays in [`crate::data`].
#[derive(Debug)]
pub enum TagFieldData {
    // Strings (fixed-size, null-padded on the wire).
    /// 32-byte null-padded UTF-8 buffer.
    String(String),
    /// 256-byte null-padded UTF-8 buffer.
    LongString(String),

    // Sub-chunk leaves.
    StringId(StringIdData),
    OldStringId(StringIdData),
    TagReference(TagReferenceData),
    Data(Vec<u8>),

    // Integers.
    CharInteger(i8),
    ShortInteger(i16),
    LongInteger(i32),
    Int64Integer(i64),
    Tag(u32),

    // Enums: raw value + resolved variant name (None if out of range
    // or no matching string_list entry).
    CharEnum { value: i8, name: Option<String> },
    ShortEnum { value: i16, name: Option<String> },
    LongEnum { value: i32, name: Option<String> },

    // Flags: raw value + names of set bits (bit index + display name).
    // Unset bits are omitted; bits past the string_list's `count` are
    // preserved in `value` but have no corresponding name entry.
    ByteFlags { value: u8, names: Vec<(u32, String)> },
    WordFlags { value: u16, names: Vec<(u32, String)> },
    LongFlags { value: i32, names: Vec<(u32, String)> },

    // Block flags: value only. TODO: resolve per-bit names against the
    // referenced block's current element count at parse time.
    ByteBlockFlags(u8),
    WordBlockFlags(u16),
    LongBlockFlags(i32),

    // Block indices.
    CharBlockIndex(i8),
    CustomCharBlockIndex(i8),
    ShortBlockIndex(i16),
    CustomShortBlockIndex(i16),
    LongBlockIndex(i32),
    CustomLongBlockIndex(i32),

    // Floats.
    Angle(f32),
    Real(f32),
    RealSlider(f32),
    RealFraction(f32),

    // Math composites.
    Point2d(math::Point2d),
    Rectangle2d(math::Rectangle2d),
    RealPoint2d(math::RealPoint2d),
    RealPoint3d(math::RealPoint3d),
    RealVector2d(math::RealVector2d),
    RealVector3d(math::RealVector3d),
    RealQuaternion(math::RealQuaternion),
    RealEulerAngles2d(math::RealEulerAngles2d),
    RealEulerAngles3d(math::RealEulerAngles3d),
    RealPlane2d(math::RealPlane2d),
    RealPlane3d(math::RealPlane3d),

    // Colors.
    RgbColor(math::RgbColor),
    ArgbColor(math::ArgbColor),
    RealRgbColor(math::RealRgbColor),
    RealArgbColor(math::RealArgbColor),
    RealHsvColor(math::RealHsvColor),
    RealAhsvColor(math::RealAhsvColor),

    // Bounds.
    ShortIntegerBounds(math::ShortBounds),
    AngleBounds(math::AngleBounds),
    RealBounds(math::RealBounds),
    FractionBounds(math::FractionBounds),

    // Opaque.
    Custom(Vec<u8>),
}

impl TagFieldData {
    /// Read a single bit from a flags-shaped variant (including
    /// block-flags). Returns `None` for variants that aren't flags.
    pub fn flag_bit(&self, bit: u32) -> Option<bool> {
        let raw = match self {
            TagFieldData::ByteFlags { value, .. } => *value as u64,
            TagFieldData::WordFlags { value, .. } => *value as u64,
            TagFieldData::LongFlags { value, .. } => *value as u32 as u64,
            TagFieldData::ByteBlockFlags(v) => *v as u64,
            TagFieldData::WordBlockFlags(v) => *v as u64,
            TagFieldData::LongBlockFlags(v) => *v as u32 as u64,
            _ => return None,
        };
        Some((raw & (1u64 << bit)) != 0)
    }

    /// Set or clear a single bit on a flags-shaped variant (including
    /// block-flags). Returns `true` on success, `false` if `self`
    /// isn't a flags variant. The bit is mutated in place; the
    /// variant's `names` field (if any) is left untouched — callers
    /// re-derive it by re-parsing if they need accurate names after
    /// mutation.
    pub fn set_flag_bit(&mut self, bit: u32, on: bool) -> bool {
        let mask = 1u64 << bit;
        let apply = |raw: u64| -> u64 { if on { raw | mask } else { raw & !mask } };
        match self {
            TagFieldData::ByteFlags { value, .. } => {
                *value = apply(*value as u64) as u8;
                true
            }
            TagFieldData::WordFlags { value, .. } => {
                *value = apply(*value as u64) as u16;
                true
            }
            TagFieldData::LongFlags { value, .. } => {
                *value = apply(*value as u32 as u64) as u32 as i32;
                true
            }
            TagFieldData::ByteBlockFlags(v) => {
                *v = apply(*v as u64) as u8;
                true
            }
            TagFieldData::WordBlockFlags(v) => {
                *v = apply(*v as u64) as u16;
                true
            }
            TagFieldData::LongBlockFlags(v) => {
                *v = apply(*v as u32 as u64) as u32 as i32;
                true
            }
            _ => false,
        }
    }
}

//================================================================================
// Display
//================================================================================

/// Render a `tgrf` payload as `"GROUP:path"`, or `"NONE"` for a null
/// reference.
impl fmt::Display for TagReferenceData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.group_tag_and_name {
            None => f.write_str("NONE"),
            Some((tag, path)) => write!(f, "{}:{}", format_group_tag(*tag), path),
        }
    }
}

/// Render a `tgsi` payload as the quoted string `"name"`, or `"NONE"`
/// for an empty (sentinel) string-id.
impl fmt::Display for StringIdData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.string.is_empty() {
            f.write_str("NONE")
        } else {
            write!(f, "\"{}\"", self.string)
        }
    }
}

/// Default rendering of a field value. The `{:#}` alternate flag
/// switches the four plain integer variants (`CharInteger`,
/// `ShortInteger`, `LongInteger`, `Int64Integer`) to fixed-width hex
/// (`0xNN` / `0xNNNN` / `0xNNNNNNNN` / `0xNNNNNNNNNNNNNNNN`).
/// Block-flags, colors, and block-index sentinels always render in
/// their canonical form regardless of the flag.
impl fmt::Display for TagFieldData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex = f.alternate();
        match self {
            TagFieldData::String(s) | TagFieldData::LongString(s) => write!(f, "\"{}\"", s),

            TagFieldData::StringId(s) | TagFieldData::OldStringId(s) => write!(f, "{}", s),
            TagFieldData::TagReference(r) => write!(f, "{}", r),
            TagFieldData::Data(d) => write!(f, "data [{} bytes]", d.len()),

            TagFieldData::CharInteger(v) => {
                if hex { write!(f, "0x{:02X}", *v as u8) } else { write!(f, "{}", v) }
            }
            TagFieldData::ShortInteger(v) => {
                if hex { write!(f, "0x{:04X}", *v as u16) } else { write!(f, "{}", v) }
            }
            TagFieldData::LongInteger(v) => {
                if hex { write!(f, "0x{:08X}", *v as u32) } else { write!(f, "{}", v) }
            }
            TagFieldData::Int64Integer(v) => {
                if hex { write!(f, "0x{:016X}", *v as u64) } else { write!(f, "{}", v) }
            }
            TagFieldData::Tag(v) => f.write_str(&format_group_tag(*v)),

            TagFieldData::CharEnum { value, name } => write_enum(f, *value as i64, name.as_deref()),
            TagFieldData::ShortEnum { value, name } => write_enum(f, *value as i64, name.as_deref()),
            TagFieldData::LongEnum { value, name } => write_enum(f, *value as i64, name.as_deref()),

            TagFieldData::ByteFlags { value, names } => write_flags(f, *value as u64, names, 2),
            TagFieldData::WordFlags { value, names } => write_flags(f, *value as u64, names, 4),
            TagFieldData::LongFlags { value, names } => write_flags(f, *value as u32 as u64, names, 8),

            TagFieldData::ByteBlockFlags(v) => write!(f, "0x{:02X}", v),
            TagFieldData::WordBlockFlags(v) => write!(f, "0x{:04X}", v),
            TagFieldData::LongBlockFlags(v) => write!(f, "0x{:08X}", *v as u32),

            TagFieldData::CharBlockIndex(v) | TagFieldData::CustomCharBlockIndex(v) => write_block_index(f, *v as i64),
            TagFieldData::ShortBlockIndex(v) | TagFieldData::CustomShortBlockIndex(v) => write_block_index(f, *v as i64),
            TagFieldData::LongBlockIndex(v) | TagFieldData::CustomLongBlockIndex(v) => write_block_index(f, *v as i64),

            TagFieldData::Angle(v) => write!(f, "{:.4} rad ({:.2} deg)", v, v.to_degrees()),
            TagFieldData::Real(v) | TagFieldData::RealSlider(v) | TagFieldData::RealFraction(v) => write!(f, "{}", v),

            TagFieldData::Point2d(p) => write!(f, "{}, {}", p.x, p.y),
            TagFieldData::Rectangle2d(r) => write!(f, "{}, {}, {}, {}", r.top, r.left, r.bottom, r.right),
            TagFieldData::RealPoint2d(p) => write!(f, "x={}, y={}", p.x, p.y),
            TagFieldData::RealPoint3d(p) => write!(f, "x={}, y={}, z={}", p.x, p.y, p.z),
            TagFieldData::RealVector2d(v) => write!(f, "i={}, j={}", v.i, v.j),
            TagFieldData::RealVector3d(v) => write!(f, "i={}, j={}, k={}", v.i, v.j, v.k),
            TagFieldData::RealQuaternion(q) => write!(f, "i={}, j={}, k={}, w={}", q.i, q.j, q.k, q.w),
            TagFieldData::RealEulerAngles2d(e) => write!(f, "yaw={}, pitch={}", e.yaw, e.pitch),
            TagFieldData::RealEulerAngles3d(e) => write!(f, "yaw={}, pitch={}, roll={}", e.yaw, e.pitch, e.roll),
            TagFieldData::RealPlane2d(p) => write!(f, "i={}, j={}, d={}", p.i, p.j, p.d),
            TagFieldData::RealPlane3d(p) => write!(f, "i={}, j={}, k={}, d={}", p.i, p.j, p.k, p.d),

            TagFieldData::RgbColor(c) => write!(f, "0x{:08X}", c.0),
            TagFieldData::ArgbColor(c) => write!(f, "0x{:08X}", c.0),
            TagFieldData::RealRgbColor(c) => write!(f, "r={}, g={}, b={}", c.red, c.green, c.blue),
            TagFieldData::RealArgbColor(c) => write!(f, "a={}, r={}, g={}, b={}", c.alpha, c.red, c.green, c.blue),
            TagFieldData::RealHsvColor(c) => write!(f, "h={}, s={}, v={}", c.hue, c.saturation, c.value),
            TagFieldData::RealAhsvColor(c) => write!(f, "a={}, h={}, s={}, v={}", c.alpha, c.hue, c.saturation, c.value),

            TagFieldData::ShortIntegerBounds(b) => write!(f, "{}..{}", b.lower, b.upper),
            TagFieldData::AngleBounds(b) | TagFieldData::RealBounds(b) | TagFieldData::FractionBounds(b) => {
                write!(f, "{}..{}", b.lower, b.upper)
            }

            TagFieldData::Custom(d) => write!(f, "custom [{} bytes]", d.len()),
        }
    }
}

fn write_enum(f: &mut fmt::Formatter<'_>, value: i64, name: Option<&str>) -> fmt::Result {
    match name {
        Some(n) => write!(f, "{} ({})", value, n),
        None => write!(f, "{}", value),
    }
}

fn write_flags(
    f: &mut fmt::Formatter<'_>,
    value: u64,
    names: &[(u32, String)],
    hex_width: usize,
) -> fmt::Result {
    if names.is_empty() {
        write!(f, "0x{:0width$X} (none set)", value, width = hex_width)
    } else {
        let joined = names.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(", ");
        write!(f, "0x{:0width$X} [{}]", value, joined, width = hex_width)
    }
}

fn write_block_index(f: &mut fmt::Formatter<'_>, value: i64) -> fmt::Result {
    if value == -1 { f.write_str("NONE") } else { write!(f, "{}", value) }
}

//================================================================================
// Raw-data read/write helpers (LE).
//================================================================================

#[inline] fn read_i8(raw: &[u8], o: usize) -> i8 { raw[o] as i8 }
#[inline] fn read_u8(raw: &[u8], o: usize) -> u8 { raw[o] }
#[inline] fn read_i16(raw: &[u8], o: usize) -> i16 {
    i16::from_le_bytes([raw[o], raw[o + 1]])
}
#[inline] fn read_u16(raw: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([raw[o], raw[o + 1]])
}
#[inline] fn read_i32(raw: &[u8], o: usize) -> i32 {
    i32::from_le_bytes(raw[o..o + 4].try_into().unwrap())
}
#[inline] fn read_u32(raw: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(raw[o..o + 4].try_into().unwrap())
}
#[inline] fn read_i64(raw: &[u8], o: usize) -> i64 {
    i64::from_le_bytes(raw[o..o + 8].try_into().unwrap())
}
#[inline] fn read_f32(raw: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(raw[o..o + 4].try_into().unwrap())
}

#[inline] fn write_i8(raw: &mut [u8], o: usize, v: i8) { raw[o] = v as u8 }
#[inline] fn write_u8(raw: &mut [u8], o: usize, v: u8) { raw[o] = v }
#[inline] fn write_i16(raw: &mut [u8], o: usize, v: i16) {
    raw[o..o + 2].copy_from_slice(&v.to_le_bytes());
}
#[inline] fn write_u16(raw: &mut [u8], o: usize, v: u16) {
    raw[o..o + 2].copy_from_slice(&v.to_le_bytes());
}
#[inline] fn write_i32(raw: &mut [u8], o: usize, v: i32) {
    raw[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
#[inline] fn write_u32(raw: &mut [u8], o: usize, v: u32) {
    raw[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
#[inline] fn write_i64(raw: &mut [u8], o: usize, v: i64) {
    raw[o..o + 8].copy_from_slice(&v.to_le_bytes());
}
#[inline] fn write_f32(raw: &mut [u8], o: usize, v: f32) {
    raw[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

/// Read a fixed-size null-padded UTF-8 string out of `bytes`. Stops at
/// the first NUL; invalid UTF-8 sequences are replaced.
fn decode_null_padded_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Write `s` into a fixed-size buffer, truncating to fit and zero-padding
/// the tail.
fn encode_null_padded_string(s: &str, dest: &mut [u8]) {
    let bytes = s.as_bytes();
    let n = bytes.len().min(dest.len());
    dest[..n].copy_from_slice(&bytes[..n]);
    for slot in &mut dest[n..] {
        *slot = 0;
    }
}

//================================================================================
// Enum / flags name resolution.
//================================================================================

/// Resolve an enum value to its variant name via the layout's
/// `string_lists[field.definition]`. Returns `None` if `value` is out
/// of range or the string list / offset is missing.
fn resolve_enum_name(layout: &TagLayout, field: &TagFieldLayout, value: i64) -> Option<String> {
    let string_list = layout.string_lists.get(field.definition as usize)?;
    if value < 0 || (value as u32) >= string_list.count {
        return None;
    }
    let offset_index = (string_list.first + value as u32) as usize;
    let string_offset = *layout.string_offsets.get(offset_index)?;
    layout.get_string(string_offset).map(str::to_string)
}

/// Reverse of the name-resolution step: look up the bit index of a
/// flags value whose variant name matches `name` (case-sensitive).
/// Returns `None` if the field has no string list or no bit with that
/// name exists. Used by the `flag` CLI command to map a user-supplied
/// flag name to the bit it should set/clear.
pub(crate) fn find_flag_bit(layout: &TagLayout, field: &TagFieldLayout, name: &str) -> Option<u32> {
    let string_list = layout.string_lists.get(field.definition as usize)?;
    for bit in 0..string_list.count {
        let offset_index = (string_list.first + bit) as usize;
        let Some(&string_offset) = layout.string_offsets.get(offset_index) else { continue };
        if layout.get_string(string_offset) == Some(name) {
            return Some(bit);
        }
    }
    None
}

/// Look up an enum variant's index by name against the field's
/// string list. Case-insensitive. Returns `None` if the field has
/// no string list or no matching option. Companion to
/// [`find_flag_bit`].
pub(crate) fn find_enum_option_index(
    layout: &TagLayout,
    field: &TagFieldLayout,
    name: &str,
) -> Option<u32> {
    let string_list = layout.string_lists.get(field.definition as usize)?;
    for i in 0..string_list.count {
        let offset_index = (string_list.first + i) as usize;
        let Some(&string_offset) = layout.string_offsets.get(offset_index) else { continue };
        let Some(candidate) = layout.get_string(string_offset) else { continue };
        if candidate.eq_ignore_ascii_case(name) {
            return Some(i);
        }
    }
    None
}

/// Render a 4-byte group tag (stored as a BE-packed `u32` with
/// trailing-space padding for short tags) as its ASCII form, with
/// trailing spaces and NULs stripped. Matches the convention used
/// throughout the tag-file wire format. Inverse of
/// [`parse_group_tag`].
pub fn format_group_tag(tag: u32) -> String {
    let bytes = tag.to_be_bytes();
    String::from_utf8_lossy(&bytes)
        .trim_end_matches(|c: char| c == '\0' || c == ' ')
        .to_string()
}

/// Parse an ASCII group-tag string (1-4 chars, e.g. `"bipd"` or
/// `"mo"`) into the BE-packed `u32` form used on disk. Short tags
/// are right-padded with spaces to 4 bytes. Returns `None` if the
/// input is longer than 4 bytes. Inverse of [`format_group_tag`].
pub(crate) fn parse_group_tag(s: &str) -> Option<u32> {
    let bytes = s.as_bytes();
    if bytes.len() > 4 {
        return None;
    }
    let mut padded = [b' '; 4];
    padded[..bytes.len()].copy_from_slice(bytes);
    Some(u32::from_be_bytes(padded))
}

/// Iterate the option names of an enum or flags field (indexed by
/// `field.definition` into `layout.string_lists`). Yields `""` for
/// empty / missing entries. Returns an empty iterator for fields
/// whose `definition` doesn't reference a valid string list
/// (e.g. block-flags, which index into `block_layouts`).
pub(crate) fn field_option_names<'a>(
    layout: &'a TagLayout,
    field: &TagFieldLayout,
) -> impl Iterator<Item = &'a str> + 'a {
    let string_list = layout.string_lists.get(field.definition as usize);
    let range = match string_list {
        Some(sl) => sl.first..sl.first + sl.count,
        None => 0..0,
    };
    range.map(move |i| {
        layout
            .string_offsets
            .get(i as usize)
            .and_then(|&off| layout.get_string(off))
            .unwrap_or("")
    })
}

/// Resolve the names of the set bits of a flags value. Unset bits and
/// bits past the string list's `count` are omitted — the raw value in
/// `TagFieldData` preserves them regardless.
fn resolve_flag_names(
    layout: &TagLayout,
    field: &TagFieldLayout,
    value: u64,
    total_bits: u32,
) -> Vec<(u32, String)> {
    let mut names = Vec::new();
    let Some(string_list) = layout.string_lists.get(field.definition as usize) else {
        return names;
    };
    for bit in 0..total_bits {
        if (value & (1u64 << bit)) == 0 {
            continue;
        }
        if bit >= string_list.count {
            continue;
        }
        let offset_index = (string_list.first + bit) as usize;
        let Some(&string_offset) = layout.string_offsets.get(offset_index) else { continue };
        let Some(name) = layout.get_string(string_offset) else { continue };
        names.push((bit, name.to_string()));
    }
    names
}

//================================================================================
// deserialize_field / serialize_field
//================================================================================

/// Parse the value of a single field.
///
/// `raw_struct` is the slice of the enclosing block's `raw_data`
/// covering exactly this struct's bytes; primitive values slice
/// directly out of it at `field.offset`. Sub-chunk leaf fields consume
/// `sub_chunk` — the caller is responsible for locating the matching
/// [`TagSubChunkContent`] entry in the containing struct's
/// `sub_chunks`.
///
/// Returns `None` for field types that don't carry a value
/// (pad/skip/explanation/terminator, container types handled via
/// sub-chunks navigation, and the not-yet-modeled api_interop /
/// vertex_buffer). Container fields live in the sub-chunks tree
/// already — the caller walks them directly.
pub(crate) fn deserialize_field(
    layout: &TagLayout,
    field: &TagFieldLayout,
    raw_struct: &[u8],
    sub_chunk: Option<&TagSubChunkContent>,
) -> Option<TagFieldData> {
    let offset = field.offset as usize;

    match field.field_type {
        // No value.
        TagFieldType::Unknown
        | TagFieldType::Pad
        | TagFieldType::UselessPad
        | TagFieldType::Skip
        | TagFieldType::Explanation
        | TagFieldType::Terminator => None,

        // Containers — navigated via sub_chunks directly.
        TagFieldType::Struct
        | TagFieldType::Block
        | TagFieldType::Array
        | TagFieldType::PageableResource => None,

        // Not yet modeled.
        TagFieldType::ApiInterop | TagFieldType::VertexBuffer => None,

        // Strings (null-padded in raw_data).
        TagFieldType::String => Some(TagFieldData::String(
            decode_null_padded_string(&raw_struct[offset..offset + 32]),
        )),
        TagFieldType::LongString => Some(TagFieldData::LongString(
            decode_null_padded_string(&raw_struct[offset..offset + 256]),
        )),

        // Integers.
        TagFieldType::CharInteger => Some(TagFieldData::CharInteger(read_i8(raw_struct, offset))),
        TagFieldType::ShortInteger => Some(TagFieldData::ShortInteger(read_i16(raw_struct, offset))),
        TagFieldType::LongInteger => Some(TagFieldData::LongInteger(read_i32(raw_struct, offset))),
        TagFieldType::Int64Integer => Some(TagFieldData::Int64Integer(read_i64(raw_struct, offset))),
        TagFieldType::Tag => Some(TagFieldData::Tag(read_u32(raw_struct, offset))),

        // Enums.
        TagFieldType::CharEnum => {
            let value = read_i8(raw_struct, offset);
            Some(TagFieldData::CharEnum { value, name: resolve_enum_name(layout, field, value as i64) })
        }
        TagFieldType::ShortEnum => {
            let value = read_i16(raw_struct, offset);
            Some(TagFieldData::ShortEnum { value, name: resolve_enum_name(layout, field, value as i64) })
        }
        TagFieldType::LongEnum => {
            let value = read_i32(raw_struct, offset);
            Some(TagFieldData::LongEnum { value, name: resolve_enum_name(layout, field, value as i64) })
        }

        // Flags.
        TagFieldType::ByteFlags => {
            let value = read_u8(raw_struct, offset);
            Some(TagFieldData::ByteFlags { value, names: resolve_flag_names(layout, field, value as u64, 8) })
        }
        TagFieldType::WordFlags => {
            let value = read_u16(raw_struct, offset);
            Some(TagFieldData::WordFlags { value, names: resolve_flag_names(layout, field, value as u64, 16) })
        }
        TagFieldType::LongFlags => {
            let value = read_i32(raw_struct, offset);
            Some(TagFieldData::LongFlags { value, names: resolve_flag_names(layout, field, value as u32 as u64, 32) })
        }

        // Block flags — value only for now.
        TagFieldType::ByteBlockFlags => Some(TagFieldData::ByteBlockFlags(read_u8(raw_struct, offset))),
        TagFieldType::WordBlockFlags => Some(TagFieldData::WordBlockFlags(read_u16(raw_struct, offset))),
        TagFieldType::LongBlockFlags => Some(TagFieldData::LongBlockFlags(read_i32(raw_struct, offset))),

        // Block indices.
        TagFieldType::CharBlockIndex => Some(TagFieldData::CharBlockIndex(read_i8(raw_struct, offset))),
        TagFieldType::CustomCharBlockIndex => Some(TagFieldData::CustomCharBlockIndex(read_i8(raw_struct, offset))),
        TagFieldType::ShortBlockIndex => Some(TagFieldData::ShortBlockIndex(read_i16(raw_struct, offset))),
        TagFieldType::CustomShortBlockIndex => Some(TagFieldData::CustomShortBlockIndex(read_i16(raw_struct, offset))),
        TagFieldType::LongBlockIndex => Some(TagFieldData::LongBlockIndex(read_i32(raw_struct, offset))),
        TagFieldType::CustomLongBlockIndex => Some(TagFieldData::CustomLongBlockIndex(read_i32(raw_struct, offset))),

        // Floats.
        TagFieldType::Angle => Some(TagFieldData::Angle(read_f32(raw_struct, offset))),
        TagFieldType::Real => Some(TagFieldData::Real(read_f32(raw_struct, offset))),
        TagFieldType::RealSlider => Some(TagFieldData::RealSlider(read_f32(raw_struct, offset))),
        TagFieldType::RealFraction => Some(TagFieldData::RealFraction(read_f32(raw_struct, offset))),

        // Math composites.
        TagFieldType::Point2d => Some(TagFieldData::Point2d(math::Point2d {
            x: read_i16(raw_struct, offset),
            y: read_i16(raw_struct, offset + 2),
        })),
        TagFieldType::Rectangle2d => Some(TagFieldData::Rectangle2d(math::Rectangle2d {
            top: read_i16(raw_struct, offset),
            left: read_i16(raw_struct, offset + 2),
            bottom: read_i16(raw_struct, offset + 4),
            right: read_i16(raw_struct, offset + 6),
        })),
        TagFieldType::RealPoint2d => Some(TagFieldData::RealPoint2d(math::RealPoint2d {
            x: read_f32(raw_struct, offset),
            y: read_f32(raw_struct, offset + 4),
        })),
        TagFieldType::RealPoint3d => Some(TagFieldData::RealPoint3d(math::RealPoint3d {
            x: read_f32(raw_struct, offset),
            y: read_f32(raw_struct, offset + 4),
            z: read_f32(raw_struct, offset + 8),
        })),
        TagFieldType::RealVector2d => Some(TagFieldData::RealVector2d(math::RealVector2d {
            i: read_f32(raw_struct, offset),
            j: read_f32(raw_struct, offset + 4),
        })),
        TagFieldType::RealVector3d => Some(TagFieldData::RealVector3d(math::RealVector3d {
            i: read_f32(raw_struct, offset),
            j: read_f32(raw_struct, offset + 4),
            k: read_f32(raw_struct, offset + 8),
        })),
        TagFieldType::RealQuaternion => Some(TagFieldData::RealQuaternion(math::RealQuaternion {
            i: read_f32(raw_struct, offset),
            j: read_f32(raw_struct, offset + 4),
            k: read_f32(raw_struct, offset + 8),
            w: read_f32(raw_struct, offset + 12),
        })),
        TagFieldType::RealEulerAngles2d => Some(TagFieldData::RealEulerAngles2d(math::RealEulerAngles2d {
            yaw: read_f32(raw_struct, offset),
            pitch: read_f32(raw_struct, offset + 4),
        })),
        TagFieldType::RealEulerAngles3d => Some(TagFieldData::RealEulerAngles3d(math::RealEulerAngles3d {
            yaw: read_f32(raw_struct, offset),
            pitch: read_f32(raw_struct, offset + 4),
            roll: read_f32(raw_struct, offset + 8),
        })),
        TagFieldType::RealPlane2d => Some(TagFieldData::RealPlane2d(math::RealPlane2d {
            i: read_f32(raw_struct, offset),
            j: read_f32(raw_struct, offset + 4),
            d: read_f32(raw_struct, offset + 8),
        })),
        TagFieldType::RealPlane3d => Some(TagFieldData::RealPlane3d(math::RealPlane3d {
            i: read_f32(raw_struct, offset),
            j: read_f32(raw_struct, offset + 4),
            k: read_f32(raw_struct, offset + 8),
            d: read_f32(raw_struct, offset + 12),
        })),

        // Colors.
        TagFieldType::RgbColor => Some(TagFieldData::RgbColor(math::RgbColor(read_u32(raw_struct, offset)))),
        TagFieldType::ArgbColor => Some(TagFieldData::ArgbColor(math::ArgbColor(read_u32(raw_struct, offset)))),
        TagFieldType::RealRgbColor => Some(TagFieldData::RealRgbColor(math::RealRgbColor {
            red: read_f32(raw_struct, offset),
            green: read_f32(raw_struct, offset + 4),
            blue: read_f32(raw_struct, offset + 8),
        })),
        TagFieldType::RealArgbColor => Some(TagFieldData::RealArgbColor(math::RealArgbColor {
            alpha: read_f32(raw_struct, offset),
            red: read_f32(raw_struct, offset + 4),
            green: read_f32(raw_struct, offset + 8),
            blue: read_f32(raw_struct, offset + 12),
        })),
        TagFieldType::RealHsvColor => Some(TagFieldData::RealHsvColor(math::RealHsvColor {
            hue: read_f32(raw_struct, offset),
            saturation: read_f32(raw_struct, offset + 4),
            value: read_f32(raw_struct, offset + 8),
        })),
        TagFieldType::RealAhsvColor => Some(TagFieldData::RealAhsvColor(math::RealAhsvColor {
            alpha: read_f32(raw_struct, offset),
            hue: read_f32(raw_struct, offset + 4),
            saturation: read_f32(raw_struct, offset + 8),
            value: read_f32(raw_struct, offset + 12),
        })),

        // Bounds.
        TagFieldType::ShortIntegerBounds => Some(TagFieldData::ShortIntegerBounds(math::ShortBounds {
            lower: read_i16(raw_struct, offset),
            upper: read_i16(raw_struct, offset + 2),
        })),
        TagFieldType::AngleBounds => Some(TagFieldData::AngleBounds(math::AngleBounds {
            lower: read_f32(raw_struct, offset),
            upper: read_f32(raw_struct, offset + 4),
        })),
        TagFieldType::RealBounds => Some(TagFieldData::RealBounds(math::RealBounds {
            lower: read_f32(raw_struct, offset),
            upper: read_f32(raw_struct, offset + 4),
        })),
        TagFieldType::FractionBounds => Some(TagFieldData::FractionBounds(math::FractionBounds {
            lower: read_f32(raw_struct, offset),
            upper: read_f32(raw_struct, offset + 4),
        })),

        // Custom: variable-size opaque bytes. Size comes from the
        // field_type's declared `size`.
        TagFieldType::Custom => {
            let size = layout.field_types[field.type_index as usize].size as usize;
            Some(TagFieldData::Custom(raw_struct[offset..offset + size].to_vec()))
        }

        // Sub-chunk leaves.
        TagFieldType::StringId => match sub_chunk {
            Some(TagSubChunkContent::StringId(payload)) => {
                Some(TagFieldData::StringId(StringIdData::from_bytes(payload)))
            }
            _ => panic!("deserialize_field: StringId field missing sub_chunk payload"),
        },
        TagFieldType::OldStringId => match sub_chunk {
            Some(TagSubChunkContent::OldStringId(payload)) => {
                Some(TagFieldData::OldStringId(StringIdData::from_bytes(payload)))
            }
            _ => panic!("deserialize_field: OldStringId field missing sub_chunk payload"),
        },
        TagFieldType::TagReference => match sub_chunk {
            Some(TagSubChunkContent::TagReference(payload)) => {
                Some(TagFieldData::TagReference(TagReferenceData::from_bytes(payload)))
            }
            _ => panic!("deserialize_field: TagReference field missing sub_chunk payload"),
        },
        TagFieldType::Data => match sub_chunk {
            Some(TagSubChunkContent::Data(payload)) => Some(TagFieldData::Data(payload.clone())),
            _ => panic!("deserialize_field: Data field missing sub_chunk payload"),
        },
    }
}

/// Serialize a [`TagFieldData`] back to its on-disk form.
///
/// Primitive/enum/flag/string/math variants write their bytes into
/// `raw_struct` at `field.offset` and return `None`. Sub-chunk leaf
/// variants (string_id, old_string_id, tag_reference, data) produce a
/// new [`TagSubChunkContent`] that the caller swaps into the owning
/// struct's `sub_chunks`. Names in enum/flag variants are ignored —
/// only the raw value is written.
///
/// Panics if `value`'s variant doesn't match `field.field_type` — the
/// caller is responsible for only passing compatible pairs.
pub(crate) fn serialize_field(
    field: &TagFieldLayout,
    value: &TagFieldData,
    raw_struct: &mut [u8],
) -> Option<TagSubChunkContent> {
    let offset = field.offset as usize;

    match value {
        TagFieldData::String(s) => {
            encode_null_padded_string(s, &mut raw_struct[offset..offset + 32]);
            None
        }
        TagFieldData::LongString(s) => {
            encode_null_padded_string(s, &mut raw_struct[offset..offset + 256]);
            None
        }

        TagFieldData::CharInteger(v) => { write_i8(raw_struct, offset, *v); None }
        TagFieldData::ShortInteger(v) => { write_i16(raw_struct, offset, *v); None }
        TagFieldData::LongInteger(v) => { write_i32(raw_struct, offset, *v); None }
        TagFieldData::Int64Integer(v) => { write_i64(raw_struct, offset, *v); None }
        TagFieldData::Tag(v) => { write_u32(raw_struct, offset, *v); None }

        TagFieldData::CharEnum { value, .. } => { write_i8(raw_struct, offset, *value); None }
        TagFieldData::ShortEnum { value, .. } => { write_i16(raw_struct, offset, *value); None }
        TagFieldData::LongEnum { value, .. } => { write_i32(raw_struct, offset, *value); None }

        TagFieldData::ByteFlags { value, .. } => { write_u8(raw_struct, offset, *value); None }
        TagFieldData::WordFlags { value, .. } => { write_u16(raw_struct, offset, *value); None }
        TagFieldData::LongFlags { value, .. } => { write_i32(raw_struct, offset, *value); None }

        TagFieldData::ByteBlockFlags(v) => { write_u8(raw_struct, offset, *v); None }
        TagFieldData::WordBlockFlags(v) => { write_u16(raw_struct, offset, *v); None }
        TagFieldData::LongBlockFlags(v) => { write_i32(raw_struct, offset, *v); None }

        TagFieldData::CharBlockIndex(v) => { write_i8(raw_struct, offset, *v); None }
        TagFieldData::CustomCharBlockIndex(v) => { write_i8(raw_struct, offset, *v); None }
        TagFieldData::ShortBlockIndex(v) => { write_i16(raw_struct, offset, *v); None }
        TagFieldData::CustomShortBlockIndex(v) => { write_i16(raw_struct, offset, *v); None }
        TagFieldData::LongBlockIndex(v) => { write_i32(raw_struct, offset, *v); None }
        TagFieldData::CustomLongBlockIndex(v) => { write_i32(raw_struct, offset, *v); None }

        TagFieldData::Angle(v) => { write_f32(raw_struct, offset, *v); None }
        TagFieldData::Real(v) => { write_f32(raw_struct, offset, *v); None }
        TagFieldData::RealSlider(v) => { write_f32(raw_struct, offset, *v); None }
        TagFieldData::RealFraction(v) => { write_f32(raw_struct, offset, *v); None }

        TagFieldData::Point2d(p) => {
            write_i16(raw_struct, offset, p.x);
            write_i16(raw_struct, offset + 2, p.y);
            None
        }
        TagFieldData::Rectangle2d(r) => {
            write_i16(raw_struct, offset, r.top);
            write_i16(raw_struct, offset + 2, r.left);
            write_i16(raw_struct, offset + 4, r.bottom);
            write_i16(raw_struct, offset + 6, r.right);
            None
        }
        TagFieldData::RealPoint2d(p) => {
            write_f32(raw_struct, offset, p.x);
            write_f32(raw_struct, offset + 4, p.y);
            None
        }
        TagFieldData::RealPoint3d(p) => {
            write_f32(raw_struct, offset, p.x);
            write_f32(raw_struct, offset + 4, p.y);
            write_f32(raw_struct, offset + 8, p.z);
            None
        }
        TagFieldData::RealVector2d(v) => {
            write_f32(raw_struct, offset, v.i);
            write_f32(raw_struct, offset + 4, v.j);
            None
        }
        TagFieldData::RealVector3d(v) => {
            write_f32(raw_struct, offset, v.i);
            write_f32(raw_struct, offset + 4, v.j);
            write_f32(raw_struct, offset + 8, v.k);
            None
        }
        TagFieldData::RealQuaternion(q) => {
            write_f32(raw_struct, offset, q.i);
            write_f32(raw_struct, offset + 4, q.j);
            write_f32(raw_struct, offset + 8, q.k);
            write_f32(raw_struct, offset + 12, q.w);
            None
        }
        TagFieldData::RealEulerAngles2d(a) => {
            write_f32(raw_struct, offset, a.yaw);
            write_f32(raw_struct, offset + 4, a.pitch);
            None
        }
        TagFieldData::RealEulerAngles3d(a) => {
            write_f32(raw_struct, offset, a.yaw);
            write_f32(raw_struct, offset + 4, a.pitch);
            write_f32(raw_struct, offset + 8, a.roll);
            None
        }
        TagFieldData::RealPlane2d(p) => {
            write_f32(raw_struct, offset, p.i);
            write_f32(raw_struct, offset + 4, p.j);
            write_f32(raw_struct, offset + 8, p.d);
            None
        }
        TagFieldData::RealPlane3d(p) => {
            write_f32(raw_struct, offset, p.i);
            write_f32(raw_struct, offset + 4, p.j);
            write_f32(raw_struct, offset + 8, p.k);
            write_f32(raw_struct, offset + 12, p.d);
            None
        }

        TagFieldData::RgbColor(c) => { write_u32(raw_struct, offset, c.0); None }
        TagFieldData::ArgbColor(c) => { write_u32(raw_struct, offset, c.0); None }
        TagFieldData::RealRgbColor(c) => {
            write_f32(raw_struct, offset, c.red);
            write_f32(raw_struct, offset + 4, c.green);
            write_f32(raw_struct, offset + 8, c.blue);
            None
        }
        TagFieldData::RealArgbColor(c) => {
            write_f32(raw_struct, offset, c.alpha);
            write_f32(raw_struct, offset + 4, c.red);
            write_f32(raw_struct, offset + 8, c.green);
            write_f32(raw_struct, offset + 12, c.blue);
            None
        }
        TagFieldData::RealHsvColor(c) => {
            write_f32(raw_struct, offset, c.hue);
            write_f32(raw_struct, offset + 4, c.saturation);
            write_f32(raw_struct, offset + 8, c.value);
            None
        }
        TagFieldData::RealAhsvColor(c) => {
            write_f32(raw_struct, offset, c.alpha);
            write_f32(raw_struct, offset + 4, c.hue);
            write_f32(raw_struct, offset + 8, c.saturation);
            write_f32(raw_struct, offset + 12, c.value);
            None
        }

        TagFieldData::ShortIntegerBounds(b) => {
            write_i16(raw_struct, offset, b.lower);
            write_i16(raw_struct, offset + 2, b.upper);
            None
        }
        TagFieldData::AngleBounds(b) => {
            write_f32(raw_struct, offset, b.lower);
            write_f32(raw_struct, offset + 4, b.upper);
            None
        }
        TagFieldData::RealBounds(b) => {
            write_f32(raw_struct, offset, b.lower);
            write_f32(raw_struct, offset + 4, b.upper);
            None
        }
        TagFieldData::FractionBounds(b) => {
            write_f32(raw_struct, offset, b.lower);
            write_f32(raw_struct, offset + 4, b.upper);
            None
        }

        TagFieldData::Custom(bytes) => {
            let size = bytes.len();
            raw_struct[offset..offset + size].copy_from_slice(bytes);
            None
        }

        // Sub-chunk leaves — produce a new TagSubChunkContent.
        TagFieldData::StringId(s) => Some(TagSubChunkContent::StringId(s.to_bytes())),
        TagFieldData::OldStringId(s) => Some(TagSubChunkContent::OldStringId(s.to_bytes())),
        TagFieldData::TagReference(r) => Some(TagSubChunkContent::TagReference(r.to_bytes())),
        TagFieldData::Data(bytes) => Some(TagSubChunkContent::Data(bytes.clone())),
    }
}
