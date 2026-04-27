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

pub mod dds;
pub mod decode;
pub mod format;
pub mod layout;
pub mod tiff;

pub use format::{BitmapCurve, BitmapFormat};

use dds::{decode_dxn_mono_alpha, write_dds, write_dds_dx10};

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
    /// Phase 2 TIFF writer is 2D-only; cube / array / 3D layouts
    /// land in Phase 4.
    TiffLayoutDeferred(String),
    /// Underlying `tiff` crate failure (carries the error string —
    /// we don't expose the crate's error enum publicly to keep our
    /// API surface stable across `tiff` versions).
    Tiff(String),
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
            Self::TiffLayoutDeferred(kind) => write!(f, "TIFF emission for bitmap type `{kind}` is deferred to Phase 4 (cube cross / array strip layouts)"),
            Self::Tiff(msg) => write!(f, "tiff error: {msg}"),
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

    /// The bitmap's gamma curve. Defaults to [`BitmapCurve::Unknown`]
    /// when the field is missing or out of range — the engine treats
    /// "unknown" as "use default for this format/usage" anyway.
    pub fn curve(&self) -> BitmapCurve {
        let raw = self.elem.read_int_any("curve").unwrap_or(0).max(0) as u8;
        BitmapCurve::from_index(raw)
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

    /// Write this image as a Tool-importable RGBA8 TIFF (Phase 2:
    /// 2D textures only, mip 0). Decodes the source format to
    /// straight-RGBA8 first; HDR formats are clamped to `[0, 1]`,
    /// signed-normalmap formats are biased into the unsigned range.
    /// Cube / array / 3D bitmaps and BC-compressed formats return
    /// the appropriate "deferred" / "unsupported" error.
    pub fn write_tiff(&self, out: &mut impl Write) -> Result<(), BitmapError> {
        tiff::write_image_tiff(self, out)
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
