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
//! ## Coverage status
//!
//! All 11 function types parse + evaluate scalar. Direct-formula types
//! (Linear, Spline, Spline2, Exponent) port from the engine's
//! pseudocode-commented `c_*_function_compact::evaluate` methods (Ares
//! `function_definitions.cpp` 800-1180). Compound types (LinearKey,
//! MultiPart) walk their compact-data graphs. Cyclic helpers
//! (`periodic_function_evaluate`, `transition_function_evaluate`)
//! reproduce the engine's analytic curve definitions directly rather
//! than via the engine's pre-baked 1024-byte lookup tables — same
//! curves, no precision loss.
//!
//! ## Known gaps
//!
//! - **Color-graph evaluation** (gradient interpolation when
//!   `color_graph_type != Scalar`) — `evaluate_color` is still a stub.
//!   Static-color params work via the render_method walker reading
//!   `header.colors[0]` directly (already wired); animated color
//!   gradients aren't.
//! - **Periodic random/noise types** (`noise`, `jitter`, `wander`,
//!   `spark`) need a per-instance PRNG seed. Currently stub to 0.5.
//!   Riverworld water doesn't use these; particle effects do.
//! - **Exclusion** (`EXCLUSION` flag bit + exclusion_min/max range
//!   remap) isn't applied. Engine `c_function_definition::exclude_value`
//!   body isn't decompiled; most shipped tags don't set the flag.
//! - **v2 functions** (`function_definitions_v2.{h,cpp}`) — Reach-era
//!   successor, irrelevant for H3 MCC tags.

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

// ---------------------------------------------------------------------------
// Periodic + transition helpers
// ---------------------------------------------------------------------------
//
// `transition_function_evaluate` / `periodic_function_evaluate` are the
// engine's 1024-entry byte-LUT evaluators (`transition_function_evaluate
// @0x180346C60` / `periodic_function_evaluate @0x180346AC0`), extracted
// verbatim from the dllcache — see `tables.rs`. The periodic noise / jitter /
// wander / spark rows hold *baked pseudo-random data with no closed form*, so
// the LUT is the only faithful source: those four cannot be reproduced
// analytically. (The previous closed-form approximations diverged from the
// engine — notably the transition cosine ease and every periodic noise type.)
mod tables;
pub use tables::{periodic_function_evaluate, transition_function_evaluate, FUNCTION_TABLES};

// ---------------------------------------------------------------------------
// Per-type compact data structures
// ---------------------------------------------------------------------------

/// `c_linear_function_compact` — 8 bytes. `evaluate(x) = slope*x + offset`
/// per `function_definitions.cpp:823`.
#[derive(Debug, Clone, Copy)]
pub struct LinearCompact {
    pub slope: f32,
    pub offset: f32,
}

impl LinearCompact {
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 8 { return None; }
        Some(Self {
            slope:  f32::from_le_bytes(data[0..4].try_into().unwrap()),
            offset: f32::from_le_bytes(data[4..8].try_into().unwrap()),
        })
    }
    fn evaluate(&self, input: f32) -> f32 {
        input * self.slope + self.offset
    }
}

/// `c_spline_function_compact` — 16 bytes. `m_basis_elements` =
/// `real_vector4d (i, j, k, l)`. Per `function_definitions.cpp:868`:
/// `f(x) = i*x³ + j*x² + k*x + l`.
#[derive(Debug, Clone, Copy)]
pub struct SplineCompact {
    pub i: f32,
    pub j: f32,
    pub k: f32,
    pub l: f32,
}

impl SplineCompact {
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 16 { return None; }
        Some(Self {
            i: f32::from_le_bytes(data[0..4].try_into().unwrap()),
            j: f32::from_le_bytes(data[4..8].try_into().unwrap()),
            k: f32::from_le_bytes(data[8..12].try_into().unwrap()),
            l: f32::from_le_bytes(data[12..16].try_into().unwrap()),
        })
    }
    fn evaluate(&self, input: f32) -> f32 {
        let x2 = input * input;
        let x3 = x2 * input;
        self.i * x3 + self.j * x2 + self.k * input + self.l
    }
}

/// `c_spline2_function_compact` — 28 bytes. A 1D spline restricted to
/// the sub-range `[left_x, left_x + width]`, with input remapping
/// driven by `bias`. Per `function_definitions.cpp:265-281`. Body
/// not commented in Ares (`evaluate` is a `_sub_*` forward); the
/// remap is reconstructed from the editor's setup behaviour: input
/// inside `[left_x, left_x + width]` maps to a normalized `[0, 1]`
/// position where `bias` shifts the midpoint, then evaluates the
/// underlying spline at that position.
#[derive(Debug, Clone, Copy)]
pub struct Spline2Compact {
    pub spline: SplineCompact,
    pub left_x: f32,
    pub width: f32,
    pub bias: f32,
}

impl Spline2Compact {
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 28 { return None; }
        let spline = SplineCompact::parse(&data[0..16])?;
        let left_x = f32::from_le_bytes(data[16..20].try_into().unwrap());
        let width  = f32::from_le_bytes(data[20..24].try_into().unwrap());
        let bias   = f32::from_le_bytes(data[24..28].try_into().unwrap());
        Some(Self { spline, left_x, width, bias })
    }
    fn evaluate(&self, input: f32) -> f32 {
        // `c_spline2_function_compact::evaluate @0x1804FBD40` — verbatim:
        //   u  = (input - left_x) / width            (NOT clamped)
        //   u' = sign(u) * |u|^bias                  (signed power remap)
        //   return i*u'³ + j*u'² + k*u' + l          (the inner spline at u')
        if self.width == 0.0 {
            return self.spline.evaluate(0.0);
        }
        let u = (input - self.left_x) / self.width;
        let remapped = u.signum() * u.abs().powf(self.bias);
        self.spline.evaluate(remapped)
    }
}

