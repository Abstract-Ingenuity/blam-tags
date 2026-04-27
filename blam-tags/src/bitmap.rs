//! Bitmap → DDS extraction support for `.bitmap` tag files.
//!
//! Two layers:
//!
//! 1. [`BitmapFormat`] — the subset of the schema's `bitmap_formats`
//!    enum that maps directly to a DDS pixelformat without a pixel
//!    decoder pass. Mapping coverage matches TagTool's
//!    `BitmapDdsFormatDetection` table plus the single-channel and
//!    16-bit-per-channel formats observed in halo3_mcc /
//!    haloreach_mcc bitmap corpora. Formats outside this set
//!    (`dxn_mono_alpha`, `ctx1`, etc.) need a pixel decoder and are
//!    surfaced via [`BitmapError::FormatNotSupported`].
//!
//! 2. [`Bitmap`] / [`BitmapImage`] — high-level accessors over a
//!    parsed [`TagFile`], wrapping the `bitmaps[]` block + the
//!    top-level `processed pixel data` blob. Each image carries the
//!    metadata (format, dimensions, mipmap levels, type) plus a
//!    sliced view of its raw pixel bytes.
//!
//! Block-compressed math: BC formats store one block (8 or 16 bytes)
//! per 4×4 pixel region. Mips smaller than 4×4 still cost one block.
//! See [`BitmapFormat::level_bytes`].
//!
//! The on-disk pipeline for halo3_mcc / haloreach_mcc bitmaps is
//! "everything inline" — every observed `.bitmap` carries non-empty
//! `processed pixel data` and no populated `hardware textures`
//! resource block. This module is built around that case; cache-paged
//! bitmaps would need a different code path (out of scope here).

use std::io::Write;

use crate::api::{TagBlock, TagStruct};
use crate::file::TagFile;

/// A bitmap pixel format that wraps in DDS without transforming the
/// pixel bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitmapFormat {
    A8, Y8, R8, Ay8, A8y8,
    A4r4g4b4, X8r8g8b8, A8r8g8b8,
    Dxt1, Dxt3, Dxt5, Dxt5a, Dxn,
    V8u8, Q8w8v8u8,
    Abgrfp16, Abgrfp32,
    A16b16g16r16, Signedr16g16b16a16,
    /// Halo-specific: BC5-shaped (16 bytes/4×4 block) but the two
    /// sub-blocks are *luminance* (red half) and *alpha* (green
    /// half). Decodes to A8R8G8B8 with `(R, R, R, A)` semantics —
    /// no DDS reader does this swizzle automatically, so we run the
    /// decoder before writing.
    DxnMonoAlpha,
}

impl BitmapFormat {
    /// Resolve the schema's `bitmap_formats` enum option name. Names
    /// match those in `definitions/<game>/bitmap.json` and are stable
    /// across halo3_mcc and haloreach_mcc (the integer indices shift
    /// but the strings are the same).
    pub fn from_schema_name(name: &str) -> Option<Self> {
        Some(match name {
            "a8" => Self::A8,
            "y8" => Self::Y8,
            "r8" => Self::R8,
            "ay8" => Self::Ay8,
            "a8y8" => Self::A8y8,
            "a4r4g4b4" => Self::A4r4g4b4,
            "x8r8g8b8" => Self::X8r8g8b8,
            "a8r8g8b8" => Self::A8r8g8b8,
            "dxt1" => Self::Dxt1,
            "dxt3" => Self::Dxt3,
            "dxt5" => Self::Dxt5,
            "dxt5a" => Self::Dxt5a,
            "dxn" => Self::Dxn,
            "v8u8" => Self::V8u8,
            "q8w8v8u8" => Self::Q8w8v8u8,
            "abgrfp16" => Self::Abgrfp16,
            "abgrfp32" => Self::Abgrfp32,
            "a16b16g16r16" => Self::A16b16g16r16,
            "signedr16g16b16a16" => Self::Signedr16g16b16a16,
            "dxn_mono_alpha" => Self::DxnMonoAlpha,
            _ => return None,
        })
    }

