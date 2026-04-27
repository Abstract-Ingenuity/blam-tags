//! Decode raw `.bitmap` pixel bytes into RGBA8 (memory order
//! `[R, G, B, A]`) for downstream TIFF / preview pipelines.
//!
//! Phase 1 covered the 14 uncompressed formats. Phase 3 wires in
//! BC1/2/3/4/5 via `bcdec_rs` plus the Halo-specific
//! `dxn_mono_alpha` codec. `ctx1` is the only block-compressed
//! schema variant still unsupported (not observed in MCC corpora).
//!
//! Channel-mapping conventions matched to the engine pipeline:
//! - Packed integer formats (`a8r8g8b8`, `x8r8g8b8`, `a4r4g4b4`,
//!   `a8y8`) follow D3D9 MSB-to-LSB naming → little-endian memory
//!   has the lowest-named channel at byte 0.
//! - "Multi-cell" formats with one cell per channel (`abgrfp16`,
//!   `abgrfp32`, `a16b16g16r16`, `signedr16g16b16a16`,
//!   `q8w8v8u8`) use DXGI-equivalent memory order (R first), per
//!   the `dxgi_format` map in [`super::dds`].
//! - Single-channel formats expand to RGBA8 with the engine's
//!   "useful preview" convention: `a8` → white-with-alpha, `y8` →
//!   replicated grey, `r8` → red-only.
//! - Signed normalmap formats (`v8u8`, `q8w8v8u8`,
//!   `signedr16g16b16a16`) are biased by +128 (or +32768 for
//!   16-bit) per channel, matching the engine's
//!   `s*(1/256)+0.5` rebias in `extract_debug_plate_copy`.
//! - HDR float formats clamp to `[0, 1]` and scale to 255 — the
//!   same loss the engine's debug-plate path applies. A future
//!   float-TIFF path can take a different lane.

use super::{BitmapError, BitmapFormat};

/// Decode one mip level of the given format into RGBA8 (memory order
/// `[R, G, B, A]`). Output length is always `width * height * 4`.
///
/// `input` must hold at least `format.level_bytes(width, height)`
/// bytes — block-compressed formats round dimensions up to the 4×4
/// block grid.
pub fn decode_to_rgba8(
    format: BitmapFormat,
    width: u32,
    height: u32,
    input: &[u8],
) -> Result<Vec<u8>, BitmapError> {
    let need = format.level_bytes(width, height) as usize;
    if input.len() < need {
        return Err(BitmapError::PixelSliceOutOfBounds {
            offset: 0,
            size: need as u64,
            available: input.len() as u64,
        });
    }

    let pixels = (width as usize) * (height as usize);
    let mut out = vec![0u8; pixels * 4];

    use BitmapFormat::*;
    match format {
        A8 => decode_a8(&input[..need], &mut out),
        Y8 => decode_y8(&input[..need], &mut out),
        R8 => decode_r8(&input[..need], &mut out),
        Ay8 => decode_ay8(&input[..need], &mut out),
        A8y8 => decode_a8y8(&input[..need], &mut out),
        A4r4g4b4 => decode_a4r4g4b4(&input[..need], &mut out),
        X8r8g8b8 => decode_x8r8g8b8(&input[..need], &mut out),
        A8r8g8b8 => decode_a8r8g8b8(&input[..need], &mut out),
        V8u8 => decode_v8u8(&input[..need], &mut out),
        Q8w8v8u8 => decode_q8w8v8u8(&input[..need], &mut out),
        A16b16g16r16 => decode_a16b16g16r16(&input[..need], &mut out),
        Signedr16g16b16a16 => decode_signedr16g16b16a16(&input[..need], &mut out),
        Abgrfp16 => decode_abgrfp16(&input[..need], &mut out),
        Abgrfp32 => decode_abgrfp32(&input[..need], &mut out),

        // Block-compressed formats — bcdec_rs ports of bcdec.
        Dxt1 => decode_bc1(&input[..need], width, height, &mut out),
        Dxt3 => decode_bc2(&input[..need], width, height, &mut out),
        Dxt5 => decode_bc3(&input[..need], width, height, &mut out),
        Dxt5a => decode_bc4(&input[..need], width, height, &mut out),
        Dxn => decode_bc5(&input[..need], width, height, &mut out),
        DxnMonoAlpha => decode_dxn_mono_alpha_rgba(&input[..need], width, height, &mut out),
    }

    Ok(out)
}

