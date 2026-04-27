//! DDS writer + Halo-specific decompressors used by the bitmap →
//! DDS pipeline.
//!
//! Two output paths:
//!
//! - [`write_dds`] — legacy DDS (4-byte magic + 124-byte header +
//!   raw pixels), the path most readers handle. Used for every
//!   format that has a clean legacy `D3DFMT` / `DDPF_*` expression.
//! - [`write_dds_dx10`] — adds the 20-byte `DDS_HEADER_DXT10`
//!   extension, used for texture arrays (no legacy `arraySize`
//!   slot) and for formats with no legacy expression (currently
//!   just `signedr16g16b16a16`).
//!
//! [`decode_dxn_mono_alpha`] is the CPU decompressor for the
//! Halo-specific BC5-shaped `(luminance, alpha)` layout. No DDS
//! reader does the `(R, R, R, A)` swizzle automatically, so we
//! decode to A8R8G8B8 before writing.

use std::io::Write;

use super::BitmapFormat;

const DDS_MAGIC: &[u8; 4] = b"DDS ";

const DDSD_CAPS: u32 = 0x1;
const DDSD_HEIGHT: u32 = 0x2;
const DDSD_WIDTH: u32 = 0x4;
const DDSD_PITCH: u32 = 0x8;
const DDSD_PIXELFORMAT: u32 = 0x1000;
const DDSD_MIPMAPCOUNT: u32 = 0x20000;
const DDSD_LINEARSIZE: u32 = 0x80000;

const DDPF_ALPHAPIXELS: u32 = 0x1;
const DDPF_ALPHA: u32 = 0x2;
const DDPF_FOURCC: u32 = 0x4;
const DDPF_RGB: u32 = 0x40;
const DDPF_LUMINANCE: u32 = 0x20000;

const DDSCAPS_COMPLEX: u32 = 0x8;
const DDSCAPS_TEXTURE: u32 = 0x1000;
const DDSCAPS_MIPMAP: u32 = 0x400000;

const DDSCAPS2_CUBEMAP: u32 = 0x200;
const DDSCAPS2_CUBEMAP_ALL_FACES: u32 = 0xFC00;

fn fourcc(s: &[u8; 4]) -> u32 { u32::from_le_bytes(*s) }

struct PixelFormat {
    flags: u32, fourcc: u32, rgb_bit_count: u32,
    r_mask: u32, g_mask: u32, b_mask: u32, a_mask: u32,
}

fn pf_fourcc(value: u32) -> PixelFormat {
    PixelFormat {
        flags: DDPF_FOURCC, fourcc: value, rgb_bit_count: 0,
        r_mask: 0, g_mask: 0, b_mask: 0, a_mask: 0,
    }
}