/// `c_transition_function_compact` — 12 bytes. Per
/// `function_definitions.cpp:1094`:
/// `f(x) = (amp_max - amp_min) * transition_function_evaluate(idx, x)
///       + amp_min`.
#[derive(Debug, Clone, Copy)]
pub struct TransitionCompact {
    pub function_index: u8,
    pub amplitude_min: f32,
    pub amplitude_max: f32,
}

impl TransitionCompact {
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 12 { return None; }
        Some(Self {
            function_index: data[0],
            // bytes 1..4 unused / padding
            amplitude_min: f32::from_le_bytes(data[4..8].try_into().unwrap()),
            amplitude_max: f32::from_le_bytes(data[8..12].try_into().unwrap()),
        })
    }
    fn evaluate(&self, input: f32) -> f32 {
        (self.amplitude_max - self.amplitude_min)
            * transition_function_evaluate(self.function_index, input)
            + self.amplitude_min
    }
}

/// `c_periodic_function_compact` — 20 bytes. Per
/// `function_definitions.cpp:1041` (decompiled body):
/// ```text
/// adjusted_time = input * frequency + phase
/// periodic_value = periodic_function_evaluate(idx, adjusted_time)
/// return (amp_max - amp_min) * periodic_value + amp_min
/// ```
#[derive(Debug, Clone, Copy)]
pub struct PeriodicCompact {
    pub function_index: u8,
    pub frequency: f32,
    pub phase: f32,
    pub amplitude_min: f32,
    pub amplitude_max: f32,
}

impl PeriodicCompact {
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 20 { return None; }
        Some(Self {
            function_index: data[0],
            frequency:     f32::from_le_bytes(data[4..8].try_into().unwrap()),
            phase:         f32::from_le_bytes(data[8..12].try_into().unwrap()),
            amplitude_min: f32::from_le_bytes(data[12..16].try_into().unwrap()),
            amplitude_max: f32::from_le_bytes(data[16..20].try_into().unwrap()),
        })
    }
    fn evaluate(&self, input: f32) -> f32 {
        let adjusted = input * self.frequency + self.phase;
        let v = periodic_function_evaluate(self.function_index, adjusted);
        (self.amplitude_max - self.amplitude_min) * v + self.amplitude_min
    }
}

/// `c_linear_key_function` — 80 bytes piecewise linear over 4 control
/// points. Layout per `function_definitions.cpp:454-457`:
/// ```text
/// real_point2d m_graph_points[4];   // 0x00 (32 bytes — x,y pairs)
/// float        m_times_vector[4];   // 0x20 (16 bytes — postprocess cache)
/// float        m_increment_vector[4]; // 0x30 (16 bytes — 1/(x[i+1]-x[i]))
/// float        m_y_delta_vector[4];   // 0x40 (16 bytes — y[i+1]-y[i])
/// ```
/// `c_linear_key_function::evaluate @0x1804FBAF0` is a **sum of three clamped
/// ramps** over the postprocess-cached vectors — `graph_points` are NOT read at
/// runtime: `y_delta[0] + Σ_{k=1,2,3} clamp((input-times[k])·increment[k],0,1)
/// · y_delta[k]`. (`y_delta[0]` is the base y; `times[k]`/`increment[k]` are the
/// per-segment start-x and 1/width.) `graph_points` is kept only for the parse.
#[derive(Debug, Clone, Copy)]
pub struct LinearKeyCompact {
    pub graph_points: [(f32, f32); 4],
    pub times_vector: [f32; 4],
    pub increment_vector: [f32; 4],
    pub y_delta_vector: [f32; 4],
}

impl LinearKeyCompact {
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 80 { return None; }
        let mut graph_points = [(0.0f32, 0.0f32); 4];
        for i in 0..4 {
            let off = i * 8;
            graph_points[i] = (
                f32::from_le_bytes(data[off..off + 4].try_into().unwrap()),
                f32::from_le_bytes(data[off + 4..off + 8].try_into().unwrap()),
            );
        }
        let mut times_vector = [0.0f32; 4];
        let mut increment_vector = [0.0f32; 4];
        let mut y_delta_vector = [0.0f32; 4];
        for i in 0..4 {
            times_vector[i]     = f32::from_le_bytes(data[32 + i*4..32 + i*4 + 4].try_into().unwrap());
            increment_vector[i] = f32::from_le_bytes(data[48 + i*4..48 + i*4 + 4].try_into().unwrap());
            y_delta_vector[i]   = f32::from_le_bytes(data[64 + i*4..64 + i*4 + 4].try_into().unwrap());
        }
        Some(Self { graph_points, times_vector, increment_vector, y_delta_vector })
    }
    fn evaluate(&self, input: f32) -> f32 {
        // Engine `c_linear_key_function::evaluate @0x1804FBAF0`: sum of three
        // clamped ramps over the cached vectors (NOT a graph_points search).
        let ramp = |k: usize| {
            ((input - self.times_vector[k]) * self.increment_vector[k]).clamp(0.0, 1.0)
        };
        self.y_delta_vector[0]
            + ramp(1) * self.y_delta_vector[1]
            + ramp(2) * self.y_delta_vector[2]
            + ramp(3) * self.y_delta_vector[3]
    }
}

/// `c_multi_part_function_compact` — variable-size. Layout:
/// ```text
/// long              m_function_count;   // 0x0
/// s_function_part   m_function_part[m_function_count];
/// ```
/// Each `s_function_part` is `(header: 8 bytes, function: variable)`.
/// `header.type` is a `e_function_type` (only Linear=4, Spline=7,
/// Spline2=10 are valid for parts) and `header.ending_x` is where this
/// segment ends. Walk parts, find the one whose `ending_x ≥ input`
/// (or the last), evaluate its compact function at input.
///
/// Per `function_definitions.cpp:1216-1228` (`get_size_of_part`):
/// linear part = 16B (8 hdr + 8 body), spline = 24B, spline2 = 36B.
#[derive(Debug, Clone)]
pub struct MultiPartCompact {
    pub parts: Vec<MultiPartSegment>,
}

