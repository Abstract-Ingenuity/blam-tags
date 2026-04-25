//! Format `TagFieldData` values for CLI output.
//!
//! All textual rendering of tag field values lives here — the library
//! holds typed data and offers no `Display` impls of its own. Two
//! entry points:
//!
//! - [`format_value`] — one-line human-readable string. `hex_mode`
//!   flips the four plain integer variants to fixed-width hex.
//! - [`value_to_json`] — structured JSON form. Preserves enum/flag
//!   names alongside the raw integers; containers aren't handled here
//!   (inspect/get's JSON walker handles descent).

use blam_tags::{format_group_tag, TagFieldData, TagReferenceData, StringIdData};
use serde_json::{json, Value};

use crate::context::CliContext;

/// One-line text rendering of `value`. `hex_mode` formats the four
/// plain-integer variants (`CharInteger`, `ShortInteger`, `LongInteger`,
/// `Int64Integer`) as `0x…` with fixed width; everything else renders
/// the same regardless. `ctx` provides the [`crate::tag_index::TagIndex`]
/// used to resolve tag-reference group tags to friendly names.
pub fn format_value(ctx: &CliContext, value: &TagFieldData, hex_mode: bool) -> String {
    let mut s = String::new();
    write_value(ctx, &mut s, value, hex_mode);
    s
}

fn write_value(ctx: &CliContext, out: &mut String, value: &TagFieldData, hex: bool) {
    use std::fmt::Write;
    match value {
        TagFieldData::String(s) | TagFieldData::LongString(s) => {
            write!(out, "\"{}\"", s).unwrap();
        }

        TagFieldData::StringId(s) | TagFieldData::OldStringId(s) => {
            write_string_id(out, s);
        }
        TagFieldData::TagReference(r) => write_tag_reference(ctx, out, r),
        TagFieldData::Data(d) => write!(out, "data [{} bytes]", d.len()).unwrap(),
        TagFieldData::ApiInterop(i) => match (i.descriptor(), i.address(), i.definition_address()) {
            (Some(d), Some(a), Some(da)) => write!(
                out,
                "api_interop {{ descriptor=0x{:08X}, address=0x{:08X}, definition_address=0x{:08X} }}",
                d, a, da,
            ).unwrap(),
            _ => write!(out, "api_interop [{} bytes]", i.raw.len()).unwrap(),
        },

        TagFieldData::CharInteger(v) => {
            if hex { write!(out, "0x{:02X}", *v as u8).unwrap() } else { write!(out, "{}", v).unwrap() }
        }
        TagFieldData::ShortInteger(v) => {
            if hex { write!(out, "0x{:04X}", *v as u16).unwrap() } else { write!(out, "{}", v).unwrap() }
        }
        TagFieldData::LongInteger(v) => {
            if hex { write!(out, "0x{:08X}", *v as u32).unwrap() } else { write!(out, "{}", v).unwrap() }
        }
        TagFieldData::Int64Integer(v) => {
            if hex { write!(out, "0x{:016X}", *v as u64).unwrap() } else { write!(out, "{}", v).unwrap() }
        }
        TagFieldData::Tag(v) => out.push_str(&format_group_tag(*v)),

        TagFieldData::CharEnum { value, name } => write_enum(out, *value as i64, name.as_deref()),
        TagFieldData::ShortEnum { value, name } => write_enum(out, *value as i64, name.as_deref()),
        TagFieldData::LongEnum { value, name } => write_enum(out, *value as i64, name.as_deref()),

        TagFieldData::ByteFlags { value, names } => write_flags(out, *value as u64, names, 2),
        TagFieldData::WordFlags { value, names } => write_flags(out, *value as u64, names, 4),
        TagFieldData::LongFlags { value, names } => write_flags(out, *value as u32 as u64, names, 8),

        TagFieldData::ByteBlockFlags(v) => write!(out, "0x{:02X}", v).unwrap(),
        TagFieldData::WordBlockFlags(v) => write!(out, "0x{:04X}", v).unwrap(),
        TagFieldData::LongBlockFlags(v) => write!(out, "0x{:08X}", *v as u32).unwrap(),

        TagFieldData::CharBlockIndex(v) | TagFieldData::CustomCharBlockIndex(v) => write_block_index(out, *v as i64),
        TagFieldData::ShortBlockIndex(v) | TagFieldData::CustomShortBlockIndex(v) => write_block_index(out, *v as i64),
        TagFieldData::LongBlockIndex(v) | TagFieldData::CustomLongBlockIndex(v) => write_block_index(out, *v as i64),

        TagFieldData::Angle(v) => write!(out, "{:.4} rad ({:.2} deg)", v, v.to_degrees()).unwrap(),
        TagFieldData::Real(v) | TagFieldData::RealSlider(v) | TagFieldData::RealFraction(v) => {
            write!(out, "{}", v).unwrap()
        }

        TagFieldData::Point2d(p) => write!(out, "{}, {}", p.x, p.y).unwrap(),
        TagFieldData::Rectangle2d(r) => write!(out, "{}, {}, {}, {}", r.top, r.left, r.bottom, r.right).unwrap(),
        TagFieldData::RealPoint2d(p) => write!(out, "x={}, y={}", p.x, p.y).unwrap(),
        TagFieldData::RealPoint3d(p) => write!(out, "x={}, y={}, z={}", p.x, p.y, p.z).unwrap(),
        TagFieldData::RealVector2d(v) => write!(out, "i={}, j={}", v.i, v.j).unwrap(),
        TagFieldData::RealVector3d(v) => write!(out, "i={}, j={}, k={}", v.i, v.j, v.k).unwrap(),
        TagFieldData::RealQuaternion(q) => write!(out, "i={}, j={}, k={}, w={}", q.i, q.j, q.k, q.w).unwrap(),
        TagFieldData::RealEulerAngles2d(e) => write!(out, "yaw={}, pitch={}", e.yaw, e.pitch).unwrap(),
        TagFieldData::RealEulerAngles3d(e) => write!(out, "yaw={}, pitch={}, roll={}", e.yaw, e.pitch, e.roll).unwrap(),
        TagFieldData::RealPlane2d(p) => write!(out, "i={}, j={}, d={}", p.i, p.j, p.d).unwrap(),
        TagFieldData::RealPlane3d(p) => write!(out, "i={}, j={}, k={}, d={}", p.i, p.j, p.k, p.d).unwrap(),

        TagFieldData::RgbColor(c) => write!(out, "0x{:08X}", c.0).unwrap(),
        TagFieldData::ArgbColor(c) => write!(out, "0x{:08X}", c.0).unwrap(),
        TagFieldData::RealRgbColor(c) => write!(out, "r={}, g={}, b={}", c.red, c.green, c.blue).unwrap(),
        TagFieldData::RealArgbColor(c) => write!(out, "a={}, r={}, g={}, b={}", c.alpha, c.red, c.green, c.blue).unwrap(),
        TagFieldData::RealHsvColor(c) => write!(out, "h={}, s={}, v={}", c.hue, c.saturation, c.value).unwrap(),
        TagFieldData::RealAhsvColor(c) => write!(out, "a={}, h={}, s={}, v={}", c.alpha, c.hue, c.saturation, c.value).unwrap(),

        TagFieldData::ShortIntegerBounds(b) => write!(out, "{}..{}", b.lower, b.upper).unwrap(),
        TagFieldData::AngleBounds(b) | TagFieldData::RealBounds(b) | TagFieldData::FractionBounds(b) => {
            write!(out, "{}..{}", b.lower, b.upper).unwrap()
        }

        TagFieldData::Custom(d) => write!(out, "custom [{} bytes]", d.len()).unwrap(),
    }
}

