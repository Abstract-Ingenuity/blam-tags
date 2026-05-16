//! Xbox 360 (Xenos GPU) texture format handling.
//!
//! Halo 4's X360 monolithic builds store bitmap pixels in two
//! GPU-friendly transforms relative to PC:
//!
//! 1. **2D-tiled layout.** Texture data is stored in a 32×32-tile
//!    swizzled order so the GPU can fetch any (x, y) with locality.
//!    See [`xg_address_2d_tiled_offset`] for the formula and
//!    [`detile_blocks`] for the bulk conversion to linear order.
//! 2. **Big-endian byte order within compressed blocks.** Each
//!    16-bit half of a DXT/BC block lands in memory with its bytes
//!    reversed relative to PC. The fix-up is a pairwise byte swap
//!    over the entire detiled buffer — see [`swap_byte_pairs`].
//!
//! Ported from TagTool's `XboxGraphics.XGAddress2DTiledOffset` and
//! `XGEndianSwapSurface` (Xbox-360 SDK reference implementations).

/// Compute the BLOCK offset of `(x, y)` inside a tiled buffer.
/// `width_in_blocks` is the texture's per-row block count rounded
/// up to the nearest 32 (the tile width). `texel_pitch` is the byte
/// count of one block: 8 for BC1/BC4, 16 for BC2/BC3/BC5.
///
/// Returns the block index in the tiled source buffer. Multiply by
/// `texel_pitch` to get the byte offset.
pub fn xg_address_2d_tiled_offset(
    x: u32,
    y: u32,
    width_in_blocks: u32,
    texel_pitch: u32,
) -> u32 {
    let aligned_width = (width_in_blocks + 31) & !31;
    let log_bpp = xg_log2_le16(texel_pitch);

    let macro_part: u32 = ((x >> 5) + (y >> 5) * (aligned_width >> 5)) << (log_bpp + 7);
    let micro_part: u32 = ((x & 7) + ((y & 6) << 2)) << log_bpp;

    let offset = macro_part
        + ((micro_part & !15) << 1)
        + (micro_part & 15)
        + ((y & 8) << (3 + log_bpp))
        + ((y & 1) << 4);

    let tiled_byte_offset = ((offset & !511) << 3)
        + ((offset & 448) << 2)
        + (offset & 63)
        + ((y & 16) << 7)
        + (((((y & 8) >> 2) + (x >> 3)) & 3) << 6);

    tiled_byte_offset >> log_bpp
}

/// `log2` for inputs in `1..=16` — the texel pitch (bytes per
/// block) is always 1, 2, 4, 8, or 16 for the formats we handle.
fn xg_log2_le16(value: u32) -> u32 {
    debug_assert!(value > 0 && value <= 16, "xg_log2_le16: value out of range: {value}");
    value.trailing_zeros()
}

/// Convert a tiled compressed-block buffer into a linear one. Both
/// buffers hold `width_in_blocks * height_in_blocks` block records
/// of `texel_pitch` bytes each; the tiled buffer's bytes are laid
/// out in Xenos 32-block-tile swizzled order, the linear buffer in
/// plain row-major order.
///
/// The source buffer must be at least the size of one tile-aligned
/// surface — Xenos rounds each surface up to a 32×32-block multiple.
pub fn detile_blocks(
    tiled: &[u8],
    width_in_blocks: u32,
    height_in_blocks: u32,
    texel_pitch: u32,
) -> Vec<u8> {
    let pitch = texel_pitch as usize;
    let mut linear =
        vec![0u8; (width_in_blocks as usize) * (height_in_blocks as usize) * pitch];
    for y in 0..height_in_blocks {
        for x in 0..width_in_blocks {
            let src_block = xg_address_2d_tiled_offset(x, y, width_in_blocks, texel_pitch);
            let src_off = src_block as usize * pitch;
            let dst_off = ((y * width_in_blocks + x) as usize) * pitch;
            if src_off + pitch <= tiled.len() {
                linear[dst_off..dst_off + pitch]
                    .copy_from_slice(&tiled[src_off..src_off + pitch]);
            }
        }
    }
    linear
}

/// Swap every pair of bytes in `buf`. Halo 4 X360 stores DXT5 / BC3
/// blocks (and many other 16-bit-aligned formats) with each `u16`
/// field's bytes reversed compared to PC LE. The PC decoders
/// already in [`super::decode`] expect LE; pairwise swap fixes
/// every affected field at once.
///
/// This matches TagTool's `XGEndianSwapSurface` for the
/// `GPUENDIAN_8IN16` case, which is what DXT-family formats use.
pub fn swap_byte_pairs(buf: &mut [u8]) {
    for pair in buf.chunks_exact_mut(2) {
        pair.swap(0, 1);
    }
}
