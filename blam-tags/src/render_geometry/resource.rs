//! Parse a `render_geometry_api_resource_definition` from the
//! fixed-up control_data bytes of an xsync resource.
//!
//! The on-disk layout (Halo 4 X360 monolithic, BE) is the C++
//! struct memory dump:
//!
//! ```text
//! struct s_render_geometry_api_resource {
//!     tag_block<vertex_buffers_block>        pc_vertex_buffers;   // 12 B
//!     tag_block<index_buffers_block>         pc_index_buffers;    // 12 B
//!     tag_block<render_vertex_buffer_block>  xenon_vertex_buffers;// 12 B
//!     tag_block<render_index_buffer_block>   xenon_index_buffers; // 12 B
//! };  // 48 B
//! ```
//!
//! Each `tag_block` field is `{ count: u32, address: u32, pad: u32 }`
//! (12 bytes). After control fixups, `address` is a
//! [`crate::monolithic::FixupAddress`] pointing into the control
//! data itself (for sub-block element arrays) or the primary /
//! secondary buffer (for raw vertex / index bytes).
//!
//! The xenon variants wrap a `tag_interop` that ultimately resolves
//! to a `render_vertex_buffer_descriptor_struct` / `…_index_…`
//! holding the per-buffer metadata plus the `TagData` reference to
//! the actual bytes. We parse the descriptors here.

use crate::monolithic::{FixupAddress, FixupTier};

/// One vertex buffer descriptor parsed from the resource's control
/// data. Mirrors the schema's
/// `render_vertex_buffer_descriptor_struct` (28 bytes).
#[derive(Debug, Clone, Copy)]
pub struct VertexBufferDescriptor {
    /// Number of vertices in this buffer.
    pub vertex_count: u32,
    /// Engine-internal vertex declaration ID. Different from
    /// `mesh_vertex_type_definition` enum values; encodes the GPU
    /// vertex layout (D3D9 VertexDeclaration index).
    pub declaration: u16,
    /// Bytes per vertex.
    pub stride: u16,
    /// Total byte count of the vertex data (`stride * vertex_count`
    /// plus engine-internal alignment).
    pub data_size: u32,
    /// Where the actual vertex bytes live — almost always
    /// [`FixupTier::Primary`] (offset = byte offset into the primary
    /// cache buffer). Read via
    /// [`crate::monolithic::XSyncState::apply_control_fixups`]'d
    /// control data.
    pub data_address: FixupAddress,
}

/// One index buffer descriptor parsed from the resource's control
/// data. Mirrors `render_index_buffer_descriptor_struct` (28 bytes).
#[derive(Debug, Clone, Copy)]
pub struct IndexBufferDescriptor {
    /// D3D primitive type. `5` = triangle strip, `4` = triangle
    /// list. Matches `D3DPRIMITIVETYPE` on Xenos.
    pub primitive_type: u32,
    /// `0` for `u16` indices, non-zero for `u32` indices.
    pub is_index32: bool,
    /// Total byte count of the index data.
    pub data_size: u32,
    /// Where the index bytes live — almost always
    /// [`FixupTier::Primary`].
    pub data_address: FixupAddress,
}

/// Fully parsed render-geometry API resource. All counts /
/// addresses come from the fixed-up control_data; the actual byte
/// payloads live in `primary_buffer` / `secondary_buffer`.
#[derive(Debug, Clone, Default)]
pub struct RenderGeometryResource {
    /// PC-platform vertex buffers (not used in X360 monolithic
    /// — kept for tags ported from MCC PC). Count is typically zero
    /// on dev builds.
    pub pc_vertex_buffers: Vec<VertexBufferDescriptor>,
    /// PC-platform index buffers (typically empty on X360 monolithic).
    pub pc_index_buffers: Vec<IndexBufferDescriptor>,
    /// Xenon-platform vertex buffers. Populated on X360 monolithic.
    pub xenon_vertex_buffers: Vec<VertexBufferDescriptor>,
    /// Xenon-platform index buffers. Populated on X360 monolithic.
    pub xenon_index_buffers: Vec<IndexBufferDescriptor>,
}

