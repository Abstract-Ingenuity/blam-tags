//! Halo `mapping_function` (a.k.a. `c_function_definition`) decoder
//! and evaluator.
//!
//! TagFunction is a compact byte-blob curve descriptor used pervasively
//! in Halo tags — material parameter values, animated UVs, particle
//! per-frame properties, weapon firing rates, light fades, scenario
//! interpolators. The same 32-byte header + variable-length per-type
//! compact data appears in `render_method_animated_parameter_block`,
//! particle/beam/contrail/light_volume/decal systems, lens_flare,
//! camera_fx_settings, and others.
//!
//! ## 32-byte header
//!
//! Mirrors `s_function_definition_internal` from Ares
//! `source/math/function_definitions.cpp:46-71`:
//!
//! ```text
//! byte 0:    function_type (enum 0..10)
//! byte 1:    flags (range/cyclic/clamped/exclusion/optimized/gpu)
//! byte 2:    color_graph_type (0=scalar, 1..4 = N-color)
//! byte 3:    unused
//! bytes 4-19: union { colors[4] | clamp_range_min/max | constants[2] }
//! bytes 20-23: exclusion_min
//! bytes 24-27: exclusion_max
//! bytes 28-31: size_of_compact_data (bytes after header)
//! ```
//!
//! ## Eval pipeline
//!
//! `evaluate(input, range)` → `evaluate_legacy` then
//! `map_to_output_range_legacy`. Type-specific normalized output
//! (typically [0, 1]) gets linearly mapped through
//! `[clamp_range_min, clamp_range_max]`.
//!
//! For type 1 (constant), the unranged variant returns 0.0 from
//! `evaluate_legacy`, so the operative value lives in
//! `clamp_range_min` (header bytes 4-7). When ranged, `evaluate_legacy`
//! returns the `range` argument verbatim and the output is mapped
//! through clamp_range like any other curve.
//!
//! ## Phase 1 scope
//!
//! Header parser + Identity + Constant variants (which together cover
//! ~95% of static material parameters in shipped tags). Other types
//! parse to `Unsupported { header, raw }` and `evaluate` returns 0.
//! Caller can detect via `function_type()`. Phases 2-7 add coverage
//! for transition/periodic/linear/exp/spline/multi-part/color paths.

use crate::math::RealRgbColor;

/// 11 function types defined in Halo's `e_function_type` enum
/// (Ares `function_definitions.h:26-41`).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionType {
    Identity        = 0,
    Constant        = 1,
    Transition      = 2,
    Periodic        = 3,
    Linear          = 4,
    LinearKey       = 5,
    MultiLinearKey  = 6,
    Spline          = 7,
    /// Also called `multi_part` in the engine — same enum value.
    MultiSpline     = 8,
    Exponent        = 9,
    Spline2         = 10,
}

impl FunctionType {
    pub fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::Identity,
            1 => Self::Constant,
            2 => Self::Transition,
            3 => Self::Periodic,
            4 => Self::Linear,
            5 => Self::LinearKey,
            6 => Self::MultiLinearKey,
            7 => Self::Spline,
            8 => Self::MultiSpline,
            9 => Self::Exponent,
            10 => Self::Spline2,
            _ => return None,
        })
    }
}

/// Flag bits at byte 1 of the header. From the `_function_flag_*_bit`
/// enum in Ares `function_definitions.cpp:33-42`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(transparent)]
pub struct FunctionFlags(pub u8);

impl FunctionFlags {
    pub const RANGE: u8     = 1 << 0;
    pub const CYCLIC: u8    = 1 << 1;
    pub const CLAMPED: u8   = 1 << 2;
    pub const EXCLUSION: u8 = 1 << 3;
    pub const OPTIMIZED: u8 = 1 << 4;
    pub const GPU: u8       = 1 << 5;

    pub fn is_ranged(self)    -> bool { (self.0 & Self::RANGE)     != 0 }
    pub fn is_cyclic(self)    -> bool { (self.0 & Self::CYCLIC)    != 0 }
    pub fn is_clamped(self)   -> bool { (self.0 & Self::CLAMPED)   != 0 }
    pub fn has_exclusion(self) -> bool { (self.0 & Self::EXCLUSION) != 0 }
    pub fn is_optimized(self) -> bool { (self.0 & Self::OPTIMIZED) != 0 }
    pub fn is_gpu(self)       -> bool { (self.0 & Self::GPU)       != 0 }
}