fn pixel_format(format: BitmapFormat) -> PixelFormat {
    use BitmapFormat::*;
    match format {
        Dxt1 => pf_fourcc(fourcc(b"DXT1")),
        Dxt3 => pf_fourcc(fourcc(b"DXT3")),
        Dxt5 => pf_fourcc(fourcc(b"DXT5")),
        Dxt5a => pf_fourcc(fourcc(b"ATI1")),
        Dxn => pf_fourcc(fourcc(b"ATI2")),

        // D3DFMT enum value passed verbatim through fourcc — readers
        // treat fourcc values < 0x10000 as D3DFMT identifiers.
        Q8w8v8u8 => pf_fourcc(63),     // D3DFMT_Q8W8V8U8
        Abgrfp16 => pf_fourcc(113),    // D3DFMT_A16B16G16R16F
        Abgrfp32 => pf_fourcc(116),    // D3DFMT_A32B32G32R32F
        A16b16g16r16 => pf_fourcc(36), // D3DFMT_A16B16G16R16

        A8 => PixelFormat {
            flags: DDPF_ALPHA, fourcc: 0, rgb_bit_count: 8,
            r_mask: 0, g_mask: 0, b_mask: 0, a_mask: 0xFF,
        },
        Y8 | R8 => PixelFormat {
            flags: DDPF_LUMINANCE, fourcc: 0, rgb_bit_count: 8,
            r_mask: 0xFF, g_mask: 0, b_mask: 0, a_mask: 0,
        },
        Ay8 => PixelFormat {
            // 8-bit replicated to ARGB — TagTool mirrors the A8 layout
            // here (alpha-only readers see the right shape).
            flags: DDPF_ALPHA, fourcc: 0, rgb_bit_count: 8,
            r_mask: 0, g_mask: 0, b_mask: 0, a_mask: 0xFF,
        },
        A8y8 => PixelFormat {
            flags: DDPF_LUMINANCE | DDPF_ALPHAPIXELS, fourcc: 0, rgb_bit_count: 16,
            r_mask: 0x00FF, g_mask: 0, b_mask: 0, a_mask: 0xFF00,
        },
        A4r4g4b4 => PixelFormat {
            flags: DDPF_RGB | DDPF_ALPHAPIXELS, fourcc: 0, rgb_bit_count: 16,
            r_mask: 0x0F00, g_mask: 0x00F0, b_mask: 0x000F, a_mask: 0xF000,
        },
        X8r8g8b8 => PixelFormat {
            flags: DDPF_RGB, fourcc: 0, rgb_bit_count: 32,
            r_mask: 0x00FF0000, g_mask: 0x0000FF00, b_mask: 0x000000FF, a_mask: 0,
        },
        A8r8g8b8 => PixelFormat {
            flags: DDPF_RGB | DDPF_ALPHAPIXELS, fourcc: 0, rgb_bit_count: 32,
            r_mask: 0x00FF0000, g_mask: 0x0000FF00, b_mask: 0x000000FF, a_mask: 0xFF000000,
        },
        V8u8 => PixelFormat {
            // Signed bumpmap — TagTool writes RG masks. Most viewers
            // display the bytes as RG; signedness interpretation is
            // up to the reader.
            flags: DDPF_RGB, fourcc: 0, rgb_bit_count: 16,
            r_mask: 0xFF00, g_mask: 0x00FF, b_mask: 0, a_mask: 0,
        },

        // DXT10-only and decoder-only formats are routed away from
        // this function by the dispatch in `BitmapImage::write_dds`.
        // Reaching this arm would mean the legacy DDS writer was
        // called for a format that has no legacy expression — a
        // programming bug.
        Signedr16g16b16a16 | DxnMonoAlpha => unreachable!(
            "`{format:?}` does not have a legacy DDS pixelformat — caller should have routed elsewhere"
        ),
    }
}