#[derive(Debug, Clone)]
pub struct MultiPartSegment {
    pub ending_x: f32,
    pub function: MultiPartSubFunction,
}

#[derive(Debug, Clone, Copy)]
pub enum MultiPartSubFunction {
    Linear(LinearCompact),
    Spline(SplineCompact),
    Spline2(Spline2Compact),
}

impl MultiPartCompact {
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 4 { return None; }
        let function_count = i32::from_le_bytes(data[0..4].try_into().unwrap());
        if function_count <= 0 || function_count > 16 {
            // sanity bound — engine has 4-segment max but we allow
            // some headroom for malformed/forward-compat tags.
            return None;
        }
        let mut parts = Vec::with_capacity(function_count as usize);
        let mut off = 4usize;
        for _ in 0..function_count {
            if off + 8 > data.len() { return None; }
            let part_type = data[off];
            // bytes [off+1..off+4] unused
            let ending_x = f32::from_le_bytes(data[off + 4..off + 8].try_into().unwrap());
            let body_off = off + 8;
            let function = match part_type {
                4 /* Linear */ => {
                    let c = LinearCompact::parse(data.get(body_off..)?)?;
                    off = body_off + 8;
                    MultiPartSubFunction::Linear(c)
                }
                7 /* Spline */ => {
                    let c = SplineCompact::parse(data.get(body_off..)?)?;
                    off = body_off + 16;
                    MultiPartSubFunction::Spline(c)
                }
                10 /* Spline2 */ => {
                    let c = Spline2Compact::parse(data.get(body_off..)?)?;
                    off = body_off + 28;
                    MultiPartSubFunction::Spline2(c)
                }
                _ => return None,
            };
            parts.push(MultiPartSegment { ending_x, function });
        }
        Some(Self { parts })
    }
    fn evaluate(&self, input: f32) -> f32 {
        if self.parts.is_empty() { return 0.0; }
        // Find the first part whose ending_x ≥ input. The engine's
        // pseudocode iterates `function_part` and breaks when found.
        for part in &self.parts {
            if input <= part.ending_x {
                return match &part.function {
                    MultiPartSubFunction::Linear(c)  => c.evaluate(input),
                    MultiPartSubFunction::Spline(c)  => c.evaluate(input),
                    MultiPartSubFunction::Spline2(c) => c.evaluate(input),
                };
            }
        }
        // Past the last part's ending_x: engine `c_multi_part_function_compact::
        // evaluate @0x1804FBB90` returns 0.0 (the loop only writes its accumulator
        // inside an accepted `input <= ending_x` branch; no match → 0.0). We do
        // NOT extrapolate the last part.
        0.0
    }
}

/// `c_exponent_function_compact` — 12 bytes. Per
/// `function_definitions.cpp:976-1003`:
/// ```text
/// if |exponent| < 1e-4 || (exponent < 0 && |input| < 1e-4):
///     return 1.0
/// else:
///     return powf(input, exponent) * (amplitude_max - amplitude_min)
///          + amplitude_min
/// ```
#[derive(Debug, Clone, Copy)]
pub struct ExponentCompact {
    pub amplitude_min: f32,
    pub amplitude_max: f32,
    pub exponent: f32,
}

impl ExponentCompact {
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 12 { return None; }
        Some(Self {
            amplitude_min: f32::from_le_bytes(data[0..4].try_into().unwrap()),
            amplitude_max: f32::from_le_bytes(data[4..8].try_into().unwrap()),
            exponent:      f32::from_le_bytes(data[8..12].try_into().unwrap()),
        })
    }
    fn evaluate(&self, input: f32) -> f32 {
        const EPSILON: f32 = 0.000_099_999_997;
        if self.exponent.abs() < EPSILON
            || (self.exponent < 0.0 && input.abs() < EPSILON)
        {
            return 1.0;
        }
        input.powf(self.exponent) * (self.amplitude_max - self.amplitude_min)
            + self.amplitude_min
    }
}

// ---------------------------------------------------------------------------
// TagFunction enum
// ---------------------------------------------------------------------------

