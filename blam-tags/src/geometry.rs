//! Geometry primitives shared across format-specific exporters
//! ([`crate::jms`], [`crate::ass`], future ones). All items are
//! `pub(crate)` — these are extraction-pipeline internals, not the
//! crate's public API.
//!
//! Categories:
//! - **Compression bounds**: dequantize bounds-compressed positions
//!   and texcoords from `render geometry/compression info[i]`. The
//!   six bounds floats are packed across two `real_point_3d` fields
//!   as the sequential tuple `[xmin, xmax, ymin, ymax, zmin, zmax]`
//!   (NOT min/max corners as the field type suggests).
//! - **Strip → list conversion**: restart-aware (`0xFFFF` sentinel)
//!   triangle-strip decoder with parity-flip windings + degenerate
//!   filtering. Matches TagTool's `ReadTriangleStrip` exactly.
//! - **Quaternion math**: `(i, j, k, w)` order to match
//!   [`crate::fields::TagFieldData::RealQuaternion`]. Includes
//!   matrix-basis-to-quat construction, multiplication, rotation,
//!   negation.
//! - **3-vector math**: add, sub, scale, length, cross.
//! - **Generic field readers** for the schema patterns that come up
//!   in every walker: integer-shaped scalars (handles 13 variants),
//!   string ids, points, quaternions, reals, vec3s, tag references.
//! - **BSP edge-ring walker**: shared between `collision_model` and
//!   `scenario_structure_bsp` (both have the same Halo BSP shape —
//!   surfaces walk an edge ring, each edge belongs to two surfaces,
//!   matching side decides start-vs-end vertex emission).
//!
//! World-units → JMS/ASS centimeter scale factor [`SCALE`] also
//! lives here so both format modules use the same value.

use crate::api::TagStruct;
use crate::fields::TagFieldData;

/// World-units → centimeter scale factor used by JMS / ASS export
/// (`position × SCALE` everywhere positions cross into the artist
/// source format).
pub(crate) const SCALE: f32 = 100.0;

// ---- compression bounds ----

#[derive(Debug, Clone, Copy)]
pub(crate) struct CompressionBounds {
    pub(crate) pos_compressed: bool,
    pub(crate) uv_compressed: bool,
    pub(crate) px_min: f32, pub(crate) px_max: f32,
    pub(crate) py_min: f32, pub(crate) py_max: f32,
    pub(crate) pz_min: f32, pub(crate) pz_max: f32,
    pub(crate) u_min: f32, pub(crate) u_max: f32,
    pub(crate) v_min: f32, pub(crate) v_max: f32,
}

impl CompressionBounds {
    pub(crate) fn identity() -> Self {
        Self {
            pos_compressed: false, uv_compressed: false,
            px_min: 0.0, px_max: 1.0, py_min: 0.0, py_max: 1.0, pz_min: 0.0, pz_max: 1.0,
            u_min: 0.0, u_max: 1.0, v_min: 0.0, v_max: 1.0,
        }
    }

    pub(crate) fn decompress_position(&self, p: [f32; 3]) -> [f32; 3] {
        if !self.pos_compressed { return p; }
        [
            self.px_min + p[0] * (self.px_max - self.px_min),
            self.py_min + p[1] * (self.py_max - self.py_min),
            self.pz_min + p[2] * (self.pz_max - self.pz_min),
        ]
    }

    pub(crate) fn decompress_texcoord(&self, uv: [f32; 2]) -> [f32; 2] {
        if !self.uv_compressed { return uv; }
        [
            self.u_min + uv[0] * (self.u_max - self.u_min),
            self.v_min + uv[1] * (self.v_max - self.v_min),
        ]
    }
}

/// Read `render geometry/compression info[0]`. For `render_model`
/// and sbsp clusters which share the global bounds.
pub(crate) fn read_compression_bounds(root: &TagStruct<'_>) -> CompressionBounds {
    read_compression_bounds_at(root, 0)
}