//================================================================================
// Single-channel formats
//================================================================================

/// `a8`: 1 byte = alpha. Expand as `(255, 255, 255, alpha)` so the
/// alpha channel carries the data and viewers see white-on-alpha.
fn decode_a8(input: &[u8], out: &mut [u8]) {
    for (i, &a) in input.iter().enumerate() {
        let p = i * 4;
        out[p] = 255;
        out[p + 1] = 255;
        out[p + 2] = 255;
        out[p + 3] = a;
    }
}

/// `y8`: 1 byte = luminance. Replicate to RGB, full alpha.
fn decode_y8(input: &[u8], out: &mut [u8]) {
    for (i, &y) in input.iter().enumerate() {
        let p = i * 4;
        out[p] = y;
        out[p + 1] = y;
        out[p + 2] = y;
        out[p + 3] = 255;
    }
}

/// `r8`: 1 byte = red. Other channels zero, full alpha.
fn decode_r8(input: &[u8], out: &mut [u8]) {
    for (i, &r) in input.iter().enumerate() {
        let p = i * 4;
        out[p] = r;
        out[p + 1] = 0;
        out[p + 2] = 0;
        out[p + 3] = 255;
    }
}

/// `ay8`: 1 byte replicated to alpha *and* luminance. Output the byte
/// in all four channels.
fn decode_ay8(input: &[u8], out: &mut [u8]) {
    for (i, &v) in input.iter().enumerate() {
        let p = i * 4;
        out[p] = v;
        out[p + 1] = v;
        out[p + 2] = v;
        out[p + 3] = v;
    }
}

//================================================================================
// 16-bit packed
//================================================================================

/// `a8y8`: u16 LE = `(A << 8) | Y`. Memory `[Y, A]`. Replicate Y to
/// RGB; A goes to alpha.
fn decode_a8y8(input: &[u8], out: &mut [u8]) {
    for (i, chunk) in input.chunks_exact(2).enumerate() {
        let y = chunk[0];
        let a = chunk[1];
        let p = i * 4;
        out[p] = y;
        out[p + 1] = y;
        out[p + 2] = y;
        out[p + 3] = a;
    }
}

/// `a4r4g4b4`: u16 LE with bits `AAAA RRRR GGGG BBBB`. Each 4-bit
/// nibble expanded to 8 bits via `n * 0x11` (bit replication).
fn decode_a4r4g4b4(input: &[u8], out: &mut [u8]) {
    for (i, chunk) in input.chunks_exact(2).enumerate() {
        let v = u16::from_le_bytes([chunk[0], chunk[1]]);
        let a = ((v >> 12) & 0xF) as u8;
        let r = ((v >> 8) & 0xF) as u8;
        let g = ((v >> 4) & 0xF) as u8;
        let b = (v & 0xF) as u8;
        let p = i * 4;
        out[p] = r * 0x11;
        out[p + 1] = g * 0x11;
        out[p + 2] = b * 0x11;
        out[p + 3] = a * 0x11;
    }
}

/// `v8u8`: 16-bit signed normalmap. u16 packed `V<<8 | U` → memory
/// `[U, V]` (V is the high byte). Maps to `(V, U)` in `(R, G)` per
/// the existing DDS pixelformat. Bias by `+128` so signed bytes
/// display as unsigned.
fn decode_v8u8(input: &[u8], out: &mut [u8]) {
    for (i, chunk) in input.chunks_exact(2).enumerate() {
        let u = chunk[0] as i8;
        let v = chunk[1] as i8;
        let p = i * 4;
        out[p] = (v as i16 + 128) as u8;       // R = V
        out[p + 1] = (u as i16 + 128) as u8;   // G = U
        out[p + 2] = 128;                      // B = 0.5 (z implied)
        out[p + 3] = 255;
    }
}

//================================================================================
// 32-bit packed
//================================================================================

/// `x8r8g8b8`: u32 LE bytes `[B, G, R, X]`. Output `(R, G, B, 255)`.
fn decode_x8r8g8b8(input: &[u8], out: &mut [u8]) {
    for (i, chunk) in input.chunks_exact(4).enumerate() {
        let p = i * 4;
        out[p] = chunk[2];
        out[p + 1] = chunk[1];
        out[p + 2] = chunk[0];
        out[p + 3] = 255;
    }
}