    /// Whether this format stores pixels as 4×4 blocks (BC1/2/3/5
    /// family) rather than per-pixel.
    pub fn is_compressed(self) -> bool {
        matches!(self,
            Self::Dxt1 | Self::Dxt3 | Self::Dxt5 | Self::Dxt5a | Self::Dxn
            | Self::DxnMonoAlpha
        )
    }

    /// Whether the format only has a clean DDS expression via the
    /// DXT10 extension header (no legacy fourcc / pixelformat that
    /// most readers will round-trip correctly). Currently just
    /// `signedr16g16b16a16` — `DXGI_FORMAT_R16G16B16A16_SNORM` has no
    /// pre-DX10 D3D9 fourcc.
    pub fn requires_dxt10(self) -> bool {
        matches!(self, Self::Signedr16g16b16a16)
    }

    /// Bytes per stored block for compressed formats. 8 for BC1 /
    /// BC4, 16 for BC2 / BC3 / BC5 / DxnMonoAlpha. 0 for
    /// uncompressed formats.
    pub fn block_bytes(self) -> u32 {
        match self {
            Self::Dxt1 | Self::Dxt5a => 8,
            Self::Dxt3 | Self::Dxt5 | Self::Dxn | Self::DxnMonoAlpha => 16,
            _ => 0,
        }
    }

    /// Bytes per pixel for uncompressed formats. 0 for compressed.
    pub fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::A8 | Self::Y8 | Self::R8 | Self::Ay8 => 1,
            Self::A8y8 | Self::A4r4g4b4 | Self::V8u8 => 2,
            Self::X8r8g8b8 | Self::A8r8g8b8 | Self::Q8w8v8u8 | Self::Abgrfp16 => 4,
            Self::A16b16g16r16 | Self::Signedr16g16b16a16 => 8,
            Self::Abgrfp32 => 16,
            _ => 0,
        }
    }

    /// Bytes consumed by one mip level at `(width, height)`. For
    /// compressed formats, dimensions round up to the 4×4 block grid
    /// with a 1-block minimum so 1×1 / 2×2 mips cost one block.
    pub fn level_bytes(self, width: u32, height: u32) -> u64 {
        if self.is_compressed() {
            let blocks_w = ((width + 3) / 4).max(1);
            let blocks_h = ((height + 3) / 4).max(1);
            blocks_w as u64 * blocks_h as u64 * self.block_bytes() as u64
        } else {
            width as u64 * height as u64 * self.bytes_per_pixel() as u64
        }
    }

    /// Total pixel bytes for one full mipmap chain at the given base
    /// dimensions. `mipmap_levels` is the total level count (= the
    /// schema's `mipmap count + 1` since that field excludes the base
    /// level).
    pub fn surface_bytes(self, width: u32, height: u32, mipmap_levels: u32) -> u64 {
        (0..mipmap_levels)
            .map(|i| self.level_bytes((width >> i).max(1), (height >> i).max(1)))
            .sum()
    }
}

/// Errors returned by the bitmap walkers and DDS writer.
#[derive(Debug)]
pub enum BitmapError {
    /// Root struct doesn't expose the fields we expect from a
    /// `.bitmap` tag (no `bitmaps` block or no `processed pixel data`).
    NotABitmapTag,
    /// `format` enum value resolved to a name that isn't in
    /// [`BitmapFormat::from_schema_name`]. Carries the name so callers
    /// can report which format failed.
    FormatNotSupported(String),
    /// `bitmaps[i].pixels offset/size` walks past the end of the
    /// `processed pixel data` blob.
    PixelSliceOutOfBounds { offset: u64, size: u64, available: u64 },
    /// Bitmap type isn't 2D / cube / array — 3D textures aren't
    /// modeled yet.
    UnsupportedTextureType(String),
    Io(std::io::Error),
}

impl std::fmt::Display for BitmapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotABitmapTag => write!(f, "tag is not a recognizable bitmap (missing `bitmaps` or `processed pixel data`)"),
            Self::FormatNotSupported(name) => write!(f, "bitmap format `{name}` is not directly DDS-mappable (needs decoder)"),
            Self::PixelSliceOutOfBounds { offset, size, available } =>
                write!(f, "pixels slice [{offset}..{}] exceeds processed pixel data ({available} bytes)", offset + size),
            Self::UnsupportedTextureType(name) => write!(f, "bitmap type `{name}` is not supported (only `2D texture`, `cube map`, and `array`)"),
            Self::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for BitmapError {}