/// Read `render geometry/compression info[index]`. sbsp's instance
/// definitions carry per-definition `compression index` since each
/// instanced geometry has its own bounds. Falls back to identity if
/// the index is out of range.
pub(crate) fn read_compression_bounds_at(root: &TagStruct<'_>, index: usize) -> CompressionBounds {
    let Some(ci_block) = root.field_path("render geometry/compression info").and_then(|f| f.as_block())
        else { return CompressionBounds::identity(); };
    if index >= ci_block.len() { return CompressionBounds::identity(); }
    let ci = ci_block.element(index).unwrap();
    let mut pos_compressed = true;
    let mut uv_compressed = true;
    if let Some(TagFieldData::WordFlags { value, .. }) = ci.field("compression flags").and_then(|f| f.value()) {
        pos_compressed = (value & 0x0001) != 0;
        uv_compressed = (value & 0x0002) != 0;
    }
    let pb0 = read_point3d(&ci, "position bounds 0");
    let pb1 = read_point3d(&ci, "position bounds 1");
    let tb0 = match ci.field("texcoord bounds 0").and_then(|f| f.value()) {
        Some(TagFieldData::RealPoint2d(p)) => [p.x, p.y], _ => [0.0, 1.0],
    };
    let tb1 = match ci.field("texcoord bounds 1").and_then(|f| f.value()) {
        Some(TagFieldData::RealPoint2d(p)) => [p.x, p.y], _ => [0.0, 1.0],
    };
    CompressionBounds {
        pos_compressed, uv_compressed,
        px_min: pb0[0], px_max: pb0[1],
        py_min: pb0[2], py_max: pb1[0],
        pz_min: pb1[1], pz_max: pb1[2],
        u_min: tb0[0], u_max: tb0[1],
        v_min: tb1[0], v_max: tb1[1],
    }
}

// ---- triangle-strip → list ----

/// Restart-aware (`0xFFFF` sentinel) triangle-strip decoder. Splits
/// the strip on restart sentinels, then within each sub-strip flips
/// winding parity per local position and drops degenerate windows
/// (any two indices equal — these are splice triangles used to stitch
/// strip pieces together).
pub(crate) fn strip_to_list(strip: &[u16]) -> Vec<(u16, u16, u16)> {
    let mut out = Vec::with_capacity(strip.len().saturating_sub(2));
    for segment in strip.split(|&x| x == 0xFFFF) {
        for i in 0..segment.len().saturating_sub(2) {
            let (a, b, c) = (segment[i], segment[i + 1], segment[i + 2]);
            if a == b || b == c || a == c { continue; }
            if i % 2 == 0 { out.push((a, b, c)); }
            else          { out.push((a, c, b)); }
        }
    }
    out
}

// ---- quaternion math (i, j, k, w order) ----

pub(crate) fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let (ax, ay, az, aw) = (a[0], a[1], a[2], a[3]);
    let (bx, by, bz, bw) = (b[0], b[1], b[2], b[3]);
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// Apply a quaternion rotation to a vector via the optimized
/// two-cross-product form: `v' = v + 2 * cross(q.xyz, cross(q.xyz, v) + q.w * v)`.
pub(crate) fn quat_rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let (qx, qy, qz, qw) = (q[0], q[1], q[2], q[3]);
    let (vx, vy, vz) = (v[0], v[1], v[2]);
    let tx = 2.0 * (qy * vz - qz * vy);
    let ty = 2.0 * (qz * vx - qx * vz);
    let tz = 2.0 * (qx * vy - qy * vx);
    [
        vx + qw * tx + (qy * tz - qz * ty),
        vy + qw * ty + (qz * tx - qx * tz),
        vz + qw * tz + (qx * ty - qy * tx),
    ]
}

pub(crate) fn quat_negate(q: [f32; 4]) -> [f32; 4] { [-q[0], -q[1], -q[2], -q[3]] }