fn write_string_id(out: &mut String, s: &StringIdData) {
    use std::fmt::Write;
    if s.string.is_empty() {
        out.push_str("NONE");
    } else {
        write!(out, "\"{}\"", s.string).unwrap();
    }
}

/// Append a tag reference to `out` as `<path>.<group_name>` (the
/// on-disk filename form), or `<group_tag>:<path>` when the group tag
/// isn't in the loaded [`crate::tag_index::TagIndex`]. `NONE` for null
/// references.
pub fn write_tag_reference(ctx: &CliContext, out: &mut String, r: &TagReferenceData) {
    use std::fmt::Write;
    let Some((group_tag, path)) = &r.group_tag_and_name else {
        out.push_str("NONE");
        return;
    };
    match ctx.tag_index.name_for(*group_tag) {
        Some(name) => write!(out, "{path}.{name}").unwrap(),
        None => write!(out, "{}:{}", format_group_tag(*group_tag), path).unwrap(),
    }
}

fn write_enum(out: &mut String, value: i64, name: Option<&str>) {
    use std::fmt::Write;
    match name {
        Some(n) => write!(out, "{} ({})", value, n).unwrap(),
        None => write!(out, "{}", value).unwrap(),
    }
}

fn write_flags(out: &mut String, value: u64, names: &[(u32, String)], hex_width: usize) {
    use std::fmt::Write;
    if names.is_empty() {
        write!(out, "0x{:0width$X} (none set)", value, width = hex_width).unwrap();
    } else {
        let joined = names.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(", ");
        write!(out, "0x{:0width$X} [{}]", value, joined, width = hex_width).unwrap();
    }
}

fn write_block_index(out: &mut String, value: i64) {
    if value == -1 {
        out.push_str("NONE");
    } else {
        use std::fmt::Write;
        write!(out, "{}", value).unwrap();
    }
}