/// Write a DDS file: 4-byte magic + 124-byte header + raw pixel
/// bytes. `mipmap_levels` is the total count including the base.
/// `is_cube` lays out 6 faces — caller is responsible for ensuring
/// `pixel_bytes` already has them in DDS face order
/// (+X, -X, +Y, -Y, +Z, -Z), each face's mip chain laid out
/// contiguously.
pub fn write_dds(
    out: &mut impl Write,
    format: BitmapFormat,
    width: u32,
    height: u32,
    mipmap_levels: u32,
    is_cube: bool,
    pixel_bytes: &[u8],
) -> std::io::Result<()> {
    let pf = pixel_format(format);

    let mut flags = DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT;
    if mipmap_levels > 1 {
        flags |= DDSD_MIPMAPCOUNT;
    }

    // Pitch (uncompressed) or linear size (compressed). DDS requires
    // one or the other; most readers ignore the value if the surface
    // bytes are correctly sized but some older tools care.
    let (pitch_or_linear_size, size_flag) = if format.is_compressed() {
        (format.level_bytes(width, height) as u32, DDSD_LINEARSIZE)
    } else {
        let pitch = (width * pf.rgb_bit_count + 7) / 8;
        (pitch, DDSD_PITCH)
    };
    flags |= size_flag;

    let mut caps = DDSCAPS_TEXTURE;
    if mipmap_levels > 1 {
        caps |= DDSCAPS_MIPMAP | DDSCAPS_COMPLEX;
    }
    if is_cube {
        caps |= DDSCAPS_COMPLEX;
    }
    let caps2 = if is_cube {
        DDSCAPS2_CUBEMAP | DDSCAPS2_CUBEMAP_ALL_FACES
    } else {
        0
    };

    out.write_all(DDS_MAGIC)?;
    write_u32(out, 124)?;                    // dwSize
    write_u32(out, flags)?;                  // dwFlags
    write_u32(out, height)?;                 // dwHeight
    write_u32(out, width)?;                  // dwWidth
    write_u32(out, pitch_or_linear_size)?;   // dwPitchOrLinearSize
    write_u32(out, 0)?;                      // dwDepth
    write_u32(out, mipmap_levels)?;          // dwMipMapCount
    for _ in 0..11 { write_u32(out, 0)?; }   // dwReserved1[11]

    // DDS_PIXELFORMAT (32 bytes)
    write_u32(out, 32)?;                     // dwSize
    write_u32(out, pf.flags)?;
    write_u32(out, pf.fourcc)?;
    write_u32(out, pf.rgb_bit_count)?;
    write_u32(out, pf.r_mask)?;
    write_u32(out, pf.g_mask)?;
    write_u32(out, pf.b_mask)?;
    write_u32(out, pf.a_mask)?;

    write_u32(out, caps)?;                   // dwCaps
    write_u32(out, caps2)?;                  // dwCaps2
    write_u32(out, 0)?;                      // dwCaps3
    write_u32(out, 0)?;                      // dwCaps4
    write_u32(out, 0)?;                      // dwReserved2

    out.write_all(pixel_bytes)?;
    Ok(())
}

fn write_u32(out: &mut impl Write, v: u32) -> std::io::Result<()> {
    out.write_all(&v.to_le_bytes())
}

//================================================================================
// DXT10 extension (texture arrays)
//================================================================================
//
// Legacy DDS has no slot for `arraySize`, so for `type = array`
// bitmaps we emit the DDS_HEADER_DXT10 extension. The pixelformat in
// the legacy header carries fourcc "DX10" as a marker; the real
// format identification lives in the DXT10 header's `dxgiFormat`.

const DXGI_FORMAT_R32G32B32A32_FLOAT: u32 = 2;
const DXGI_FORMAT_R16G16B16A16_FLOAT: u32 = 10;
const DXGI_FORMAT_R16G16B16A16_UNORM: u32 = 11;
const DXGI_FORMAT_R16G16B16A16_SNORM: u32 = 13;
const DXGI_FORMAT_R8G8B8A8_SNORM: u32 = 32;
const DXGI_FORMAT_R8G8_SNORM: u32 = 51;
const DXGI_FORMAT_R8G8_UNORM: u32 = 49;
const DXGI_FORMAT_R8_UNORM: u32 = 61;
const DXGI_FORMAT_A8_UNORM: u32 = 65;
const DXGI_FORMAT_BC1_UNORM: u32 = 71;
const DXGI_FORMAT_BC2_UNORM: u32 = 74;
const DXGI_FORMAT_BC3_UNORM: u32 = 77;
const DXGI_FORMAT_BC4_UNORM: u32 = 80;
const DXGI_FORMAT_BC5_UNORM: u32 = 83;
const DXGI_FORMAT_B8G8R8A8_UNORM: u32 = 87;
const DXGI_FORMAT_B8G8R8X8_UNORM: u32 = 88;
const DXGI_FORMAT_B4G4R4A4_UNORM: u32 = 115;

const D3D10_RESOURCE_DIMENSION_TEXTURE2D: u32 = 3;