/// Construct a quaternion `(i, j, k, w)` from three column basis
/// vectors of an orthonormal rotation matrix. Standard
/// trace-and-largest-diagonal extraction.
pub(crate) fn quat_from_basis_columns(c0: [f32; 3], c1: [f32; 3], c2: [f32; 3]) -> [f32; 4] {
    let m00 = c0[0]; let m10 = c0[1]; let m20 = c0[2];
    let m01 = c1[0]; let m11 = c1[1]; let m21 = c1[2];
    let m02 = c2[0]; let m12 = c2[1]; let m22 = c2[2];
    let trace = m00 + m11 + m22;
    if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [(m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [(m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [(m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s]
    }
}

// ---- 3-vector math ----

pub(crate) fn vec3_add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(crate) fn vec3_sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn vec3_scale(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

pub(crate) fn vec3_len(a: [f32; 3]) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

pub(crate) fn vec3_cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Apply [`SCALE`] to every component — world-units → JMS/ASS cm.
pub(crate) fn scale_point(p: [f32; 3]) -> [f32; 3] {
    [p[0] * SCALE, p[1] * SCALE, p[2] * SCALE]
}

// ---- generic field readers ----

/// Read any integer-shaped field. Handles all 13 integer-like
/// `TagFieldData` variants (regular ints, block indices, custom
/// block indices, enums). Most walker code only cares about the
/// integer value, not which exact variant carries it.
pub(crate) fn read_int_any(s: &TagStruct<'_>, name: &str) -> Option<i64> {
    match s.field(name)?.value()? {
        TagFieldData::CharInteger(v) => Some(v as i64),
        TagFieldData::ShortInteger(v) => Some(v as i64),
        TagFieldData::LongInteger(v) => Some(v as i64),
        TagFieldData::Int64Integer(v) => Some(v),
        TagFieldData::CharBlockIndex(v) => Some(v as i64),
        TagFieldData::ShortBlockIndex(v) => Some(v as i64),
        TagFieldData::LongBlockIndex(v) => Some(v as i64),
        TagFieldData::CustomCharBlockIndex(v) => Some(v as i64),
        TagFieldData::CustomShortBlockIndex(v) => Some(v as i64),
        TagFieldData::CustomLongBlockIndex(v) => Some(v as i64),
        TagFieldData::CharEnum { value, .. } => Some(value as i64),
        TagFieldData::ShortEnum { value, .. } => Some(value as i64),
        TagFieldData::LongEnum { value, .. } => Some(value as i64),
        TagFieldData::ByteFlags { value, .. } => Some(value as i64),
        TagFieldData::WordFlags { value, .. } => Some(value as i64),
        TagFieldData::LongFlags { value, .. } => Some(value as i64),
        _ => None,
    }
}

pub(crate) fn read_string_id(s: &TagStruct<'_>, name: &str) -> Option<String> {
    match s.field(name)?.value()? {
        TagFieldData::StringId(sid) | TagFieldData::OldStringId(sid) =>
            Some(sid.string).filter(|s| !s.is_empty()),
        _ => None,
    }
}

pub(crate) fn read_quat(s: &TagStruct<'_>, name: &str) -> [f32; 4] {
    match s.field(name).and_then(|f| f.value()) {
        Some(TagFieldData::RealQuaternion(q)) => [q.i, q.j, q.k, q.w],
        _ => [0.0, 0.0, 0.0, 1.0],
    }
}

pub(crate) fn read_point3d(s: &TagStruct<'_>, name: &str) -> [f32; 3] {
    match s.field(name).and_then(|f| f.value()) {
        Some(TagFieldData::RealPoint3d(p)) => [p.x, p.y, p.z],
        Some(TagFieldData::RealVector3d(v)) => [v.i, v.j, v.k],
        _ => [0.0; 3],
    }
}

pub(crate) fn read_vec3(s: &TagStruct<'_>, name: &str) -> [f32; 3] {
    match s.field(name).and_then(|f| f.value()) {
        Some(TagFieldData::RealVector3d(v)) => [v.i, v.j, v.k],
        Some(TagFieldData::RealPoint3d(p)) => [p.x, p.y, p.z],
        _ => [0.0; 3],
    }
}

pub(crate) fn read_real(s: &TagStruct<'_>, name: &str) -> Option<f32> {
    match s.field(name)?.value()? {
        TagFieldData::Real(r) => Some(r),
        TagFieldData::RealFraction(r) => Some(r),
        TagFieldData::Angle(r) => Some(r),
        _ => None,
    }
}

pub(crate) fn read_tag_ref_path(s: &TagStruct<'_>, name: &str) -> Option<String> {
    match s.field(name)?.value()? {
        TagFieldData::TagReference(r) => r.group_tag_and_name.map(|(_, p)| p),
        _ => None,
    }
}

// ---- BSP edge-ring walker ----

/// Cached row of a Halo BSP `edges[]` block. Walking a surface's
/// polygon ring hammers these fields tens of thousands of times in
/// hot loops, so callers pre-cache once into a `Vec<EdgeRow>` rather
/// than re-resolving via `as_struct()` per step.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EdgeRow {
    pub(crate) start_vertex: i32,
    pub(crate) end_vertex: i32,
    pub(crate) forward_edge: i32,
    pub(crate) reverse_edge: i32,
    pub(crate) left_surface: i32,
    pub(crate) right_surface: i32,
}

/// Walk a single surface's edge ring and return the ordered list of
/// vertex indices that bound it. Each edge belongs to two surfaces;
/// the matching side decides which vertex (start vs end) to emit
/// AND which neighbour edge to follow next. Returns an empty vec on
/// malformed rings (cycles that don't return to `first_edge` within
/// a reasonable bound).
///
/// Used by both `collision_model` (object collision) and
/// `scenario_structure_bsp` (level collision).
pub(crate) fn walk_surface_ring(
    surface_index: i32,
    first_edge: i32,
    edges: &[EdgeRow],
) -> Vec<i32> {
    let mut out = Vec::new();
    let mut current = first_edge;
    let mut steps = 0;
    let max_steps = edges.len() * 2 + 8;
    loop {
        if current < 0 || (current as usize) >= edges.len() { return Vec::new(); }
        let e = edges[current as usize];
        let next = if e.left_surface == surface_index {
            out.push(e.start_vertex);
            e.forward_edge
        } else if e.right_surface == surface_index {
            out.push(e.end_vertex);
            e.reverse_edge
        } else {
            return Vec::new();
        };
        if next == first_edge { break; }
        current = next;
        steps += 1;
        if steps > max_steps { return Vec::new(); }
    }
    out
}
