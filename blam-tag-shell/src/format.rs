//! Format `TagFieldData` values for CLI output.

use blam_tags::fields::{format_group_tag, StringIdData, TagFieldData, TagReferenceData};
use serde_json::{json, Value};

/// Format a single `TagFieldData` value as a one-line human-readable
/// string. `hex_mode` renders numeric variants in hex where it makes
/// sense; enum/flag and color variants always use hex for their raw
/// integer regardless, to match the on-disk encoding.
pub fn format_value(value: &TagFieldData, hex_mode: bool) -> String {
    match value {
        TagFieldData::String(s) | TagFieldData::LongString(s) => format!("\"{}\"", s),

        TagFieldData::StringId(s) | TagFieldData::OldStringId(s) => format_string_id(s),
        TagFieldData::TagReference(r) => format_tag_reference(r),
        TagFieldData::Data(d) => format!("data [{} bytes]", d.len()),

        TagFieldData::CharInteger(v) => {
            if hex_mode { format!("0x{:02X}", *v as u8) } else { v.to_string() }
        }
        TagFieldData::ShortInteger(v) => {
            if hex_mode { format!("0x{:04X}", *v as u16) } else { v.to_string() }
        }
        TagFieldData::LongInteger(v) => {
            if hex_mode { format!("0x{:08X}", *v as u32) } else { v.to_string() }
        }
        TagFieldData::Int64Integer(v) => {
            if hex_mode { format!("0x{:016X}", *v as u64) } else { v.to_string() }
        }
        TagFieldData::Tag(v) => format_tag_group(*v),

        TagFieldData::CharEnum { value, name } => format_enum(*value as i64, name.as_deref()),
        TagFieldData::ShortEnum { value, name } => format_enum(*value as i64, name.as_deref()),
        TagFieldData::LongEnum { value, name } => format_enum(*value as i64, name.as_deref()),

        TagFieldData::ByteFlags { value, names } => format_flags(*value as u64, names, 2),
        TagFieldData::WordFlags { value, names } => format_flags(*value as u64, names, 4),
        TagFieldData::LongFlags { value, names } => format_flags(*value as u32 as u64, names, 8),

        TagFieldData::ByteBlockFlags(v) => format!("0x{:02X}", v),
        TagFieldData::WordBlockFlags(v) => format!("0x{:04X}", v),
        TagFieldData::LongBlockFlags(v) => format!("0x{:08X}", *v as u32),

        TagFieldData::CharBlockIndex(v) | TagFieldData::CustomCharBlockIndex(v) => format_block_index(*v as i64),
        TagFieldData::ShortBlockIndex(v) | TagFieldData::CustomShortBlockIndex(v) => format_block_index(*v as i64),
        TagFieldData::LongBlockIndex(v) | TagFieldData::CustomLongBlockIndex(v) => format_block_index(*v as i64),

        TagFieldData::Angle(v) => format!("{:.4} rad ({:.2} deg)", v, v.to_degrees()),
        TagFieldData::Real(v) | TagFieldData::RealSlider(v) | TagFieldData::RealFraction(v) => v.to_string(),

        TagFieldData::Point2d(p) => format!("{}, {}", p.x, p.y),
        TagFieldData::Rectangle2d(r) => format!("{}, {}, {}, {}", r.top, r.left, r.bottom, r.right),
        TagFieldData::RealPoint2d(p) => format!("x={}, y={}", p.x, p.y),
        TagFieldData::RealPoint3d(p) => format!("x={}, y={}, z={}", p.x, p.y, p.z),
        TagFieldData::RealVector2d(v) => format!("i={}, j={}", v.i, v.j),
        TagFieldData::RealVector3d(v) => format!("i={}, j={}, k={}", v.i, v.j, v.k),
        TagFieldData::RealQuaternion(q) => format!("i={}, j={}, k={}, w={}", q.i, q.j, q.k, q.w),
        TagFieldData::RealEulerAngles2d(e) => format!("yaw={}, pitch={}", e.yaw, e.pitch),
        TagFieldData::RealEulerAngles3d(e) => format!("yaw={}, pitch={}, roll={}", e.yaw, e.pitch, e.roll),
        TagFieldData::RealPlane2d(p) => format!("i={}, j={}, d={}", p.i, p.j, p.d),
        TagFieldData::RealPlane3d(p) => format!("i={}, j={}, k={}, d={}", p.i, p.j, p.k, p.d),

        TagFieldData::RgbColor(c) => format!("0x{:08X}", c.0),
        TagFieldData::ArgbColor(c) => format!("0x{:08X}", c.0),
        TagFieldData::RealRgbColor(c) => format!("r={}, g={}, b={}", c.red, c.green, c.blue),
        TagFieldData::RealArgbColor(c) => format!("a={}, r={}, g={}, b={}", c.alpha, c.red, c.green, c.blue),
        TagFieldData::RealHsvColor(c) => format!("h={}, s={}, v={}", c.hue, c.saturation, c.value),
        TagFieldData::RealAhsvColor(c) => format!("a={}, h={}, s={}, v={}", c.alpha, c.hue, c.saturation, c.value),

        TagFieldData::ShortIntegerBounds(b) => format!("{}..{}", b.lower, b.upper),
        TagFieldData::AngleBounds(b) | TagFieldData::RealBounds(b) | TagFieldData::FractionBounds(b) => {
            format!("{}..{}", b.lower, b.upper)
        }

        TagFieldData::Custom(d) => format!("custom [{} bytes]", d.len()),
    }
}