/// `a8r8g8b8`: u32 LE bytes `[B, G, R, A]`. Output `(R, G, B, A)`.
fn decode_a8r8g8b8(input: &[u8], out: &mut [u8]) {
    for (i, chunk) in input.chunks_exact(4).enumerate() {
        let p = i * 4;
        out[p] = chunk[2];
        out[p + 1] = chunk[1];
        out[p + 2] = chunk[0];
        out[p + 3] = chunk[3];
    }
}

/// `q8w8v8u8`: u32 packed `Q<<24 | W<<16 | V<<8 | U` MSB-to-LSB.
/// Memory `[U, V, W, Q]`. Per DXGI map, treats as
/// `R8G8B8A8_SNORM` → `(U, V, W, Q)` in `(R, G, B, A)`. Bias each
/// signed byte by `+128`.
fn decode_q8w8v8u8(input: &[u8], out: &mut [u8]) {
    for (i, chunk) in input.chunks_exact(4).enumerate() {
        let p = i * 4;
        for c in 0..4 {
            out[p + c] = ((chunk[c] as i8) as i16 + 128) as u8;
        }
    }
}

//================================================================================
// 64-bit / 128-bit multi-cell
//================================================================================

/// `a16b16g16r16`: 4×u16 LE in memory order `[R, G, B, A]`. Truncate
/// each u16 to its high byte (`>> 8`).
fn decode_a16b16g16r16(input: &[u8], out: &mut [u8]) {
    for (i, chunk) in input.chunks_exact(8).enumerate() {
        let r = u16::from_le_bytes([chunk[0], chunk[1]]);
        let g = u16::from_le_bytes([chunk[2], chunk[3]]);
        let b = u16::from_le_bytes([chunk[4], chunk[5]]);
        let a = u16::from_le_bytes([chunk[6], chunk[7]]);
        let p = i * 4;
        out[p] = (r >> 8) as u8;
        out[p + 1] = (g >> 8) as u8;
        out[p + 2] = (b >> 8) as u8;
        out[p + 3] = (a >> 8) as u8;
    }
}

/// `signedr16g16b16a16`: 4×i16 LE in memory order `[R, G, B, A]`.
/// Bias by `+32768`, then truncate to high byte.
fn decode_signedr16g16b16a16(input: &[u8], out: &mut [u8]) {
    for (i, chunk) in input.chunks_exact(8).enumerate() {
        let r = i16::from_le_bytes([chunk[0], chunk[1]]) as i32 + 32768;
        let g = i16::from_le_bytes([chunk[2], chunk[3]]) as i32 + 32768;
        let b = i16::from_le_bytes([chunk[4], chunk[5]]) as i32 + 32768;
        let a = i16::from_le_bytes([chunk[6], chunk[7]]) as i32 + 32768;
        let p = i * 4;
        out[p] = (r >> 8) as u8;
        out[p + 1] = (g >> 8) as u8;
        out[p + 2] = (b >> 8) as u8;
        out[p + 3] = (a >> 8) as u8;
    }
}

/// `abgrfp16`: 4×half-float in memory order `[R, G, B, A]`. Clamp
/// `[0, 1]`, scale to 255. Lossy for HDR — float TIFF will skip
/// this path.
fn decode_abgrfp16(input: &[u8], out: &mut [u8]) {
    for (i, chunk) in input.chunks_exact(8).enumerate() {
        let r = half_to_f32(u16::from_le_bytes([chunk[0], chunk[1]]));
        let g = half_to_f32(u16::from_le_bytes([chunk[2], chunk[3]]));
        let b = half_to_f32(u16::from_le_bytes([chunk[4], chunk[5]]));
        let a = half_to_f32(u16::from_le_bytes([chunk[6], chunk[7]]));
        let p = i * 4;
        out[p] = clamp_to_u8(r);
        out[p + 1] = clamp_to_u8(g);
        out[p + 2] = clamp_to_u8(b);
        out[p + 3] = clamp_to_u8(a);
    }
}

