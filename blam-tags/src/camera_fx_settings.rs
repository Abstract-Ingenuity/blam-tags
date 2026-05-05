//! `camera_fx_settings` (`cfxs`) tag walker — per-level exposure,
//! bloom, and tone curve. Pointed at by `scenario.camera_fx_settings`.
//!
//! Halo's render path (per dllcache):
//! - `c_player_view::setup_camera_fx_parameters @ 0x180689c20` reads
//!   the scenario's camera_fx_settings and applies it to the player's
//!   `m_camera_fx_values`.
//! - `c_camera_fx_values::get_render_exposure @ 0x18068e3e0` then
//!   computes the per-frame view_exposure as:
//!   `pow(2, scripted + g_exposure_stops + exposure_boost)
//!    × tone_curve_white_point × 0.66943294`.
//! - `c_rasterizer::setup_render_target_globals_with_exposure @ 0x180670ad0`
//!   uploads `(view_exposure, pow(2, HDR_target_stops), 1, 1)` to
//!   shader cbuffer slot 0x28 (`g_exposure`).
//!
//! For now we walk only the exposure block — the rest of the tag
//! (bloom, bling, tone curve) lands when those passes go in.

use crate::api::TagStruct;
use crate::file::TagFile;
use crate::math::RealRgbColor;

const CFXS_GROUP: [u8; 4] = *b"cfxs";

#[derive(Debug)]
pub enum CameraFxError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
}

impl std::fmt::Display for CameraFxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongGroup { expected, actual } => write!(
                f,
                "expected group '{}', got '{}'",
                std::str::from_utf8(expected).unwrap_or("?"),
                std::str::from_utf8(actual).unwrap_or("?"),
            ),
        }
    }
}
impl std::error::Error for CameraFxError {}

/// Decoded `.camera_fx_settings`. Subset — exposure + bloom basics.
#[derive(Debug, Clone, Default)]
pub struct CameraFxSettings {
    /// Exposure block.
    pub exposure: ExposureBlock,
    /// `bloom_point` — bright threshold (typically 1.5 in HDR units).
    pub bloom_point: f32,
    /// `bloom_intensity` — global bloom multiplier (riverworld: 0.1).
    pub bloom_intensity: f32,
    /// `bloom_inherent` — additional bloom on top of the threshold (0.1 default).
    pub bloom_inherent: f32,
    /// `bling_intensity`, `bling_size`, `bling_angle`, `bling_count`
    /// — sun-disc spike effects, captured for completeness.
    pub bling_intensity: f32,
    pub bling_size: f32,
    pub bling_angle_deg: f32,
    pub bling_count: u8,
    /// Per-stage bloom color overrides (when not flagged "use default").
    pub bloom_large_color: RealRgbColor,
    pub bloom_medium_color: RealRgbColor,
    pub bloom_small_color: RealRgbColor,
}

#[derive(Debug, Clone, Default)]
pub struct ExposureBlock {
    /// Bit 0: auto-adjust target. Bit 2: auto-adjust delay enabled.
    /// Bit 4: fixed (use `exposure` value verbatim, no auto). Bit 5:
    /// scripted.
    pub flags: u16,
    /// Exposure stops (log₂ of luminance multiplier). 0 = neutral.
    /// `view_exposure = pow(2, exposure) × tone_curve_white_point × 0.66943`.
    pub exposure: f32,
    /// Auto-exposure max delta per frame.
    pub maximum_change: f32,
    /// Auto-exposure blend speed.
    pub blend_speed: f32,
    /// Min/max stops clamp for auto-exposure.
    pub minimum: f32,
    pub maximum: f32,
    /// Target screen brightness for auto-exposure (0-1).
    pub auto_exposure_screen_brightness: f32,
    pub auto_exposure_delay: f32,
}

impl ExposureBlock {
    /// `flags & 0x10` — fixed-exposure flag.
    pub fn is_fixed(&self) -> bool {
        (self.flags & 0x10) != 0
    }

    /// Compute Halo's `view_exposure` for a given exposure_boost +
    /// tone_curve_white_point (default 1.0). Mirrors
    /// `c_camera_fx_values::get_render_exposure @ 0x18068e3e0` — we
    /// skip the scripted_exposure component (always 0 for our v1).
    pub fn view_exposure(&self, tone_curve_white_point: f32, exposure_boost: f32) -> f32 {
        // Effective stops: scripted (0) + exposure (`g_exposure_stops`
        // is the same field) + boost.
        let stops = self.exposure + exposure_boost;
        2.0_f32.powf(stops) * tone_curve_white_point * 0.66943294
    }
}

impl CameraFxSettings {
    pub fn from_tag(tag: &TagFile) -> Result<Self, CameraFxError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != CFXS_GROUP {
            return Err(CameraFxError::WrongGroup { expected: CFXS_GROUP, actual });
        }
        Ok(Self::from_struct(&tag.root()))
    }

    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        let exposure = s
            .field("exposure")
            .and_then(|f| f.as_struct())
            .map(|sub| ExposureBlock {
                flags: sub.read_int_any("flags").unwrap_or(0) as u16,
                exposure: sub.read_real("exposure").unwrap_or(0.0),
                maximum_change: sub.read_real("maximum change").unwrap_or(0.0),
                blend_speed: sub.read_real("blend speed (0-1)").unwrap_or(0.0),
                minimum: sub.read_real("minimum").unwrap_or(0.0),
                maximum: sub.read_real("maximum").unwrap_or(0.0),
                auto_exposure_screen_brightness: sub
                    .read_real("auto-exposure screen brightness")
                    .unwrap_or(0.5),
                auto_exposure_delay: sub
                    .read_real("auto-exposure delay")
                    .unwrap_or(0.0),
            })
            .unwrap_or_default();

        // Bloom + bling fields are nested structs with `value` field
        // for the scalar plus blend-speed/max-change wrappers.
        let read_struct_real = |name: &str, field: &str| -> f32 {
            s.field(name)
                .and_then(|f| f.as_struct())
                .and_then(|sub| sub.read_real(field))
                .unwrap_or(0.0)
        };
        let read_struct_color = |name: &str, field: &str| -> RealRgbColor {
            s.field(name)
                .and_then(|f| f.as_struct())
                .map(|sub| sub.read_rgb(field))
                .unwrap_or_default()
        };

        Self {
            exposure,
            bloom_point: read_struct_real("bloom_point", "bloom point"),
            bloom_intensity: read_struct_real("bloom_intensity", "bloom intensity"),
            bloom_inherent: read_struct_real("bloom_inherent", "inherent bloom"),
            bling_intensity: read_struct_real("bling_intensity", "bling intensity"),
            bling_size: read_struct_real("bling_size", "bling length"),
            bling_angle_deg: read_struct_real("bling_angle", "bling angle"),
            bling_count: s
                .field("bling_count")
                .and_then(|f| f.as_struct())
                .and_then(|sub| sub.read_int_any("bling spikes"))
                .unwrap_or(0) as u8,
            bloom_large_color: read_struct_color("bloom_large_color", "large color"),
            bloom_medium_color: read_struct_color("bloom_medium_color", "medium color"),
            bloom_small_color: read_struct_color("bloom_small_color", "small color"),
        }
    }
}