/// `e_color_graph_type` — selects scalar vs N-color output.
/// When non-Scalar, the union at bytes 4-19 holds 4 ARGB u32s instead
/// of clamp_range floats.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorGraphType {
    Scalar     = 0,
    OneColor   = 1,
    TwoColor   = 2,
    ThreeColor = 3,
    FourColor  = 4,
}

impl ColorGraphType {
    pub fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::Scalar,
            1 => Self::OneColor,
            2 => Self::TwoColor,
            3 => Self::ThreeColor,
            4 => Self::FourColor,
            _ => return None,
        })
    }
}

/// Parsed 32-byte header. The `clamp_range_*` fields and `colors`
/// alias the same memory in the source struct (they're a union); both
/// are read so callers can use whichever interpretation matches the
/// `color_graph_type`.
#[derive(Debug, Clone)]
pub struct TagFunctionHeader {
    pub function_type: FunctionType,
    pub flags: FunctionFlags,
    pub color_graph_type: ColorGraphType,
    /// Bytes 4-7 (f32 LE). For scalar functions: lower bound of the
    /// output range. For constant type unranged: the operative value.
    pub clamp_range_min: f32,
    /// Bytes 8-11 (f32 LE). Upper bound of output range.
    pub clamp_range_max: f32,
    /// Bytes 4-19 (4× u32 LE). For color functions, ARGB-packed colors.
    /// `color_graph_type` says how many entries are populated.
    pub colors: [u32; 4],
    pub exclusion_min: f32,
    pub exclusion_max: f32,
    /// Bytes after the 32-byte header that belong to the per-type
    /// compact data block. Type-specific structures live here.
    pub compact_size: i32,
}

impl TagFunctionHeader {
    pub fn parse(data: &[u8]) -> Result<Self, TagFunctionError> {
        if data.len() < 32 {
            return Err(TagFunctionError::TooShort { len: data.len() });
        }
        let function_type = FunctionType::from_byte(data[0])
            .ok_or(TagFunctionError::UnknownFunctionType { byte: data[0] })?;
        let flags = FunctionFlags(data[1]);
        let color_graph_type = ColorGraphType::from_byte(data[2])
            .ok_or(TagFunctionError::UnknownColorGraphType { byte: data[2] })?;
        let clamp_range_min = f32::from_le_bytes(data[4..8].try_into().unwrap());
        let clamp_range_max = f32::from_le_bytes(data[8..12].try_into().unwrap());
        let colors = [
            u32::from_le_bytes(data[4..8].try_into().unwrap()),
            u32::from_le_bytes(data[8..12].try_into().unwrap()),
            u32::from_le_bytes(data[12..16].try_into().unwrap()),
            u32::from_le_bytes(data[16..20].try_into().unwrap()),
        ];
        let exclusion_min = f32::from_le_bytes(data[20..24].try_into().unwrap());
        let exclusion_max = f32::from_le_bytes(data[24..28].try_into().unwrap());
        let compact_size = i32::from_le_bytes(data[28..32].try_into().unwrap());
        Ok(Self {
            function_type, flags, color_graph_type,
            clamp_range_min, clamp_range_max, colors,
            exclusion_min, exclusion_max, compact_size,
        })
    }
}

/// Decoded TagFunction. Phase 1 implements Identity + Constant
/// fully; other types parse the header and stash the raw remaining
/// bytes for later phases.
#[derive(Debug, Clone)]
pub enum TagFunction {
    Identity { header: TagFunctionHeader },
    Constant { header: TagFunctionHeader },
    /// Function type recognized but not yet implemented. `evaluate`
    /// returns 0.0; `as_constant()` returns None. Caller can detect
    /// via `function_type()` and choose to fall back / error.
    Unsupported { header: TagFunctionHeader, raw: Vec<u8> },
}

#[derive(Debug)]
pub enum TagFunctionError {
    TooShort { len: usize },
    UnknownFunctionType { byte: u8 },
    UnknownColorGraphType { byte: u8 },
}

impl std::fmt::Display for TagFunctionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort { len } => write!(f, "TagFunction data too short: {len} bytes (need 32)"),
            Self::UnknownFunctionType { byte } => write!(f, "unknown function_type byte: 0x{byte:02x}"),
            Self::UnknownColorGraphType { byte } => write!(f, "unknown color_graph_type byte: 0x{byte:02x}"),
        }
    }
}