/// `abgrfp32`: 4×f32 in memory order `[R, G, B, A]`. Same clamp as
/// `abgrfp16`.
fn decode_abgrfp32(input: &[u8], out: &mut [u8]) {
    for (i, chunk) in input.chunks_exact(16).enumerate() {
        let r = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let g = f32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        let b = f32::from_le_bytes([chunk[8], chunk[9], chunk[10], chunk[11]]);
        let a = f32::from_le_bytes([chunk[12], chunk[13], chunk[14], chunk[15]]);
        let p = i * 4;
        out[p] = clamp_to_u8(r);
        out[p + 1] = clamp_to_u8(g);
        out[p + 2] = clamp_to_u8(b);
        out[p + 3] = clamp_to_u8(a);
    }
}

fn clamp_to_u8(v: f32) -> u8 {
    let clamped = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
    (clamped * 255.0 + 0.5) as u8
}

/// IEEE 754 half (1 sign + 5 exponent + 10 mantissa) → f32. Handles
/// zero, subnormals, infinity, and NaN. Hand-rolled to avoid a
/// dependency for one decoder path.
fn half_to_f32(h: u16) -> f32 {
    let sign = (h >> 15) & 1;
    let exp = (h >> 10) & 0x1F;
    let mant = h & 0x3FF;
    let sign_f = if sign == 1 { -1.0_f32 } else { 1.0 };

    if exp == 0 {
        if mant == 0 {
            sign_f * 0.0
        } else {
            // Subnormal: value = (-1)^s * mant/2^10 * 2^-14
            sign_f * (mant as f32) * 2.0_f32.powi(-24)
        }
    } else if exp == 0x1F {
        if mant == 0 { sign_f * f32::INFINITY } else { f32::NAN }
    } else {
        // Normal: value = (-1)^s * (1 + mant/2^10) * 2^(exp-15)
        let exponent = exp as i32 - 15;
        let mantissa = 1.0 + (mant as f32) / 1024.0;
        sign_f * mantissa * 2.0_f32.powi(exponent)
    }
}

//================================================================================
// Block-compressed formats
//================================================================================
//
// All BC walkers share the same outer shape:
//   1. Allocate a 4×4 staging buffer (RGBA8 = 64 bytes; smaller for
//      single/dual-channel BC4/BC5).
//   2. For each block, decode into staging via bcdec_rs.
//   3. Blit the (clipped) staging rectangle into the output mip,
//      converting channel layout for BC4/BC5 (which produce R8 / RG8,
//      not RGBA8).

/// Block-decode into a 4×4 RGBA8 staging buffer, then copy the
/// in-bounds pixels into the destination mip at `(bx, by)`.
fn blit_rgba_block(
    staging: &[u8; 64],
    out: &mut [u8],
    width: u32,
    height: u32,
    bx: u32,
    by: u32,
) {
    let w = width as usize;
    for j in 0..4u32 {
        let py = by * 4 + j;
        if py >= height { break; }
        for i in 0..4u32 {
            let px = bx * 4 + i;
            if px >= width { break; }
            let dst = (py as usize * w + px as usize) * 4;
            let src = ((j * 4 + i) as usize) * 4;
            out[dst..dst + 4].copy_from_slice(&staging[src..src + 4]);
        }
    }
}

/// `dxt1` → BC1. 8-byte block. Direct RGBA8 output from bcdec_rs.
fn decode_bc1(input: &[u8], width: u32, height: u32, out: &mut [u8]) {
    let blocks_w = ((width + 3) / 4).max(1);
    let blocks_h = ((height + 3) / 4).max(1);
    for by in 0..blocks_h {
        for bx in 0..blocks_w {
            let block_idx = (by * blocks_w + bx) as usize;
            let block = &input[block_idx * 8..(block_idx + 1) * 8];
            let mut staging = [0u8; 64];
            bcdec_rs::bc1(block, &mut staging, 16);
            blit_rgba_block(&staging, out, width, height, bx, by);
        }
    }
}

/// `dxt3` → BC2. 16-byte block.
fn decode_bc2(input: &[u8], width: u32, height: u32, out: &mut [u8]) {
    let blocks_w = ((width + 3) / 4).max(1);
    let blocks_h = ((height + 3) / 4).max(1);
    for by in 0..blocks_h {
        for bx in 0..blocks_w {
            let block_idx = (by * blocks_w + bx) as usize;
            let block = &input[block_idx * 16..(block_idx + 1) * 16];
            let mut staging = [0u8; 64];
            bcdec_rs::bc2(block, &mut staging, 16);
            blit_rgba_block(&staging, out, width, height, bx, by);
        }
    }
}