impl RenderGeometryResource {
    /// Parse the resource definition starting at
    /// `root_address.offset` inside the fixed-up control data.
    /// Returns `None` if any of the structural reads run past the
    /// end of `control_data` — the caller should treat that as a
    /// malformed resource.
    pub fn parse(control_data: &[u8], root_address: FixupAddress) -> Option<Self> {
        if root_address.tier() != FixupTier::Control {
            return None;
        }
        let root = root_address.offset() as usize;
        // 48-byte root struct = 4 × tag_block (12 bytes each).
        let pc_v_block = read_tag_block(control_data, root + 0)?;
        let pc_i_block = read_tag_block(control_data, root + 12)?;
        let xn_v_block = read_tag_block(control_data, root + 24)?;
        let xn_i_block = read_tag_block(control_data, root + 36)?;

        let mut me = Self::default();
        me.pc_vertex_buffers = read_vertex_buffers(control_data, pc_v_block)?;
        me.pc_index_buffers = read_index_buffers(control_data, pc_i_block)?;
        me.xenon_vertex_buffers = read_xenon_vertex_buffers(control_data, xn_v_block)?;
        me.xenon_index_buffers = read_xenon_index_buffers(control_data, xn_i_block)?;
        Some(me)
    }

    /// Convenience: pick the best vertex-buffer list for this
    /// platform. Returns `xenon_vertex_buffers` if populated, else
    /// `pc_vertex_buffers`.
    pub fn vertex_buffers(&self) -> &[VertexBufferDescriptor] {
        if !self.xenon_vertex_buffers.is_empty() {
            &self.xenon_vertex_buffers
        } else {
            &self.pc_vertex_buffers
        }
    }

    /// Convenience: pick the best index-buffer list for this
    /// platform.
    pub fn index_buffers(&self) -> &[IndexBufferDescriptor] {
        if !self.xenon_index_buffers.is_empty() {
            &self.xenon_index_buffers
        } else {
            &self.pc_index_buffers
        }
    }
}

//================================================================================
// Sub-parsers
//================================================================================

/// `(count, FixupAddress)` from a 12-byte tag_block field. Returns
/// `None` on truncated input. The trailing 4 bytes (`pad`) are
/// ignored.
fn read_tag_block(bytes: &[u8], at: usize) -> Option<(u32, FixupAddress)> {
    if at + 12 > bytes.len() {
        return None;
    }
    let count = u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let addr = u32::from_be_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]);
    Some((count, FixupAddress(addr)))
}

/// PC vertex buffers — each element is the schema's
/// `vertex_buffers_block` (16 bytes):
/// `(declaration_type:u8, stride:u8, pad:u16, count:u32,
///   d3d_hw_format:u32, d3d_shader_view:u32)`. There's no data
/// pointer on the PC side — these are metadata mirrors of the
/// xenon list.
fn read_vertex_buffers(
    control: &[u8],
    (count, addr): (u32, FixupAddress),
) -> Option<Vec<VertexBufferDescriptor>> {
    if addr.is_null() || count == 0 {
        return Some(Vec::new());
    }
    if addr.tier() != FixupTier::Control {
        return Some(Vec::new());
    }
    let base = addr.offset() as usize;
    let elem_size = 16;
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let off = base + i * elem_size;
        if off + elem_size > control.len() {
            return None;
        }
        let decl = control[off];
        let stride = control[off + 1];
        let cnt = u32::from_be_bytes([
            control[off + 4], control[off + 5], control[off + 6], control[off + 7],
        ]);
        out.push(VertexBufferDescriptor {
            vertex_count: cnt,
            declaration: decl as u16,
            stride: stride as u16,
            data_size: 0,
            data_address: FixupAddress(0),
        });
    }
    Some(out)
}

/// PC index buffers — schema's `index_buffers_block` (12 bytes):
/// `(declaration_type:u8, stride:u8, pad:u16, count:u32,
///   d3d_hw_format:u32)`. No data pointer on the PC side.
fn read_index_buffers(
    control: &[u8],
    (count, addr): (u32, FixupAddress),
) -> Option<Vec<IndexBufferDescriptor>> {
    if addr.is_null() || count == 0 {
        return Some(Vec::new());
    }
    if addr.tier() != FixupTier::Control {
        return Some(Vec::new());
    }
    let base = addr.offset() as usize;
    let elem_size = 12;
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let off = base + i * elem_size;
        if off + elem_size > control.len() {
            return None;
        }
        let cnt = u32::from_be_bytes([
            control[off + 4], control[off + 5], control[off + 6], control[off + 7],
        ]);
        out.push(IndexBufferDescriptor {
            primitive_type: 0,
            is_index32: false,
            data_size: cnt,
            data_address: FixupAddress(0),
        });
    }
    Some(out)
}

