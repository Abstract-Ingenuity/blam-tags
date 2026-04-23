//! Format `TagFieldData` values for CLI output.
//!
//! Default text rendering is owned by the library's `Display` impls —
//! hex mode flips the `{:#}` alternate flag. Only JSON formatting
//! stays CLI-local because its structure is CLI-opinionated.

use blam_tags::{format_group_tag, TagFieldData};
use serde_json::{json, Value};

/// Format a single `TagFieldData` value as a one-line human-readable
/// string. `hex_mode` only affects the four plain integer variants
/// (via the `Display` alternate flag); other variants render the same
/// either way.
pub fn format_value(value: &TagFieldData, hex_mode: bool) -> String {
    if hex_mode { format!("{:#}", value) } else { format!("{}", value) }
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
            Some((tag, path)) => json!({ "group": format_group_tag(*tag), "path": path }),
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