/// Decoded TagFunction. All 11 function types parse + evaluate.
#[derive(Debug, Clone)]
pub enum TagFunction {
    Identity { header: TagFunctionHeader },
    Constant { header: TagFunctionHeader },
    Transition { header: TagFunctionHeader, compact: TransitionCompact },
    Periodic { header: TagFunctionHeader, compact: PeriodicCompact },
    Linear   { header: TagFunctionHeader, compact: LinearCompact },
    LinearKey { header: TagFunctionHeader, compact: LinearKeyCompact },
    /// `MultiLinearKey` — multi-graph LinearKey (one graph per color channel).
    /// NOTE: the engine `evaluate_legacy @0x1804F9390` has **no case 6** — type
    /// 6 hits `default` and returns 0.0; it is a COLOR-only multi-graph type that
    /// the cache-builder pre-splits into per-channel type-5 functions, so the
    /// engine's scalar path never meaningfully sees it. We load loose (uncached)
    /// tags, so we approximate by evaluating the FIRST graph as a LinearKey
    /// (3-ramp-sum) — better than the engine's literal 0.0 for an animated color.
    MultiLinearKey { header: TagFunctionHeader, compact: LinearKeyCompact },
    Spline   { header: TagFunctionHeader, compact: SplineCompact },
    Spline2  { header: TagFunctionHeader, compact: Spline2Compact },
    /// `MultiSpline` (a.k.a. `_function_type_multi_part`, enum value 8).
    /// Variable-size sequence of (Linear | Spline | Spline2) parts each
    /// covering an `[ending_x[i-1], ending_x[i]]` sub-domain.
    MultiSpline { header: TagFunctionHeader, compact: MultiPartCompact },
    Exponent { header: TagFunctionHeader, compact: ExponentCompact },
    /// Function type recognized but not yet implemented. `evaluate`
    /// returns 0.0; `as_constant()` returns None.
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
        // Compact data follows the 32-byte header. Length = `compact_size`
        // when the header reports it; older blobs may have it in
        // `m_constants[0]` — for now we trust the header field and
        // bound by remaining bytes.
        let compact = data.get(32..).unwrap_or(&[]);
        Ok(match header.function_type {
            FunctionType::Identity => Self::Identity { header },
            FunctionType::Constant => Self::Constant { header },
            FunctionType::Transition => match TransitionCompact::parse(compact) {
                Some(c) => Self::Transition { header, compact: c },
                None => Self::Unsupported { header, raw: data.to_vec() },
            },
            FunctionType::Periodic => match PeriodicCompact::parse(compact) {
                Some(c) => Self::Periodic { header, compact: c },
                None => Self::Unsupported { header, raw: data.to_vec() },
            },
            FunctionType::Linear => match LinearCompact::parse(compact) {
                Some(c) => Self::Linear { header, compact: c },
                None => Self::Unsupported { header, raw: data.to_vec() },
            },
            FunctionType::Spline => match SplineCompact::parse(compact) {
                Some(c) => Self::Spline { header, compact: c },
                None => Self::Unsupported { header, raw: data.to_vec() },
            },
            FunctionType::Spline2 => match Spline2Compact::parse(compact) {
                Some(c) => Self::Spline2 { header, compact: c },
                None => Self::Unsupported { header, raw: data.to_vec() },
            },
            FunctionType::Exponent => match ExponentCompact::parse(compact) {
                Some(c) => Self::Exponent { header, compact: c },
                None => Self::Unsupported { header, raw: data.to_vec() },
            },
            FunctionType::LinearKey => match LinearKeyCompact::parse(compact) {
                Some(c) => Self::LinearKey { header, compact: c },
                None => Self::Unsupported { header, raw: data.to_vec() },
            },
            FunctionType::MultiLinearKey => match LinearKeyCompact::parse(compact) {
                Some(c) => Self::MultiLinearKey { header, compact: c },
                None => Self::Unsupported { header, raw: data.to_vec() },
            },
            FunctionType::MultiSpline => match MultiPartCompact::parse(compact) {
                Some(c) => Self::MultiSpline { header, compact: c },
                None => Self::Unsupported { header, raw: data.to_vec() },
            },
        })
    }

    pub fn header(&self) -> &TagFunctionHeader {
        match self {
            Self::Identity { header }
            | Self::Constant { header }
            | Self::Transition { header, .. }
            | Self::Periodic { header, .. }
            | Self::Linear { header, .. }
            | Self::LinearKey { header, .. }
            | Self::MultiLinearKey { header, .. }
            | Self::Spline { header, .. }
            | Self::Spline2 { header, .. }
            | Self::MultiSpline { header, .. }
            | Self::Exponent { header, .. }
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
        // Engine animated-scalar path = `evaluate_function @0x1806864d0`
        // (→ `evaluate_legacy`) followed by `update_constants @0x180685300`
        // applying `map_to_output_range_legacy` to the result. So the full
        // value IS `map_to_output_range(evaluate_legacy)`, periodic included —
        // the periodic compact's amp_min/amp_max is the NORMALIZED curve, then
        // the header clamp_range maps it (with the CLAMPED-flag [0,1] clamp,
        // see `map_to_output_range`). For the storm self_illum_intensity
        // (amp [-8,1.5], clamp_range [1,20], CLAMPED): clamp(amp·noise) to
        // [0,1] → map to [1,20] = a bright positive flash, never negative.
        let normalized = self.evaluate_legacy(input, range);
        self.map_to_output_range(normalized)
    }

    /// The "normalized" curve output before output-range remapping.
    /// Ports `c_function_definition::evaluate_legacy` for the types
    /// implemented so far.
    fn evaluate_legacy(&self, input: f32, range: f32) -> f32 {
        let v = match self {
            Self::Identity { .. } => input,
            Self::Constant { header } => {
                if header.flags.is_ranged() { range } else { 0.0 }
            }
            // Compact-data evaluators output the function's value
            // directly; map_to_output_range_legacy is then applied by
            // the caller (`evaluate`). The compacts already encode
            // amplitude_min/max for types that need them
            // (Exponent), so the outer map is a no-op for those when
            // clamp_range = (0, 1). For Linear / Spline / Spline2,
            // the engine applies clamp_range as the [out_min, out_max]
            // interpretation per `map_to_output_range_legacy`.
            Self::Transition     { compact, .. } => compact.evaluate(input),
            Self::Periodic       { compact, .. } => compact.evaluate(input),
            Self::Linear         { compact, .. } => compact.evaluate(input),
            Self::LinearKey      { compact, .. } => compact.evaluate(input),
            Self::MultiLinearKey { compact, .. } => compact.evaluate(input),
            Self::Spline         { compact, .. } => compact.evaluate(input),
            Self::Spline2        { compact, .. } => compact.evaluate(input),
            Self::MultiSpline    { compact, .. } => compact.evaluate(input),
            Self::Exponent       { compact, .. } => compact.evaluate(input),
            Self::Unsupported { .. } => 0.0,
        };
        // Engine `evaluate_legacy @0x1804F9390` applies `unexclude_value
        // @0x1804facd0` to the per-type result (before the output map): when the
        // EXCLUSION flag (0x08) is set, re-expand the excluded discontinuity —
        // `if v > exclusion_min { v += exclusion_max - exclusion_min }`.
        let h = self.header();
        if h.flags.has_exclusion() && v > h.exclusion_min {
            v + (h.exclusion_max - h.exclusion_min)
        } else {
            v
        }
    }

    /// Linearly map a normalized output through `[clamp_range_min,
    /// clamp_range_max]`. Mirrors
    /// `c_function_definition::map_to_output_range_legacy`.
    fn map_to_output_range(&self, normalized: f32) -> f32 {
        let h = self.header();
        // Engine `c_function_definition::map_to_output_range_legacy`
        // (@0x1804F99A0, verified vs Ares evaluate_scalar): when the CLAMPED
        // flag (0x04 = engine header-dword bit 0x400) is set, the normalized
        // output is clamped to [0,1] BEFORE the lerp. Without this, a periodic
        // compact whose own amplitude (`amp_min/max`) already pushed the value
        // outside [0,1] gets re-mapped through clamp_range and explodes — e.g.
        // the mainmenu storm `self_illum_intensity` (periodic noise, amp
        // [-8,1.5], clamp_range [1,20]) resolved to -60.75 instead of 1.0.
        let n = if h.flags.is_clamped() {
            normalized.clamp(0.0, 1.0)
        } else {
            normalized
        };
        h.clamp_range_min + n * (h.clamp_range_max - h.clamp_range_min)
    }

    /// Fast path for the common case: callers that just want a single
    /// scalar value for a constant material parameter. Returns `Some`
    /// for constant (and trivially-constant compact) functions, `None`
    /// for anything that genuinely depends on input.
    pub fn as_constant(&self) -> Option<f32> {
        match self {
            Self::Constant { header } if !header.flags.is_ranged() => {
                Some(header.clamp_range_min)
            }
            // A linear function with slope=0 is constant at `offset`,
            // remapped through clamp_range. Same idea for other types
            // whose compact data trivially collapses to a constant.
            Self::Linear { compact, .. } if compact.slope == 0.0 => {
                Some(self.map_to_output_range(compact.offset))
            }
            Self::Spline { compact, .. }
                if compact.i == 0.0 && compact.j == 0.0 && compact.k == 0.0 =>
            {
                Some(self.map_to_output_range(compact.l))
            }
            Self::Exponent { compact, .. }
                if compact.exponent.abs() < 1e-4
                    || compact.amplitude_min == compact.amplitude_max =>
            {
                let v = if compact.exponent.abs() < 1e-4 {
                    1.0
                } else {
                    compact.amplitude_min
                };
                Some(self.map_to_output_range(v))
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

    /// Color-output evaluator — interpolates the `m_colors[4]` ARGB
    /// stops (bytes 4-19) by the scalar curve output. `OneColor` returns
    /// the single stop; `TwoColor`+ walk a piecewise gradient. `Scalar`
    /// graphs carry no stops → grayscale (n,n,n). Mirrors the engine's
    /// `c_function_definition::evaluate_color` → `map_to_color_range_legacy`.
    pub fn evaluate_color(&self, input: f32, range: f32) -> RealRgbColor {
        let h = self.header();
        let unpack = |c: u32| RealRgbColor {
            red: ((c >> 16) & 0xff) as f32 / 255.0,
            green: ((c >> 8) & 0xff) as f32 / 255.0,
            blue: (c & 0xff) as f32 / 255.0,
        };
        // Engine `map_to_color_range_legacy @0x1804f9af0` constant short-circuit:
        // a Constant color function returns colors[0] directly when unranged, or
        // when the graph is Scalar/OneColor (no gradient to walk).
        if h.function_type == FunctionType::Constant
            && (!h.flags.is_ranged()
                || matches!(h.color_graph_type, ColorGraphType::Scalar | ColorGraphType::OneColor))
        {
            return unpack(h.colors[0]);
        }
        // Gradient position from the underlying scalar curve.
        let t = self.evaluate_legacy(input, range).clamp(0.0, 1.0);
        // Engine indexes the 4-slot m_colors array NON-consecutively: TwoColor
        // interpolates [0]→[3]; ThreeColor uses [0],[1],[3] (skips [2]); FourColor
        // [0..3]. A Scalar graph returns grayscale (n,n,n), NOT white.
        let stops: &[usize] = match h.color_graph_type {
            ColorGraphType::Scalar => return RealRgbColor { red: t, green: t, blue: t },
            ColorGraphType::OneColor => &[0],
            ColorGraphType::TwoColor => &[0, 3],
            ColorGraphType::ThreeColor => &[0, 1, 3],
            ColorGraphType::FourColor => &[0, 1, 2, 3],
        };
        if stops.len() == 1 {
            return unpack(h.colors[stops[0]]);
        }
        let pos = t * (stops.len() - 1) as f32;
        let i = (pos.floor() as usize).min(stops.len() - 2);
        let f = pos - i as f32;
        let a = unpack(h.colors[stops[i]]);
        let b = unpack(h.colors[stops[i + 1]]);
        RealRgbColor {
            red: a.red + (b.red - a.red) * f,
            green: a.green + (b.green - a.green) * f,
            blue: a.blue + (b.blue - a.blue) * f,
        }
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

    /// Build a 32-byte header with the given function type + clamp range.
    fn header_with(func_type: u8, clamp_min: f32, clamp_max: f32) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[0] = func_type;
        bytes[4..8].copy_from_slice(&clamp_min.to_le_bytes());
        bytes[8..12].copy_from_slice(&clamp_max.to_le_bytes());
        bytes
    }

    #[test]
    fn linear_evaluates() {
        // y = 2*x + 5, mapped through clamp [0, 1] (no-op)
        let mut blob = header_with(4, 0.0, 1.0).to_vec();
        blob.extend_from_slice(&2.0f32.to_le_bytes());  // slope
        blob.extend_from_slice(&5.0f32.to_le_bytes());  // offset
        let f = TagFunction::parse(&blob).unwrap();
        assert_eq!(f.function_type(), FunctionType::Linear);
        // evaluate(x) = 2x + 5; clamp [0, 1] → linear remap from
        // normalized [0, 1] to [0, 1] is identity. So 2*0 + 5 = 5
        // would normally be the result, but the engine applies the
        // OUTPUT range, treating compact output as the [0,1] normalized
        // value. With clamp [0,1] the map is identity so we get 5.
        assert!((f.evaluate(0.0, 0.0) - 5.0).abs() < 1e-5);
        assert!((f.evaluate(1.0, 0.0) - 7.0).abs() < 1e-5);
    }

    #[test]
    fn linear_constant_recognized() {
        // slope=0, offset=3, clamp [0, 1] → constant 3.
        let mut blob = header_with(4, 0.0, 1.0).to_vec();
        blob.extend_from_slice(&0.0f32.to_le_bytes());
        blob.extend_from_slice(&3.0f32.to_le_bytes());
        let f = TagFunction::parse(&blob).unwrap();
        // map_to_output_range(3) when clamp=[0,1] is 0 + 3*(1-0) = 3.
        assert_eq!(f.as_constant(), Some(3.0));
    }

    #[test]
    fn spline_evaluates_cubic() {
        // f(x) = 1*x³ + 0*x² + 0*x + 0 = x³
        let mut blob = header_with(7, 0.0, 1.0).to_vec();
        blob.extend_from_slice(&1.0f32.to_le_bytes()); // i
        blob.extend_from_slice(&0.0f32.to_le_bytes()); // j
        blob.extend_from_slice(&0.0f32.to_le_bytes()); // k
        blob.extend_from_slice(&0.0f32.to_le_bytes()); // l
        let f = TagFunction::parse(&blob).unwrap();
        assert_eq!(f.function_type(), FunctionType::Spline);
        assert!((f.evaluate(0.5, 0.0) - 0.125).abs() < 1e-5);
        assert!((f.evaluate(2.0, 0.0) - 8.0).abs() < 1e-5);
    }

    #[test]
    fn spline_constant_recognized() {
        // i=j=k=0, l=4 → constant 4 (after clamp [0, 1] identity remap).
        let mut blob = header_with(7, 0.0, 1.0).to_vec();
        blob.extend_from_slice(&0.0f32.to_le_bytes());
        blob.extend_from_slice(&0.0f32.to_le_bytes());
        blob.extend_from_slice(&0.0f32.to_le_bytes());
        blob.extend_from_slice(&4.0f32.to_le_bytes());
        let f = TagFunction::parse(&blob).unwrap();
        assert_eq!(f.as_constant(), Some(4.0));
    }

    #[test]
    fn spline2_evaluates_with_subrange() {
        // Inner spline f(t) = t. Sub-range [0.2, 0.7] (left_x=0.2, width=0.5).
        // Bias=0.5 → linear remap: t = (input - 0.2) / 0.5, clamped.
        let mut blob = header_with(10, 0.0, 1.0).to_vec();
        // spline (i=j=l=0, k=1) → f(t) = t
        blob.extend_from_slice(&0.0f32.to_le_bytes()); // i
        blob.extend_from_slice(&0.0f32.to_le_bytes()); // j
        blob.extend_from_slice(&1.0f32.to_le_bytes()); // k
        blob.extend_from_slice(&0.0f32.to_le_bytes()); // l
        blob.extend_from_slice(&0.2f32.to_le_bytes()); // left_x
        blob.extend_from_slice(&0.5f32.to_le_bytes()); // width
        blob.extend_from_slice(&0.5f32.to_le_bytes()); // bias = linear
        let f = TagFunction::parse(&blob).unwrap();
        assert_eq!(f.function_type(), FunctionType::Spline2);
        // Engine `c_spline2_function_compact::evaluate @0x1804FBD40`:
        //   u  = (input - left_x) / width             (NOT clamped)
        //   u' = sign(u) * |u|^bias                    (bias=0.5 → signed sqrt)
        //   return inner_spline(u')                    (here f(t)=t → u')
        // input=0.45 → u=0.5 → u'=sqrt(0.5)=0.70711
        assert!((f.evaluate(0.45, 0.0) - 0.5f32.sqrt()).abs() < 1e-5);
        // input=0.0 → u=-0.4 → u'=-sqrt(0.4)=-0.63246 (no clamping)
        assert!((f.evaluate(0.0, 0.0) - (-(0.4f32.sqrt()))).abs() < 1e-5);
        // input=1.0 → u=1.6 → u'=sqrt(1.6)=1.26491 (extrapolates past 1)
        assert!((f.evaluate(1.0, 0.0) - 1.6f32.sqrt()).abs() < 1e-5);
    }

    #[test]
    fn exponent_evaluates_pow_curve() {
        // amp_min=0, amp_max=1, exponent=2 → input^2
        let mut blob = header_with(9, 0.0, 1.0).to_vec();
        blob.extend_from_slice(&0.0f32.to_le_bytes());
        blob.extend_from_slice(&1.0f32.to_le_bytes());
        blob.extend_from_slice(&2.0f32.to_le_bytes());
        let f = TagFunction::parse(&blob).unwrap();
        assert_eq!(f.function_type(), FunctionType::Exponent);
        assert!((f.evaluate(0.5, 0.0) - 0.25).abs() < 1e-5);
        assert!((f.evaluate(0.7, 0.0) - 0.49).abs() < 1e-5);
    }

    #[test]
    fn transition_linear_passes_through() {
        // function_index=0 (linear), amp_min=0, amp_max=1
        let mut blob = header_with(2, 0.0, 1.0).to_vec();
        blob.push(0); blob.extend_from_slice(&[0, 0, 0]); // linear + padding
        blob.extend_from_slice(&0.0f32.to_le_bytes());
        blob.extend_from_slice(&1.0f32.to_le_bytes());
        let f = TagFunction::parse(&blob).unwrap();
        assert_eq!(f.function_type(), FunctionType::Transition);
        // linear ramp 0..1
        assert!((f.evaluate(0.0, 0.0) - 0.0).abs() < 1e-5);
        assert!((f.evaluate(0.5, 0.0) - 0.5).abs() < 1e-5);
        assert!((f.evaluate(1.0, 0.0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn transition_late_eases_in() {
        // function_index=3 (late = ease-in), amp_min=0, amp_max=1
        let mut blob = header_with(2, 0.0, 1.0).to_vec();
        blob.push(3); blob.extend_from_slice(&[0, 0, 0]);
        blob.extend_from_slice(&0.0f32.to_le_bytes());
        blob.extend_from_slice(&1.0f32.to_le_bytes());
        let f = TagFunction::parse(&blob).unwrap();
        // "late" (engine LUT row 3) eases in ≈ t² → ~0.25 at the midpoint.
        // Tolerance covers the 1024-entry byte quantization (±1/255 + interp).
        let mid = f.evaluate(0.5, 0.0);
        assert!((mid - 0.25).abs() < 0.01, "late(0.5) = {mid}");
        assert!(mid < 0.5, "late eases in");
    }

    #[test]
    fn transition_one_constant() {
        // function_index=6 (one) → constant amp_max
        let mut blob = header_with(2, 0.0, 1.0).to_vec();
        blob.push(6); blob.extend_from_slice(&[0, 0, 0]);
        blob.extend_from_slice(&0.5f32.to_le_bytes()); // amp_min
        blob.extend_from_slice(&3.0f32.to_le_bytes()); // amp_max
        let f = TagFunction::parse(&blob).unwrap();
        // (3 - 0.5) * 1.0 + 0.5 = 3.0
        assert!((f.evaluate(0.42, 0.0) - 3.0).abs() < 1e-5);
    }

    #[test]
    fn periodic_cosine_oscillates() {
        // function_index=2 (cosine), frequency=1, phase=0, amp [-1, 1]
        let mut blob = header_with(3, -1.0, 1.0).to_vec();
        blob.push(2); blob.extend_from_slice(&[0, 0, 0]);
        blob.extend_from_slice(&1.0f32.to_le_bytes()); // frequency
        blob.extend_from_slice(&0.0f32.to_le_bytes()); // phase
        blob.extend_from_slice(&0.0f32.to_le_bytes()); // amp_min (compact)
        blob.extend_from_slice(&1.0f32.to_le_bytes()); // amp_max (compact)
        let f = TagFunction::parse(&blob).unwrap();
        assert_eq!(f.function_type(), FunctionType::Periodic);
        // Engine periodic LUT (row 2): the cosine starts at the table TOP
        // (byte[0]=0xff → 1.0), unlike the old `0.5-0.5cos` approximation
        // which started at 0. At input 0: lut=1.0, compact=(1-0)*1+0=1.0,
        // outer [-1,1] → -1 + 1*2 = 1.0. (Clean grid hit, engine-exact.)
        assert!((f.evaluate(0.0, 0.0) - 1.0).abs() < 1e-2, "cosine@0");
        // The function must oscillate across the full [-1, 1] output band.
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for n in 0..400 {
            let v = f.evaluate(n as f32 * 0.05, 0.0); // input 0..20
            lo = lo.min(v);
            hi = hi.max(v);
        }
        assert!(hi > 0.8 && lo < -0.8, "cosine oscillates: [{lo}, {hi}]");
    }

    #[test]
    fn periodic_diagonal_wave_triangle() {
        // function_index=4 (diagonal_wave / triangle wave)
        let mut blob = header_with(3, 0.0, 1.0).to_vec();
        blob.push(4); blob.extend_from_slice(&[0, 0, 0]);
        blob.extend_from_slice(&1.0f32.to_le_bytes());
        blob.extend_from_slice(&0.0f32.to_le_bytes());
        blob.extend_from_slice(&0.0f32.to_le_bytes());
        blob.extend_from_slice(&1.0f32.to_le_bytes());
        let f = TagFunction::parse(&blob).unwrap();
        // Engine periodic LUT (row 4): the diagonal/triangle wave starts at
        // the table bottom (byte[0]=0x00 → 0.0) at input 0 (clean grid hit).
        assert!(f.evaluate(0.0, 0.0).abs() < 1e-2, "triangle@0");
        // Over a span of inputs it must sweep the full [0, 1] band.
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for n in 0..400 {
            let v = f.evaluate(n as f32 * 0.05, 0.0); // input 0..20
            lo = lo.min(v);
            hi = hi.max(v);
        }
        assert!(hi > 0.9 && lo < 0.1, "triangle oscillates: [{lo}, {hi}]");
    }

    #[test]
    fn linear_key_4_points() {
        // Type 5 (LinearKey). 4 control points: (0, 0), (0.25, 1.0),
        // (0.75, 1.0), (1.0, 0.0) — a trapezoid pulse.
        let mut blob = header_with(5, 0.0, 1.0).to_vec();
        // graph_points
        for &(x, y) in &[(0.0_f32, 0.0_f32), (0.25, 1.0), (0.75, 1.0), (1.0, 0.0)] {
            blob.extend_from_slice(&x.to_le_bytes());
            blob.extend_from_slice(&y.to_le_bytes());
        }
        // The engine evaluator reads ONLY these postprocess-cached vectors
        // (3-ramp-sum), not graph_points. For the trapezoid: a rise ramp over
        // [0,0.25] and a fall ramp over [0.75,1.0].
        // times_vector: per-ramp start-x (k=1 rise@0, k=2 fall@0.75).
        for &v in &[0.0_f32, 0.0, 0.75, 0.0] { blob.extend_from_slice(&v.to_le_bytes()); }
        // increment_vector: 1/width per ramp (both 1/0.25 = 4).
        for &v in &[0.0_f32, 4.0, 4.0, 0.0] { blob.extend_from_slice(&v.to_le_bytes()); }
        // y_delta_vector: [base_y, +rise, -fall, unused].
        for &v in &[0.0_f32, 1.0, -1.0, 0.0] { blob.extend_from_slice(&v.to_le_bytes()); }
        let f = TagFunction::parse(&blob).unwrap();
        assert_eq!(f.function_type(), FunctionType::LinearKey);
        // Ramp up: at t=0.125 → halfway between (0,0) and (0.25,1) = 0.5
        assert!((f.evaluate(0.125, 0.0) - 0.5).abs() < 1e-5);
        // Plateau: at t=0.5 → between (0.25,1) and (0.75,1) = 1
        assert!((f.evaluate(0.5, 0.0) - 1.0).abs() < 1e-5);
        // Ramp down: at t=0.875 → halfway from 1 to 0 = 0.5
        assert!((f.evaluate(0.875, 0.0) - 0.5).abs() < 1e-5);
        // Clamp before / after
        assert_eq!(f.evaluate(-1.0, 0.0), 0.0);
        assert_eq!(f.evaluate(2.0, 0.0), 0.0);
    }

    #[test]
    fn multi_part_linear_segments() {
        // Type 8 (MultiSpline / multi_part). 2 linear parts:
        // Part 1: x ∈ [0, 0.5], f(x) = 2x.        slope=2, offset=0
        // Part 2: x ∈ [0.5, 1.0], f(x) = -2x+2.   slope=-2, offset=2
        // Triangle peaking at (0.5, 1).
        let mut blob = header_with(8, 0.0, 1.0).to_vec();
        // function_count = 2
        blob.extend_from_slice(&2i32.to_le_bytes());
        // Part 1: header (type=4 linear, ending_x=0.5) + linear body
        blob.push(4); blob.extend_from_slice(&[0, 0, 0]);
        blob.extend_from_slice(&0.5_f32.to_le_bytes()); // ending_x
        blob.extend_from_slice(&2.0_f32.to_le_bytes()); // slope
        blob.extend_from_slice(&0.0_f32.to_le_bytes()); // offset
        // Part 2: header (type=4 linear, ending_x=1.0) + linear body
        blob.push(4); blob.extend_from_slice(&[0, 0, 0]);
        blob.extend_from_slice(&1.0_f32.to_le_bytes()); // ending_x
        blob.extend_from_slice(&(-2.0_f32).to_le_bytes()); // slope
        blob.extend_from_slice(&2.0_f32.to_le_bytes()); // offset
        let f = TagFunction::parse(&blob).unwrap();
        assert_eq!(f.function_type(), FunctionType::MultiSpline);
        // Part 1 at x=0.25 → 0.5
        assert!((f.evaluate(0.25, 0.0) - 0.5).abs() < 1e-5);
        // Part 1 at x=0.5 → 1.0
        assert!((f.evaluate(0.5, 0.0) - 1.0).abs() < 1e-5);
        // Part 2 at x=0.75 → -1.5 + 2 = 0.5
        assert!((f.evaluate(0.75, 0.0) - 0.5).abs() < 1e-5);
        // Past the last part's ending_x: engine returns 0.0 (no extrapolation).
        assert!(f.evaluate(1.5, 0.0).abs() < 1e-5);
    }

    #[test]
    fn exponent_zero_returns_one() {
        // |exponent| < epsilon → returns 1.0 (no remap, since the
        // engine returns 1.0 from evaluate_legacy directly).
        let mut blob = header_with(9, 0.0, 10.0).to_vec();
        blob.extend_from_slice(&0.0f32.to_le_bytes());
        blob.extend_from_slice(&5.0f32.to_le_bytes());
        blob.extend_from_slice(&0.0f32.to_le_bytes()); // exponent ≈ 0
        let f = TagFunction::parse(&blob).unwrap();
        // Exponent collapses to 1.0 via compact.evaluate, then maps
        // through clamp [0, 10] → 0 + 1.0*(10-0) = 10.
        assert!((f.evaluate(0.5, 0.0) - 10.0).abs() < 1e-4);
    }

    #[test]
    fn exclusion_widens_above_min() {
        // Identity + EXCLUSION flag: evaluate_legacy applies unexclude_value —
        // input > exclusion_min → input + (exclusion_max - exclusion_min).
        // clamp_range [0,1] without the CLAMPED flag is an identity map.
        let mut hdr = header_with(0, 0.0, 1.0);
        hdr[1] = 0x08; // EXCLUSION flag
        hdr[20..24].copy_from_slice(&0.5f32.to_le_bytes()); // exclusion_min
        hdr[24..28].copy_from_slice(&1.5f32.to_le_bytes()); // exclusion_max
        let f = TagFunction::parse(&hdr).unwrap();
        // 0.3 <= min → unchanged
        assert!((f.evaluate(0.3, 0.0) - 0.3).abs() < 1e-5);
        // 0.7 > min → 0.7 + (1.5 - 0.5) = 1.7
        assert!((f.evaluate(0.7, 0.0) - 1.7).abs() < 1e-5);
    }
}