/// `dxt5` → BC3. 16-byte block.
fn decode_bc3(input: &[u8], width: u32, height: u32, out: &mut [u8]) {
    let blocks_w = ((width + 3) / 4).max(1);
    let blocks_h = ((height + 3) / 4).max(1);
    for by in 0..blocks_h {
        for bx in 0..blocks_w {
            let block_idx = (by * blocks_w + bx) as usize;
            let block = &input[block_idx * 16..(block_idx + 1) * 16];
            let mut staging = [0u8; 64];
            bcdec_rs::bc3(block, &mut staging, 16);
            blit_rgba_block(&staging, out, width, height, bx, by);
        }
    }
}

/// `dxt5a` → BC4 (single-channel). Replicate the decoded R8 value
/// into RGB and force alpha=255 so visualization shows luminance.
fn decode_bc4(input: &[u8], width: u32, height: u32, out: &mut [u8]) {
    let blocks_w = ((width + 3) / 4).max(1);
    let blocks_h = ((height + 3) / 4).max(1);
    for by in 0..blocks_h {
        for bx in 0..blocks_w {
            let block_idx = (by * blocks_w + bx) as usize;
            let block = &input[block_idx * 8..(block_idx + 1) * 8];
            let mut staging = [0u8; 16]; // 4×4 R8
            bcdec_rs::bc4(block, &mut staging, 4, false);
            let w = width as usize;
            for j in 0..4u32 {
                let py = by * 4 + j;
                if py >= height { break; }
                for i in 0..4u32 {
                    let px = bx * 4 + i;
                    if px >= width { break; }
                    let r = staging[(j * 4 + i) as usize];
                    let dst = (py as usize * w + px as usize) * 4;
                    out[dst] = r;
                    out[dst + 1] = r;
                    out[dst + 2] = r;
                    out[dst + 3] = 255;
                }
            }
        }
    }
}

/// `dxn` → BC5 (two-channel). Output `(R, G, 128, 255)` matching the
/// normalmap convention V8U8 already uses (B = z = 0.5 implied).
fn decode_bc5(input: &[u8], width: u32, height: u32, out: &mut [u8]) {
    let blocks_w = ((width + 3) / 4).max(1);
    let blocks_h = ((height + 3) / 4).max(1);
    for by in 0..blocks_h {
        for bx in 0..blocks_w {
            let block_idx = (by * blocks_w + bx) as usize;
            let block = &input[block_idx * 16..(block_idx + 1) * 16];
            let mut staging = [0u8; 32]; // 4×4 RG8
            bcdec_rs::bc5(block, &mut staging, 8, false);
            let w = width as usize;
            for j in 0..4u32 {
                let py = by * 4 + j;
                if py >= height { break; }
                for i in 0..4u32 {
                    let px = bx * 4 + i;
                    if px >= width { break; }
                    let src = ((j * 4 + i) * 2) as usize;
                    let r = staging[src];
                    let g = staging[src + 1];
                    let dst = (py as usize * w + px as usize) * 4;
                    out[dst] = r;
                    out[dst + 1] = g;
                    out[dst + 2] = 128;
                    out[dst + 3] = 255;
                }
            }
        }
    }
}

/// `dxn_mono_alpha` → custom Halo codec. Each 16-byte block is two
/// BC4-style sub-blocks back to back: `red` carries luminance,
/// `green` carries alpha. Output `(L, L, L, A)`.
///
/// Same numerical work as [`super::dds::decode_dxn_mono_alpha`] but
/// inlined here for the per-mip RGBA8 output convention. The two
/// produce byte-identical pixel data because R = G = B = luminance,
/// so the BGRA / RGBA distinction collapses.
fn decode_dxn_mono_alpha_rgba(input: &[u8], width: u32, height: u32, out: &mut [u8]) {
    let blocks_w = ((width + 3) / 4).max(1);
    let blocks_h = ((height + 3) / 4).max(1);
    let w = width as usize;
    for by in 0..blocks_h {
        for bx in 0..blocks_w {
            let block_idx = (by * blocks_w + bx) as usize;
            let block = &input[block_idx * 16..(block_idx + 1) * 16];

            let mut red_values = [0u8; 8];
            let red_indices = unpack_bc4_alpha_block(&block[0..8], &mut red_values);
            let mut green_values = [0u8; 8];
            let green_indices = unpack_bc4_alpha_block(&block[8..16], &mut green_values);

            for j in 0..4u32 {
                let py = by * 4 + j;
                if py >= height { continue; }
                for i in 0..4u32 {
                    let px = bx * 4 + i;
                    if px >= width { continue; }
                    let bit_offset = 3 * (j * 4 + i);
                    let red_idx = ((red_indices >> bit_offset) & 0x07) as usize;
                    let green_idx = ((green_indices >> bit_offset) & 0x07) as usize;
                    let r = red_values[red_idx];
                    let g = green_values[green_idx];
                    let dst = (py as usize * w + px as usize) * 4;
                    out[dst] = r;
                    out[dst + 1] = r;
                    out[dst + 2] = r;
                    out[dst + 3] = g;
                }
            }
        }
    }
}