/// Convert a `TagFieldData` to a JSON value. Preserves enum/flag
/// names alongside the raw integers; containers aren't handled here
/// (inspect/get's JSON walker handles descent). `ctx` provides the
/// [`crate::tag_index::TagIndex`] used to resolve tag-reference group
/// tags to friendly names.
pub fn value_to_json(ctx: &CliContext, value: &TagFieldData) -> Value {
    match value {
        TagFieldData::String(s) | TagFieldData::LongString(s) => json!(s),

        TagFieldData::StringId(s) | TagFieldData::OldStringId(s) => json!({ "string": s.string }),
        TagFieldData::TagReference(r) => match &r.group_tag_and_name {
            None => json!(null),
            Some(_) => {
                let mut s = String::new();
                write_tag_reference(ctx, &mut s, r);
                json!(s)
            }
        },
        TagFieldData::Data(d) => json!({ "size": d.len() }),
        TagFieldData::ApiInterop(i) => match (i.descriptor(), i.address(), i.definition_address()) {
            (Some(d), Some(a), Some(da)) => json!({
                "descriptor": d, "address": a, "definition_address": da,
            }),
            _ => json!({ "raw_size": i.raw.len() }),
        },

        TagFieldData::CharInteger(v) => json!(v),
        TagFieldData::ShortInteger(v) => json!(v),
        TagFieldData::LongInteger(v) => json!(v),
        TagFieldData::Int64Integer(v) => json!(v),
        TagFieldData::Tag(v) => json!(format_group_tag(*v)),

        TagFieldData::CharEnum { value, name } => json!({ "value": value, "name": name }),
        TagFieldData::ShortEnum { value, name } => json!({ "value": value, "name": name }),
        TagFieldData::LongEnum { value, name } => json!({ "value": value, "name": name }),

        TagFieldData::ByteFlags { value, names } => json!({
            "value": value,
            "set": names.iter().map(|(_, n)| n).collect::<Vec<_>>()
        }),
        TagFieldData::WordFlags { value, names } => json!({
            "value": value,
            "set": names.iter().map(|(_, n)| n).collect::<Vec<_>>()
        }),
        TagFieldData::LongFlags { value, names } => json!({
            "value": value,
            "set": names.iter().map(|(_, n)| n).collect::<Vec<_>>()
        }),

        TagFieldData::ByteBlockFlags(v) => json!(v),
        TagFieldData::WordBlockFlags(v) => json!(v),
        TagFieldData::LongBlockFlags(v) => json!(v),

        TagFieldData::CharBlockIndex(v) | TagFieldData::CustomCharBlockIndex(v) => json!(v),
        TagFieldData::ShortBlockIndex(v) | TagFieldData::CustomShortBlockIndex(v) => json!(v),
        TagFieldData::LongBlockIndex(v) | TagFieldData::CustomLongBlockIndex(v) => json!(v),

        TagFieldData::Angle(v) => json!(v),
        TagFieldData::Real(v) | TagFieldData::RealSlider(v) | TagFieldData::RealFraction(v) => json!(v),

        TagFieldData::Point2d(p) => json!({ "x": p.x, "y": p.y }),
        TagFieldData::Rectangle2d(r) => json!({
            "top": r.top, "left": r.left, "bottom": r.bottom, "right": r.right
        }),
        TagFieldData::RealPoint2d(p) => json!({ "x": p.x, "y": p.y }),
        TagFieldData::RealPoint3d(p) => json!({ "x": p.x, "y": p.y, "z": p.z }),
        TagFieldData::RealVector2d(v) => json!({ "i": v.i, "j": v.j }),
        TagFieldData::RealVector3d(v) => json!({ "i": v.i, "j": v.j, "k": v.k }),
        TagFieldData::RealQuaternion(q) => json!({ "i": q.i, "j": q.j, "k": q.k, "w": q.w }),
        TagFieldData::RealEulerAngles2d(e) => json!({ "yaw": e.yaw, "pitch": e.pitch }),
        TagFieldData::RealEulerAngles3d(e) => json!({ "yaw": e.yaw, "pitch": e.pitch, "roll": e.roll }),
        TagFieldData::RealPlane2d(p) => json!({ "i": p.i, "j": p.j, "d": p.d }),
        TagFieldData::RealPlane3d(p) => json!({ "i": p.i, "j": p.j, "k": p.k, "d": p.d }),

        TagFieldData::RgbColor(c) => json!(c.0),
        TagFieldData::ArgbColor(c) => json!(c.0),
        TagFieldData::RealRgbColor(c) => json!({ "r": c.red, "g": c.green, "b": c.blue }),
        TagFieldData::RealArgbColor(c) => json!({ "a": c.alpha, "r": c.red, "g": c.green, "b": c.blue }),
        TagFieldData::RealHsvColor(c) => json!({ "h": c.hue, "s": c.saturation, "v": c.value }),
        TagFieldData::RealAhsvColor(c) => json!({ "a": c.alpha, "h": c.hue, "s": c.saturation, "v": c.value }),

        TagFieldData::ShortIntegerBounds(b) => json!({ "lower": b.lower, "upper": b.upper }),
        TagFieldData::AngleBounds(b) | TagFieldData::RealBounds(b) | TagFieldData::FractionBounds(b) => {
            json!({ "lower": b.lower, "upper": b.upper })
        }

        TagFieldData::Custom(d) => json!({ "size": d.len() }),
    }
}
