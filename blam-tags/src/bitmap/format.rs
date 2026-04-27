//! Bitmap format identifiers and associated metadata.
//!
//! [`BitmapFormat`] is the subset of the schema's `bitmap_formats`
//! enum we currently support — the formats observed in halo3_mcc /
//! haloreach_mcc bitmap corpora plus the legacy `signedr16g16b16a16`
//! and `dxn_mono_alpha` extensions. Schema variants outside this set
//! (`r5g6b5`, `a1r5g5b5`, `software rgbfp32`, `g8b8`,
//! `a2r10g10b10`, `v16u16`, `ctx1`, the `dxt3a*` family) are not
//! observed in MCC content and aren't modeled here yet.
//!
//! [`BitmapCurve`] is the canonical 6-value `e_bitmap_curve` enum
//! verified against H3 source `bitmap_curve.h`. Maps to the schema's
//! `bitmap_curve_enum` by index — the schema strings are display
//! names with awkward curly-brace alts, so we read the enum by index
//! rather than by string.

/// A bitmap pixel format. Members map 1:1 to schema enum names; the
/// integer indices used at the bytes-on-disk layer differ between
/// halo3_mcc and haloreach_mcc but the names are stable.
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
    /// across halo3_mcc and haloreach_mcc.
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

    /// Whether channel values are stored as signed integers and need
    /// a `+128` (or `+32768` for 16-bit) bias to display as unsigned.
    /// Engine's `extract_debug_plate_copy` applies `s*(1/256)+0.5`
    /// for 8-bit signed — equivalent to integer `+128`.
    pub fn is_signed(self) -> bool {
        matches!(self, Self::V8u8 | Self::Q8w8v8u8 | Self::Signedr16g16b16a16)
    }

    /// Whether the format stores HDR float values that exceed the
    /// `[0, 1]` range a typical 8-bit display can carry. Decoding to
    /// RGBA8 requires a tone-map or clamp; emitting a float TIFF
    /// preserves the dynamic range.
    pub fn is_hdr(self) -> bool {
        matches!(self, Self::Abgrfp16 | Self::Abgrfp32)
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
    /// Matches the canonical 40-entry bits-per-pixel table read from
    /// MCC ManagedBlam.dll at `0x180C99010` — verified
    /// `abgrfp16 = 64 bits` (4 half-floats), not 32.
    pub fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::A8 | Self::Y8 | Self::R8 | Self::Ay8 => 1,
            Self::A8y8 | Self::A4r4g4b4 | Self::V8u8 => 2,
            Self::X8r8g8b8 | Self::A8r8g8b8 | Self::Q8w8v8u8 => 4,
            Self::A16b16g16r16 | Self::Signedr16g16b16a16 | Self::Abgrfp16 => 8,
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

/// Per-image gamma curve. Mirrors `e_bitmap_curve` from H3 source
/// (`bitmap_curve.h`) — same enumeration order as the schema's
/// `bitmap_curve_enum`. The schema's display strings are noisy
/// (e.g. `"xRGB (gamma about 2.0){SRGB (gamma 2.2)}"` for `XrgbGamma2`),
/// so callers should read the underlying integer rather than the
/// resolved string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitmapCurve {
    Unknown = 0,
    /// xRGB on Xenon, sRGB-equivalent (~γ 2.2) on PC.
    XrgbGamma2 = 1,
    Gamma2 = 2,
    Linear = 3,
    OffsetLog = 4,
    Srgb = 5,
}

impl BitmapCurve {
    /// Map an integer curve index (as stored in the tag) to the enum.
    /// Returns `Unknown` for out-of-range values rather than failing —
    /// the engine treats unknown curves as "use default" anyway.
    pub fn from_index(index: u8) -> Self {
        match index {
            1 => Self::XrgbGamma2,
            2 => Self::Gamma2,
            3 => Self::Linear,
            4 => Self::OffsetLog,
            5 => Self::Srgb,
            _ => Self::Unknown,
        }
    }
}