/// 8-byte BC4-style alpha sub-block: 2 endpoint bytes + 6 bytes of
/// 3-bit indices. Fills `values` with the 8-entry palette and
/// returns the 48-bit index field as a `u64`. (Mirror of the helper
/// in [`super::dds`].)
fn unpack_bc4_alpha_block(block: &[u8], values: &mut [u8; 8]) -> u64 {
    let v0 = block[0] as u32;
    let v1 = block[1] as u32;
    values[0] = v0 as u8;
    values[1] = v1 as u8;

    if v0 > v1 {
        for i in 0..6u32 {
            values[(2 + i) as usize] = (((6 - i) * v0 + (1 + i) * v1) / 7) as u8;
        }
    } else {
        for i in 0..4u32 {
            values[(2 + i) as usize] = (((4 - i) * v0 + (1 + i) * v1) / 5) as u8;
        }
        values[6] = 0;
        values[7] = 255;
    }

    (block[2] as u64)
        | ((block[3] as u64) << 8)
        | ((block[4] as u64) << 16)
        | ((block[5] as u64) << 24)
        | ((block[6] as u64) << 32)
        | ((block[7] as u64) << 40)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(r: u8, g: u8, b: u8, a: u8) -> [u8; 4] { [r, g, b, a] }

    #[test]
    fn a8_white_with_alpha() {
        let out = decode_to_rgba8(BitmapFormat::A8, 4, 1, &[0x00, 0x80, 0xFF, 0x40]).unwrap();
        assert_eq!(&out[0..4], &rgba(255, 255, 255, 0x00));
        assert_eq!(&out[4..8], &rgba(255, 255, 255, 0x80));
        assert_eq!(&out[8..12], &rgba(255, 255, 255, 0xFF));
        assert_eq!(&out[12..16], &rgba(255, 255, 255, 0x40));
    }

    #[test]
    fn y8_replicates_to_rgb() {
        let out = decode_to_rgba8(BitmapFormat::Y8, 2, 1, &[0x00, 0x80]).unwrap();
        assert_eq!(&out[0..4], &rgba(0, 0, 0, 255));
        assert_eq!(&out[4..8], &rgba(0x80, 0x80, 0x80, 255));
    }

    #[test]
    fn r8_red_only() {
        let out = decode_to_rgba8(BitmapFormat::R8, 1, 1, &[0xCC]).unwrap();
        assert_eq!(&out, &rgba(0xCC, 0, 0, 255));
    }

    #[test]
    fn ay8_replicates_to_all_four() {
        let out = decode_to_rgba8(BitmapFormat::Ay8, 1, 1, &[0x40]).unwrap();
        assert_eq!(&out, &rgba(0x40, 0x40, 0x40, 0x40));
    }

    #[test]
    fn a8y8_y_in_rgb_a_in_alpha() {
        // bytes [Y, A] in memory
        let out = decode_to_rgba8(BitmapFormat::A8y8, 1, 1, &[0x80, 0x40]).unwrap();
        assert_eq!(&out, &rgba(0x80, 0x80, 0x80, 0x40));
    }

    #[test]
    fn a4r4g4b4_nibble_replication() {
        // u16 LE = 0xFEDC → AAAA=0xF RRRR=0xE GGGG=0xD BBBB=0xC
        let out = decode_to_rgba8(BitmapFormat::A4r4g4b4, 1, 1, &[0xDC, 0xFE]).unwrap();
        assert_eq!(&out, &rgba(0xEE, 0xDD, 0xCC, 0xFF));
    }

    #[test]
    fn x8r8g8b8_alpha_forced_to_255() {
        // Memory [B, G, R, X]
        let out = decode_to_rgba8(BitmapFormat::X8r8g8b8, 1, 1, &[0x10, 0x20, 0x30, 0xAA]).unwrap();
        assert_eq!(&out, &rgba(0x30, 0x20, 0x10, 0xFF));
    }

    #[test]
    fn a8r8g8b8_bgra_to_rgba() {
        // Memory [B, G, R, A]
        let out = decode_to_rgba8(BitmapFormat::A8r8g8b8, 1, 1, &[0x10, 0x20, 0x30, 0x40]).unwrap();
        assert_eq!(&out, &rgba(0x30, 0x20, 0x10, 0x40));
    }

    #[test]
    fn v8u8_signed_bias_to_unsigned() {
        // Memory [U, V] = [-1, 0] → expected (V+128, U+128, 128, 255) = (128, 127, 128, 255)
        let out = decode_to_rgba8(BitmapFormat::V8u8, 1, 1, &[0xFF, 0x00]).unwrap();
        assert_eq!(&out, &rgba(128, 127, 128, 255));

        // Memory [U=+127, V=-128] → (0, 255, 128, 255)
        let out = decode_to_rgba8(BitmapFormat::V8u8, 1, 1, &[0x7F, 0x80]).unwrap();
        assert_eq!(&out, &rgba(0, 255, 128, 255));
    }

    #[test]
    fn q8w8v8u8_signed_bias_per_channel() {
        // Memory [U, V, W, Q] = [-128, 0, +127, +1] → (0, 128, 255, 129)
        let out = decode_to_rgba8(BitmapFormat::Q8w8v8u8, 1, 1, &[0x80, 0x00, 0x7F, 0x01]).unwrap();
        assert_eq!(&out, &rgba(0, 128, 255, 129));
    }

    #[test]
    fn a16b16g16r16_high_byte_only() {
        // R=0xFF00, G=0x8000, B=0x0100, A=0xFFFF (LE bytes)
        let bytes = [0x00, 0xFF, 0x00, 0x80, 0x00, 0x01, 0xFF, 0xFF];
        let out = decode_to_rgba8(BitmapFormat::A16b16g16r16, 1, 1, &bytes).unwrap();
        assert_eq!(&out, &rgba(0xFF, 0x80, 0x01, 0xFF));
    }

    #[test]
    fn signedr16g16b16a16_bias_by_32768() {
        // R=-32768 → 0, G=0 → 128, B=+32767 → 255, A=-1 → 127
        let r = (-32768i16).to_le_bytes();
        let g = 0i16.to_le_bytes();
        let b = 32767i16.to_le_bytes();
        let a = (-1i16).to_le_bytes();
        let bytes = [r[0], r[1], g[0], g[1], b[0], b[1], a[0], a[1]];
        let out = decode_to_rgba8(BitmapFormat::Signedr16g16b16a16, 1, 1, &bytes).unwrap();
        assert_eq!(&out, &rgba(0, 128, 255, 127));
    }

    #[test]
    fn abgrfp16_clamp_and_scale() {
        // Half-float encodings:
        //   1.0 = 0x3C00, 0.0 = 0x0000, 0.5 = 0x3800, 2.0 = 0x4000 (clamps to 1.0)
        let bytes = [
            0x00, 0x3C, // R = 1.0
            0x00, 0x38, // G = 0.5
            0x00, 0x00, // B = 0.0
            0x00, 0x40, // A = 2.0 → clamps to 1.0
        ];
        let out = decode_to_rgba8(BitmapFormat::Abgrfp16, 1, 1, &bytes).unwrap();
        assert_eq!(&out, &rgba(255, 128, 0, 255));
    }

    #[test]
    fn abgrfp32_clamp_and_scale() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1.0_f32.to_le_bytes());   // R
        bytes.extend_from_slice(&0.5_f32.to_le_bytes());   // G
        bytes.extend_from_slice(&(-0.5_f32).to_le_bytes()); // B → clamps to 0
        bytes.extend_from_slice(&3.0_f32.to_le_bytes());   // A → clamps to 1
        let out = decode_to_rgba8(BitmapFormat::Abgrfp32, 1, 1, &bytes).unwrap();
        assert_eq!(&out, &rgba(255, 128, 0, 255));
    }

    #[test]
    fn input_too_short_returns_oob() {
        let err = decode_to_rgba8(BitmapFormat::A8r8g8b8, 2, 2, &[0u8; 12]);
        assert!(matches!(err, Err(BitmapError::PixelSliceOutOfBounds { .. })));
    }

    // --- BC sanity tests --------------------------------------------------
    //
    // We're testing our walker / dispatch / channel-mapping rather
    // than bcdec_rs's correctness — the upstream library has its
    // own test suite. So each test crafts a block whose decoded
    // output is trivially predictable (uniform color) and checks
    // we splat it across all 16 pixels with the right channel order.

    /// BC1 block with both endpoints = solid red (R5G6B5 = 0xF800).
    /// Expect every pixel to be opaque red.
    #[test]
    fn bc1_solid_red_block() {
        let mut block = [0u8; 8];
        block[0..2].copy_from_slice(&0xF800u16.to_le_bytes()); // color0
        block[2..4].copy_from_slice(&0xF800u16.to_le_bytes()); // color1
        // index bits all 0 → palette[0] = color0 = red
        let out = decode_to_rgba8(BitmapFormat::Dxt1, 4, 4, &block).unwrap();
        for i in 0..16 {
            assert_eq!(&out[i * 4..i * 4 + 4], &[0xFF, 0x00, 0x00, 0xFF]);
        }
    }

    /// BC4 block (DXT5A) with both endpoints = 0x80. Expect uniform
    /// grey replicated to RGB with full alpha.
    #[test]
    fn bc4_dxt5a_replicates_to_grey() {
        let mut block = [0u8; 8];
        block[0] = 0x80;
        block[1] = 0x80;
        let out = decode_to_rgba8(BitmapFormat::Dxt5a, 4, 4, &block).unwrap();
        for i in 0..16 {
            assert_eq!(&out[i * 4..i * 4 + 4], &[0x80, 0x80, 0x80, 0xFF]);
        }
    }

    /// BC5 block (DXN) with red sub-block = 0x40 and green sub-block
    /// = 0xC0. Expect (R=0x40, G=0xC0, B=128, A=255) at every pixel.
    #[test]
    fn bc5_dxn_two_channel_with_neutral_blue() {
        let mut block = [0u8; 16];
        // Red sub-block: both endpoints = 0x40
        block[0] = 0x40;
        block[1] = 0x40;
        // Green sub-block: both endpoints = 0xC0
        block[8] = 0xC0;
        block[9] = 0xC0;
        let out = decode_to_rgba8(BitmapFormat::Dxn, 4, 4, &block).unwrap();
        for i in 0..16 {
            assert_eq!(&out[i * 4..i * 4 + 4], &[0x40, 0xC0, 0x80, 0xFF]);
        }
    }

    /// `dxn_mono_alpha` block with luminance=0xA0, alpha=0x60.
    /// Expect (0xA0, 0xA0, 0xA0, 0x60).
    #[test]
    fn dxn_mono_alpha_lum_in_rgb_alpha_in_alpha() {
        let mut block = [0u8; 16];
        // Red sub-block (luminance) endpoints
        block[0] = 0xA0;
        block[1] = 0xA0;
        // Green sub-block (alpha) endpoints
        block[8] = 0x60;
        block[9] = 0x60;
        let out = decode_to_rgba8(BitmapFormat::DxnMonoAlpha, 4, 4, &block).unwrap();
        for i in 0..16 {
            assert_eq!(&out[i * 4..i * 4 + 4], &[0xA0, 0xA0, 0xA0, 0x60]);
        }
    }

    /// Sub-4-pixel mip (1×1) for a BC format: input is still one
    /// 4×4 block, output should hold one valid pixel.
    #[test]
    fn bc1_1x1_mip_single_pixel_decodes() {
        let mut block = [0u8; 8];
        block[0..2].copy_from_slice(&0x07E0u16.to_le_bytes()); // R5G6B5 green
        block[2..4].copy_from_slice(&0x07E0u16.to_le_bytes());
        let out = decode_to_rgba8(BitmapFormat::Dxt1, 1, 1, &block).unwrap();
        assert_eq!(out.len(), 4);
        assert_eq!(&out, &[0x00, 0xFF, 0x00, 0xFF]);
    }
}