fn dxgi_format(format: BitmapFormat) -> u32 {
    use BitmapFormat::*;
    match format {
        A8 => DXGI_FORMAT_A8_UNORM,
        Y8 | R8 | Ay8 => DXGI_FORMAT_R8_UNORM,
        A8y8 => DXGI_FORMAT_R8G8_UNORM,
        A4r4g4b4 => DXGI_FORMAT_B4G4R4A4_UNORM,
        X8r8g8b8 => DXGI_FORMAT_B8G8R8X8_UNORM,
        A8r8g8b8 => DXGI_FORMAT_B8G8R8A8_UNORM,
        Dxt1 => DXGI_FORMAT_BC1_UNORM,
        Dxt3 => DXGI_FORMAT_BC2_UNORM,
        Dxt5 => DXGI_FORMAT_BC3_UNORM,
        Dxt5a => DXGI_FORMAT_BC4_UNORM,
        Dxn => DXGI_FORMAT_BC5_UNORM,
        V8u8 => DXGI_FORMAT_R8G8_SNORM,
        Q8w8v8u8 => DXGI_FORMAT_R8G8B8A8_SNORM,
        Abgrfp16 => DXGI_FORMAT_R16G16B16A16_FLOAT,
        Abgrfp32 => DXGI_FORMAT_R32G32B32A32_FLOAT,
        A16b16g16r16 => DXGI_FORMAT_R16G16B16A16_UNORM,
        Signedr16g16b16a16 => DXGI_FORMAT_R16G16B16A16_SNORM,
        // Decoder substitutes A8R8G8B8 before reaching this point.
        DxnMonoAlpha => unreachable!("`DxnMonoAlpha` should be decoded to A8R8G8B8 before DXT10 dispatch"),
    }
}

/// Write a DDS file with the DXT10 extension header. Used for
/// texture arrays (set `layer_count > 1`) and for formats with no
/// clean legacy DDS expression (e.g. `signedr16g16b16a16`). Surface
/// bytes are laid out layer-major: layer 0 mip 0 → layer 0 mip N →
/// layer 1 mip 0 → … (matching what the bitmap tag stores).
pub fn write_dds_dx10(
    out: &mut impl Write,
    format: BitmapFormat,
    width: u32,
    height: u32,
    mipmap_levels: u32,
    layer_count: u32,
    pixel_bytes: &[u8],
) -> std::io::Result<()> {
    // Pixelformat block uses fourcc "DX10" as the extension marker.
    let mut flags = DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT;
    if mipmap_levels > 1 { flags |= DDSD_MIPMAPCOUNT; }
    let pitch_or_linear_size = if format.is_compressed() {
        format.level_bytes(width, height) as u32
    } else {
        let bpp = format.bytes_per_pixel() * 8;
        (width * bpp + 7) / 8
    };
    flags |= if format.is_compressed() { DDSD_LINEARSIZE } else { DDSD_PITCH };

    let mut caps = DDSCAPS_TEXTURE;
    if mipmap_levels > 1 { caps |= DDSCAPS_MIPMAP | DDSCAPS_COMPLEX; }
    if layer_count > 1 { caps |= DDSCAPS_COMPLEX; }

    out.write_all(DDS_MAGIC)?;
    write_u32(out, 124)?;
    write_u32(out, flags)?;
    write_u32(out, height)?;
    write_u32(out, width)?;
    write_u32(out, pitch_or_linear_size)?;
    write_u32(out, 0)?;                    // dwDepth (3D only)
    write_u32(out, mipmap_levels)?;
    for _ in 0..11 { write_u32(out, 0)?; }

    // DDS_PIXELFORMAT — DX10 marker
    write_u32(out, 32)?;
    write_u32(out, DDPF_FOURCC)?;
    write_u32(out, fourcc(b"DX10"))?;
    write_u32(out, 0)?;
    write_u32(out, 0)?;
    write_u32(out, 0)?;
    write_u32(out, 0)?;
    write_u32(out, 0)?;

    write_u32(out, caps)?;
    write_u32(out, 0)?;                    // dwCaps2
    write_u32(out, 0)?;
    write_u32(out, 0)?;
    write_u32(out, 0)?;

    // DDS_HEADER_DXT10 (20 bytes)
    write_u32(out, dxgi_format(format))?;
    write_u32(out, D3D10_RESOURCE_DIMENSION_TEXTURE2D)?;
    write_u32(out, 0)?;                    // miscFlag (0; cubemap arrays would set bit 2)
    write_u32(out, layer_count)?;          // arraySize
    write_u32(out, 0)?;                    // miscFlags2 (alpha mode unknown)

    out.write_all(pixel_bytes)?;
    Ok(())
}