impl std::error::Error for TagFunctionError {}

impl TagFunction {
    /// Parse a `mapping_function` `data` blob. The slice should be
    /// the raw bytes from the schema's `data` field; we read 32
    /// bytes for the header and stash the rest for phase-2+ decoders.
    pub fn parse(data: &[u8]) -> Result<Self, TagFunctionError> {
        let header = TagFunctionHeader::parse(data)?;
        Ok(match header.function_type {
            FunctionType::Identity => Self::Identity { header },
            FunctionType::Constant => Self::Constant { header },
            _ => Self::Unsupported {
                header,
                raw: data.to_vec(),
            },
        })
    }

    pub fn header(&self) -> &TagFunctionHeader {
        match self {
            Self::Identity { header }
            | Self::Constant { header }
            | Self::Unsupported { header, .. } => header,
        }
    }

    pub fn function_type(&self)    -> FunctionType    { self.header().function_type }
    pub fn flags(&self)            -> FunctionFlags   { self.header().flags }
    pub fn color_graph_type(&self) -> ColorGraphType  { self.header().color_graph_type }

    /// Evaluate the function at `(input, range)` returning a scalar.
    /// Mirrors `c_function_definition::evaluate_scalar` — calls
    /// `evaluate_legacy` to get a normalized output, then maps through
    /// `[clamp_range_min, clamp_range_max]`.
    ///
    /// For unsupported (not-yet-decoded) types returns 0.0 — callers
    /// who need a fallback should check `function_type()` first or
    /// use `as_constant()` on the constant path.
    pub fn evaluate(&self, input: f32, range: f32) -> f32 {
        let normalized = self.evaluate_legacy(input, range);
        self.map_to_output_range(normalized)
    }

    /// The "normalized" curve output before output-range remapping.
    /// Ports `c_function_definition::evaluate_legacy` for the types
    /// implemented so far.
    fn evaluate_legacy(&self, input: f32, range: f32) -> f32 {
        match self {
            Self::Identity { .. } => input,
            Self::Constant { header } => {
                if header.flags.is_ranged() { range } else { 0.0 }
            }
            Self::Unsupported { .. } => 0.0,
        }
    }

    /// Linearly map a normalized output through `[clamp_range_min,
    /// clamp_range_max]`. Mirrors
    /// `c_function_definition::map_to_output_range_legacy`.
    fn map_to_output_range(&self, normalized: f32) -> f32 {
        let h = self.header();
        h.clamp_range_min + normalized * (h.clamp_range_max - h.clamp_range_min)
    }

    /// Fast path for the common case: callers that just want a single
    /// scalar value for a constant material parameter. Returns `Some`
    /// for constant (and unranged identity) functions, `None` for
    /// anything that genuinely depends on input.
    pub fn as_constant(&self) -> Option<f32> {
        match self {
            Self::Constant { header } if !header.flags.is_ranged() => {
                Some(header.clamp_range_min)
            }
            _ => None,
        }
    }

    /// True if the function is constant for all inputs (used as a
    /// fast-skip hint when building per-frame uniforms). Includes
    /// unranged constant; identity is NOT constant since it varies
    /// with input.
    pub fn is_constant(&self) -> bool {
        self.as_constant().is_some()
    }