impl From<std::io::Error> for BitmapError {
    fn from(value: std::io::Error) -> Self { Self::Io(value) }
}

/// High-level view of a `.bitmap` tag — the `bitmaps[]` block + the
/// shared `processed pixel data` blob. Construct from a parsed
/// [`TagFile`] via [`Bitmap::new`].
pub struct Bitmap<'a> {
    pixels: &'a [u8],
    bitmaps: TagBlock<'a>,
}

impl<'a> Bitmap<'a> {
    /// Wrap a parsed `.bitmap` tag. Errors with [`BitmapError::NotABitmapTag`]
    /// if the tag doesn't expose `bitmaps[]` and `processed pixel data`.
    pub fn new(tag: &'a TagFile) -> Result<Self, BitmapError> {
        let root = tag.root();

        let pixels = root
            .field_path("processed pixel data")
            .and_then(|f| f.as_data())
            .ok_or(BitmapError::NotABitmapTag)?;

        let bitmaps = root
            .field_path("bitmaps")
            .and_then(|f| f.as_block())
            .ok_or(BitmapError::NotABitmapTag)?;

        Ok(Self { pixels, bitmaps })
    }

    /// Number of images in the tag's `bitmaps[]` block.
    pub fn len(&self) -> usize { self.bitmaps.len() }

    /// `true` if this tag has no images.
    pub fn is_empty(&self) -> bool { self.bitmaps.is_empty() }

    /// Get the image at `index`, or `None` if out of range.
    pub fn image(&self, index: usize) -> Option<BitmapImage<'a>> {
        let elem = self.bitmaps.element(index)?;
        Some(BitmapImage { elem, pixels: self.pixels })
    }

    /// Iterate every image in declaration order.
    pub fn iter(&self) -> impl Iterator<Item = BitmapImage<'a>> + '_ {
        let pixels = self.pixels;
        self.bitmaps.iter().map(move |elem| BitmapImage { elem, pixels })
    }
}

/// One element of `bitmaps[]` — metadata plus the slice of
/// `processed pixel data` it owns.
#[derive(Clone, Copy)]
pub struct BitmapImage<'a> {
    elem: TagStruct<'a>,
    pixels: &'a [u8],
}

impl<'a> BitmapImage<'a> {
    /// Base-mip width in pixels.
    pub fn width(&self) -> u32 { self.elem.read_int_any("width").unwrap_or(0).max(0) as u32 }

    /// Base-mip height in pixels.
    pub fn height(&self) -> u32 { self.elem.read_int_any("height").unwrap_or(0).max(0) as u32 }

    /// 3D-texture depth (or array layer count for arrays). Always `>= 1`.
    pub fn depth(&self) -> u32 { self.elem.read_int_any("depth").unwrap_or(1).max(1) as u32 }

    /// Total mipmap levels including the base level. The schema's
    /// `mipmap count` excludes the base, so this is `that + 1`.
    pub fn mipmap_levels(&self) -> u32 {
        self.elem.read_int_any("mipmap count").unwrap_or(0).max(0) as u32 + 1
    }

    /// The schema's per-image format name (e.g. `"dxt5"`, `"dxn"`).
    /// h3 uses `short_enum` while reach uses `char_enum`; both map the
    /// same way through the wider enum-name reader.
    pub fn format_name(&self) -> Option<String> {
        self.elem.read_enum_name("format")
    }

    /// The format mapped to a known [`BitmapFormat`], if supported.
    pub fn format(&self) -> Result<BitmapFormat, BitmapError> {
        let name = self.format_name().ok_or(BitmapError::NotABitmapTag)?;
        BitmapFormat::from_schema_name(&name)
            .ok_or(BitmapError::FormatNotSupported(name))
    }

    /// The schema's per-image type name (e.g. `"2D texture"`, `"cube map"`).
    pub fn type_name(&self) -> Option<String> {
        self.elem.read_enum_name("type")
    }

    /// `true` for cube map textures (six 2D surfaces under one image).
    pub fn is_cube(&self) -> bool {
        matches!(self.type_name().as_deref(), Some("cube map"))
    }