//================================================================================
// dxn_mono_alpha decompression
//================================================================================
//
// Mirrors TagTool's `BitmapCompression.DecompressDXNMonoAlpha`
// (`TagTool/Bitmaps/Utils/BitmapCompression.cs:419`). Each 4×4 block
// is two BC4-style sub-blocks back to back: the *red* sub-block
// carries luminance, the *green* sub-block carries alpha. We expand
// to A8R8G8B8 with `(R, R, R, A) = (luminance, luminance, luminance,
// alpha)` semantics.

/// Decode a stacked DxnMonoAlpha mip chain (optionally cubemap faces
/// or array layers) into A8R8G8B8 bytes. Output is laid out
/// layer-major to match the input.
pub fn decode_dxn_mono_alpha(
    input: &[u8],
    width: u32,
    height: u32,
    mipmap_levels: u32,
    layers: u32,
) -> Vec<u8> {
    let mut output = Vec::new();
    let mut input_offset = 0usize;

    for _layer in 0..layers {
        for level in 0..mipmap_levels {
            let w = (width >> level).max(1);
            let h = (height >> level).max(1);
            let blocks_w = ((w + 3) / 4).max(1);
            let blocks_h = ((h + 3) / 4).max(1);

            let mut mip_out = vec![0u8; (w as usize) * (h as usize) * 4];

            for by in 0..blocks_h {
                for bx in 0..blocks_w {
                    let block_offset = input_offset
                        + ((by * blocks_w + bx) as usize) * 16;
                    let block = &input[block_offset..block_offset + 16];

                    let mut red_values = [0u8; 8];
                    let red_indices = unpack_bc4_alpha_block(&block[0..8], &mut red_values);
                    let mut green_values = [0u8; 8];
                    let green_indices = unpack_bc4_alpha_block(&block[8..16], &mut green_values);

                    for j in 0..4u32 {
                        for i in 0..4u32 {
                            let px = bx * 4 + i;
                            let py = by * 4 + j;
                            if px >= w || py >= h { continue; }

                            let pixel_idx = ((py * w + px) as usize) * 4;
                            let bit_offset = 3 * (j * 4 + i);
                            let red_idx = ((red_indices >> bit_offset) & 0x07) as usize;
                            let green_idx = ((green_indices >> bit_offset) & 0x07) as usize;

                            let r = red_values[red_idx];
                            let g = green_values[green_idx];

                            // A8R8G8B8 LE byte layout: [B, G, R, A].
                            // Replicate red into RGB and put the green
                            // sub-block result into alpha — matches
                            // TagTool's DecompressDXNMonoAlpha.
                            mip_out[pixel_idx] = r;
                            mip_out[pixel_idx + 1] = r;
                            mip_out[pixel_idx + 2] = r;
                            mip_out[pixel_idx + 3] = g;
                        }
                    }
                }
            }

            output.extend_from_slice(&mip_out);
            input_offset += (blocks_w * blocks_h) as usize * 16;
        }
    }

    output
}

/// Decode one BC4-style alpha sub-block: 2 endpoint bytes + 6 bytes
/// of 3-bit indices. Fills `values[0..8]` with the 8-entry palette
/// (2 endpoints + 6 interpolated values, with the BC4 mode switch
/// based on whether `v0 > v1`). Returns the 48-bit index field
/// packed into a `u64`.
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