    /// Color-output evaluator. Phase 6 will implement the actual
    /// gradient interpolation using the `m_colors[4]` block; Phase 1
    /// returns white as a stub so callers can wire the API now.
    pub fn evaluate_color(&self, _input: f32, _range: f32) -> RealRgbColor {
        RealRgbColor { red: 1.0, green: 1.0, blue: 1.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// grunt_armor.shader parameters[3] (`diffuse_coefficient`):
    /// constant function, value 1.0.
    const DIFFUSE_COEFFICIENT: [u8; 32] = [
        0x01, 0x20, 0x00, 0x00,             // type=Constant, flags=GPU, color=Scalar
        0x00, 0x00, 0x80, 0x3f,             // clamp_range_min = 1.0
        0x00, 0x00, 0x80, 0x3f,             // clamp_range_max = 1.0
        0, 0, 0, 0, 0, 0, 0, 0,             // colors[2..3] = 0
        0, 0, 0, 0, 0, 0, 0, 0,             // exclusion_min/max = 0
        0, 0, 0, 0,                          // compact_size = 0
    ];

    /// grunt_armor.shader parameters[4] (`specular_coefficient`):
    /// unranged constant, min=1.0, max=0.318...
    /// The operative value for an unranged constant is min (1.0).
    const SPECULAR_COEFFICIENT: [u8; 32] = [
        0x01, 0x20, 0x00, 0x00,
        0x00, 0x00, 0x80, 0x3f,             // 1.0
        0x83, 0xf9, 0xa2, 0x3e,             // 0.31831 (1/π) — max field
        0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0,
    ];

    /// grunt_armor.shader parameters[6] (`roughness`):
    /// unranged constant, min=0.2, max=1.0.
    const ROUGHNESS: [u8; 32] = [
        0x01, 0x20, 0x00, 0x00,
        0xcd, 0xcc, 0x4c, 0x3e,             // 0.2
        0x00, 0x00, 0x80, 0x3f,             // 1.0
        0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0,
    ];

    #[test]
    fn parses_constant_diffuse_coefficient() {
        let f = TagFunction::parse(&DIFFUSE_COEFFICIENT).unwrap();
        assert_eq!(f.function_type(), FunctionType::Constant);
        assert!(f.flags().is_gpu());
        assert!(!f.flags().is_ranged());
        assert_eq!(f.color_graph_type(), ColorGraphType::Scalar);
        assert_eq!(f.as_constant(), Some(1.0));
        assert_eq!(f.evaluate(0.0, 0.0), 1.0);
        assert_eq!(f.evaluate(1.0, 1.0), 1.0);
    }

    #[test]
    fn unranged_constant_uses_min_not_max() {
        let f = TagFunction::parse(&SPECULAR_COEFFICIENT).unwrap();
        // The operative value for an UNRANGED constant function is
        // clamp_range_min (bytes 4-7), NOT max (bytes 8-11).
        // evaluate_legacy returns 0.0 unranged → maps to min.
        assert_eq!(f.as_constant(), Some(1.0));
        assert_eq!(f.evaluate(0.0, 0.0), 1.0);
        assert_eq!(f.evaluate(123.4, 56.7), 1.0);
    }

    #[test]
    fn roughness_returns_min() {
        let f = TagFunction::parse(&ROUGHNESS).unwrap();
        assert!((f.as_constant().unwrap() - 0.2).abs() < 1e-6);
        assert!((f.evaluate(0.5, 0.5) - 0.2).abs() < 1e-6);
    }

    #[test]
    fn parses_identity() {
        let mut bytes = [0u8; 32];
        bytes[0] = 0x00; // function_type = Identity
        // clamp_range maps normalized → output
        bytes[4..8].copy_from_slice(&0.0f32.to_le_bytes());     // min
        bytes[8..12].copy_from_slice(&10.0f32.to_le_bytes());   // max
        let f = TagFunction::parse(&bytes).unwrap();
        assert_eq!(f.function_type(), FunctionType::Identity);
        assert_eq!(f.as_constant(), None);
        // identity returns input as normalized → mapped through [0, 10]
        assert!((f.evaluate(0.5, 0.0) - 5.0).abs() < 1e-6);
        assert!((f.evaluate(1.0, 0.0) - 10.0).abs() < 1e-6);
    }

    #[test]
    fn unsupported_type_evaluates_to_min() {
        let mut bytes = [0u8; 32];
        bytes[0] = 0x03; // Periodic — Phase 3
        bytes[4..8].copy_from_slice(&5.0f32.to_le_bytes());
        bytes[8..12].copy_from_slice(&7.0f32.to_le_bytes());
        let f = TagFunction::parse(&bytes).unwrap();
        assert_eq!(f.function_type(), FunctionType::Periodic);
        // Unsupported normalized = 0 → maps to min
        assert_eq!(f.evaluate(0.0, 0.0), 5.0);
        assert!(f.as_constant().is_none());
    }

    #[test]
    fn rejects_short_data() {
        assert!(matches!(
            TagFunction::parse(&[0u8; 31]),
            Err(TagFunctionError::TooShort { len: 31 })
        ));
    }

    #[test]
    fn rejects_unknown_function_type() {
        let mut bytes = [0u8; 32];
        bytes[0] = 0xff;
        assert!(matches!(
            TagFunction::parse(&bytes),
            Err(TagFunctionError::UnknownFunctionType { byte: 0xff })
        ));
    }
}