/// Xenon vertex buffers — each element is the schema's
/// `render_vertex_buffer_block` (12 bytes), which is a single
/// `tag_interop` field. After the control_data fixups, the
/// interop's `descriptor` slot points at a
/// `render_vertex_buffer_descriptor_struct` (28 bytes) somewhere
/// else in the control_data; the descriptor's `vertices` TagData
/// then points at the primary/secondary buffer.
fn read_xenon_vertex_buffers(
    control: &[u8],
    (count, addr): (u32, FixupAddress),
) -> Option<Vec<VertexBufferDescriptor>> {
    if addr.is_null() || count == 0 {
        return Some(Vec::new());
    }
    if addr.tier() != FixupTier::Control {
        return Some(Vec::new());
    }
    let base = addr.offset() as usize;
    let elem_size = 12;
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let off = base + i * elem_size;
        let desc_addr = read_be_u32(control, off)?;
        let desc_addr = FixupAddress(desc_addr);
        if desc_addr.tier() != FixupTier::Control {
            continue;
        }
        let desc = parse_vertex_descriptor(control, desc_addr.offset() as usize)?;
        out.push(desc);
    }
    Some(out)
}

/// Xenon index buffers — same wrapper shape as vertex buffers,
/// pointing at a `render_index_buffer_descriptor_struct` (28
/// bytes).
fn read_xenon_index_buffers(
    control: &[u8],
    (count, addr): (u32, FixupAddress),
) -> Option<Vec<IndexBufferDescriptor>> {
    if addr.is_null() || count == 0 {
        return Some(Vec::new());
    }
    if addr.tier() != FixupTier::Control {
        return Some(Vec::new());
    }
    let base = addr.offset() as usize;
    let elem_size = 12;
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let off = base + i * elem_size;
        let desc_addr = read_be_u32(control, off)?;
        let desc_addr = FixupAddress(desc_addr);
        if desc_addr.tier() != FixupTier::Control {
            continue;
        }
        let desc = parse_index_descriptor(control, desc_addr.offset() as usize)?;
        out.push(desc);
    }
    Some(out)
}

/// Parse the 28-byte vertex-buffer descriptor at `at`. Schema
/// fields: `vertex_count:u32, declaration:u16, stride:u16,
/// vertices:TagData (20 bytes)`.
fn parse_vertex_descriptor(control: &[u8], at: usize) -> Option<VertexBufferDescriptor> {
    if at + 28 > control.len() {
        return None;
    }
    let vertex_count = read_be_u32(control, at)?;
    let declaration = read_be_u16(control, at + 4)?;
    let stride = read_be_u16(control, at + 6)?;
    let (data_size, data_address) = read_tag_data(control, at + 8)?;
    Some(VertexBufferDescriptor {
        vertex_count,
        declaration,
        stride,
        data_size,
        data_address,
    })
}

/// Parse the 28-byte index-buffer descriptor at `at`. Schema
/// fields: `primitive_type:u32, is_index32:u8, pad[3],
/// index_data:TagData (20 bytes)`.
fn parse_index_descriptor(control: &[u8], at: usize) -> Option<IndexBufferDescriptor> {
    if at + 28 > control.len() {
        return None;
    }
    let primitive_type = read_be_u32(control, at)?;
    let is_index32 = control[at + 4] != 0;
    let (data_size, data_address) = read_tag_data(control, at + 8)?;
    Some(IndexBufferDescriptor {
        primitive_type,
        is_index32,
        data_size,
        data_address,
    })
}

/// `TagData` on Xenos = 20 bytes:
/// `(size:u32, flags:u32, unused:u32, address:u32, unused:u32)`.
/// Returns `(size, address)`.
fn read_tag_data(bytes: &[u8], at: usize) -> Option<(u32, FixupAddress)> {
    if at + 20 > bytes.len() {
        return None;
    }
    let size = read_be_u32(bytes, at)?;
    // bytes 4..12 are flags + unused — ignored.
    let address = read_be_u32(bytes, at + 12)?;
    Some((size, FixupAddress(address)))
}

fn read_be_u32(bytes: &[u8], at: usize) -> Option<u32> {
    bytes.get(at..at + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn read_be_u16(bytes: &[u8], at: usize) -> Option<u16> {
    bytes.get(at..at + 2).map(|s| u16::from_be_bytes([s[0], s[1]]))
}