fn format_enum(value: i64, name: Option<&str>) -> String {
    match name {
        Some(n) => format!("{} ({})", value, n),
        None => value.to_string(),
    }
}

fn format_flags(value: u64, names: &[(u32, String)], hex_width: usize) -> String {
    let value_str = format!("0x{:0width$X}", value, width = hex_width);
    if names.is_empty() {
        format!("{} (none set)", value_str)
    } else {
        let joined = names
            .iter()
            .map(|(_, n)| n.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        format!("{} [{}]", value_str, joined)
    }
}

fn format_block_index(value: i64) -> String {
    if value == -1 { "NONE".into() } else { value.to_string() }
}

/// Render a string-id as a quoted string, or `"NONE"` for the
/// empty (sentinel) form.
pub fn format_string_id(s: &StringIdData) -> String {
    if s.string.is_empty() { "NONE".into() } else { format!("\"{}\"", s.string) }
}

/// Render a tag reference as `GROUP:path`, or `"NONE"` for null refs.
pub fn format_tag_reference(r: &TagReferenceData) -> String {
    match &r.group_tag_and_name {
        None => "NONE".into(),
        Some((tag, path)) => format!("{}:{}", format_tag_group(*tag), path),
    }
}

/// Render a 4-byte group tag as its ASCII form. Thin re-export of
/// [`blam_tags::fields::format_group_tag`] under the CLI's `format::`
/// module so callers can stay within one namespace.
pub fn format_tag_group(tag: u32) -> String {
    format_group_tag(tag)
}

/// Convert a `TagFieldData` to a JSON value. Preserves enum/flag
/// names alongside the raw integers; containers aren't handled here
/// (inspect/get's JSON walker handles descent).
pub fn value_to_json(value: &TagFieldData) -> Value {
    match value {
        TagFieldData::String(s) | TagFieldData::LongString(s) => json!(s),

        TagFieldData::StringId(s) | TagFieldData::OldStringId(s) => json!({ "string": s.string }),
        TagFieldData::TagReference(r) => match &r.group_tag_and_name {
            None => json!(null),
            Some((tag, path)) => json!({ "group": format_tag_group(*tag), "path": path }),
        },
        TagFieldData::Data(d) => json!({ "size": d.len() }),

        TagFieldData::CharInteger(v) => json!(v),
        TagFieldData::ShortInteger(v) => json!(v),
        TagFieldData::LongInteger(v) => json!(v),
        TagFieldData::Int64Integer(v) => json!(v),
        TagFieldData::Tag(v) => json!(format_tag_group(*v)),

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