    /// `true` for texture arrays (`depth` layers of 2D surfaces).
    pub fn is_array(&self) -> bool {
        matches!(self.type_name().as_deref(), Some("array"))
    }

    /// Number of stacked surfaces beyond the base mip chain:
    /// - 2D: 1
    /// - cube map: 6 (faces)
    /// - array: `depth` (layer count)
    pub fn layer_count(&self) -> u32 {
        if self.is_cube() {
            6
        } else if self.is_array() {
            self.depth()
        } else {
            1
        }
    }

    /// Byte slice into `processed pixel data` covering this image's
    /// full mipmap chain across all faces / layers.
    ///
    /// `pixels offset` is taken from the tag — it's the authoritative
    /// position for multi-image slicing. The slice *length* is
    /// recomputed from format + dimensions + mipmap count + layer
    /// count rather than `pixels size`, because the size field is
    /// sometimes stale on MCC tags (e.g. inflated by 2× from an
    /// original split-resource layout).
    pub fn pixel_bytes(&self) -> Result<&'a [u8], BitmapError> {
        let offset = self.elem.read_int_any("pixels offset").unwrap_or(0).max(0) as usize;
        let format = self.format()?;
        let layers = self.layer_count() as u64;
        let expected = format.surface_bytes(self.width(), self.height(), self.mipmap_levels())
            .saturating_mul(layers) as usize;

        let end = offset.saturating_add(expected);
        if end > self.pixels.len() {
            return Err(BitmapError::PixelSliceOutOfBounds {
                offset: offset as u64,
                size: expected as u64,
                available: self.pixels.len() as u64,
            });
        }
        Ok(&self.pixels[offset..end])
    }

    /// Write a DDS file representing this image. Picks the legacy
    /// DDS pixelformat when possible; falls back to the DXT10
    /// extension when the format or type requires it (texture
    /// arrays, or formats with no legacy fourcc / pixelformat).
    /// Halo-specific decoder-only formats (`dxn_mono_alpha`) are
    /// decompressed to A8R8G8B8 first, then routed through the
    /// normal DDS writer.
    pub fn write_dds(&self, out: &mut impl Write) -> Result<(), BitmapError> {
        let format = self.format()?;
        let type_name = self.type_name();
        let is_cube = self.is_cube();
        let is_array = self.is_array();
        // 3D textures aren't observed in either corpus and the
        // layer / depth semantics need a different writer path —
        // refuse for now.
        if !is_cube && !is_array && !matches!(type_name.as_deref(), Some("2D texture") | None) {
            return Err(BitmapError::UnsupportedTextureType(type_name.unwrap_or_default()));
        }
        let bytes = self.pixel_bytes()?;

        // Halo-specific BC5-shaped layout that needs CPU
        // decompression to be readable. Decode to A8R8G8B8 and
        // dispatch as that.
        if format == BitmapFormat::DxnMonoAlpha {
            let decoded = decode_dxn_mono_alpha(
                bytes, self.width(), self.height(),
                self.mipmap_levels(), self.layer_count(),
            );
            return self.write_dds_with_format(
                out, BitmapFormat::A8r8g8b8, &decoded, is_cube, is_array,
            );
        }

        self.write_dds_with_format(out, format, bytes, is_cube, is_array)
    }

    fn write_dds_with_format(
        &self,
        out: &mut impl Write,
        format: BitmapFormat,
        bytes: &[u8],
        is_cube: bool,
        is_array: bool,
    ) -> Result<(), BitmapError> {
        let needs_dx10 = is_array || format.requires_dxt10();
        if needs_dx10 {
            // Cube + DXT10 uses miscFlag bit 2 + arraySize=6 — no
            // observed corpus images need it, so we punt for now.
            if is_cube {
                return Err(BitmapError::UnsupportedTextureType(format!(
                    "cube map of `{:?}` requires DXT10 cube + array support", format
                )));
            }
            let layers = if is_array { self.layer_count() } else { 1 };
            write_dds_dx10(out, format, self.width(), self.height(), self.mipmap_levels(), layers, bytes)?;
        } else {
            write_dds(out, format, self.width(), self.height(), self.mipmap_levels(), is_cube, bytes)?;
        }
        Ok(())
    }
}

//================================================================================
// DDS header construction
//================================================================================

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
