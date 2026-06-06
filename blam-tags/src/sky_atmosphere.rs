//! `sky_atm_parameters` (`skya`) tag walker — atmospheric scattering
//! parameters consumed by `compute_scattering` in atmosphere_fx.hlsl.
//!
//! Pointed at by `scenario.atmospheric` tag-ref. Holds 4 named
//! atmosphere settings (e.g. "haze_level", "haze_skydome") plus
//! underwater fog settings.
//!
//! Schema reference: `Ares/source/atmosphere/atmosphere_definitions.h`
//! and inspection of `riverworld.sky_atm_parameters`.

use crate::api::TagStruct;
use crate::file::TagFile;
use crate::math::{RealPoint3d, RealRgbColor, RealVector3d};
use crate::typed_enums::Flags;

/// `atmosphere_flags` (`sky_atm_parameters.json`). Discriminants are the
/// canonical bit indices.
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u16)]
pub enum AtmosphereFlags {
    #[strum(serialize = "Enable Atmosphere")] EnableAtmosphere = 0,
    #[strum(serialize = "Override Real Sun Values")] OverrideRealSunValues = 1,
    #[strum(serialize = "Patchy Fog")] PatchyFog = 2,
}

/// Errors from sky_atm_parameters walking.
#[derive(Debug)]
pub enum SkyAtmosphereError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
}

impl std::fmt::Display for SkyAtmosphereError {
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

impl std::error::Error for SkyAtmosphereError {}

const SKY_ATM_GROUP: [u8; 4] = *b"skya";

/// One named atmosphere setting — one element of the
/// `atmosphere settings[4]` block.
#[derive(Debug, Clone, Default)]
pub struct AtmosphereSettings {
    /// `atmosphere_flags`. Test with `.contains(AtmosphereFlags::*)`.
    pub flags: Flags<AtmosphereFlags, u16>,
    /// Setting name, e.g. "haze_level", "haze_skydome".
    pub name: String,
    /// Sun pitch above horizon, degrees [0, 90].
    pub sun_pitch: f32,
    /// Sun heading azimuth, degrees [0, 360].
    pub sun_heading: f32,
    /// Sun / sky color (linear RGB).
    pub color: RealRgbColor,
    /// Sun intensity scalar.
    pub intensity: f32,
    /// World z reference plane for Mie/Rayleigh height stratification.
    pub sea_level: f32,
    /// Rayleigh density falloff height (world units).
    pub rayleigh_height_scale: f32,
    /// Mie density falloff height (world units).
    pub mie_height_scale: f32,
    /// Rayleigh scattering coefficient multiplier.
    pub rayleigh_multiplier: f32,
    /// Mie scattering coefficient multiplier.
    pub mie_multiplier: f32,
    /// Henyey-Greenstein phase asymmetry parameter (g).
    pub sun_phase_function: f32,
    /// De-saturation amount applied to inscatter (0..1).
    pub desaturation: f32,
    /// Distance bias added before fog computation.
    pub distance_bias: f32,
    /// Maximum fog thickness clamp.
    pub max_fog_thickness: f32,

    // --- Per-setting patchy fog ---
    // Engine `c_atmosphere_setting`. Accumulated by
    // `c_atmosphere_fog_interface::accumulate_atmosphere_settings @
    // 0x1803AFD90` only when the setting's `flags & 4` (bit 2 =
    // "Patchy Fog") is set. Zero-default when unauthored.
    /// Schema "Sheet density" → engine `m_patchy_fog_density`. Per-
    /// sheet density multiplier; PS uses it as `sheet_fade_factors *=
    /// patchy_fog_density`.
    pub patchy_fog_density: f32,
    /// Height (world z) above which fog density starts decaying.
    pub full_intensity_height: f32,
    /// Height where fog has half its full-intensity density. Drives
    /// the exponential height fade `exp(rate × (full - z))`.
    pub half_intensity_height: f32,
    /// World-space wind direction. PS-side accumulated into per-sheet
    /// UV offsets each frame.
    pub wind_direction: RealVector3d,

    /// Schema "Weather effect" → engine `m_effect_tag` @ +0x08C.
    /// Particle-system tag reference (raindrops, snowflakes) that
    /// follows the camera and wraps seamlessly. Empty string when
    /// unauthored. Not currently rendered — Phase 8 of the atmosphere
    /// port plan.
    pub weather_effect: String,

    /// Runtime scratch field — engine `c_atmosphere_setting::m_weight`
    /// @ +0x09C. Populated each frame by
    /// `c_atmosphere_fog_interface::compute_cluster_weights` based on
    /// the camera-to-cluster distance and `cluster_search_radius` /
    /// `falloff_start_distance` / `distance_falloff_power`. Consumed
    /// by `accumulate_atmosphere_settings` as the per-setting blend
    /// weight. Zero-default from the parser; Phase 3 will mutate it
    /// in-place per frame.
    pub weight: f32,

    /// Runtime scratch field — engine `c_atmosphere_setting::m_effect_weight`
    /// @ +0x0A0. Per-setting weight for the weather-effect particle
    /// system. Zero-default; populated by `compute_cluster_weights`
    /// alongside `weight`.
    pub effect_weight: f32,
}

impl AtmosphereSettings {
    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            flags: s.try_read_flags("Flags").unwrap_or_default(),
            name: s.read_string_id("Name").unwrap_or_default(),
            sun_pitch: s.read_real("Pitch [0 to 90]").unwrap_or(45.0),
            sun_heading: s.read_real("Heading [0 to 360]").unwrap_or(0.0),
            color: s.read_rgb("Color"),
            intensity: s.read_real("Intensity").unwrap_or(1.0),
            sea_level: s.read_real("Sea Level").unwrap_or(0.0),
            rayleigh_height_scale: s.read_real("Rayleign Height Scale").unwrap_or(150.0),
            mie_height_scale: s.read_real("Mie Height Scale").unwrap_or(6.0),
            rayleigh_multiplier: s.read_real("Rayleigh Multiplier").unwrap_or(0.05),
            mie_multiplier: s.read_real("Mie Multiplier").unwrap_or(0.025),
            sun_phase_function: s.read_real("Sun Phase Function").unwrap_or(0.2),
            desaturation: s.read_real("De-saturation").unwrap_or(0.0),
            distance_bias: s.read_real("Distance Bias").unwrap_or(0.0),
            max_fog_thickness: s.read_real("Max Fog Thickness").unwrap_or(65536.0),
            patchy_fog_density: s.read_real("Sheet density").unwrap_or(0.0),
            full_intensity_height: s.read_real("Full intensity height").unwrap_or(0.0),
            half_intensity_height: s.read_real("Half intensity height").unwrap_or(0.0),
            wind_direction: s.read_vec3("Wind direction"),
            weather_effect: s.read_tag_ref_path("Weather effect").unwrap_or_default(),
            // Runtime scratch — zero from the parser; Phase 3 of the
            // atmosphere port plan populates these via compute_cluster_weights.
            weight: 0.0,
            effect_weight: 0.0,
        }
    }

    /// True iff the "Patchy Fog" flag (bit 2 / mask 4) is set. Engine:
    /// `c_atmosphere_fog_interface::accumulate_atmosphere_settings @
    /// 0x1803AFD90` gates the patchy-fog field accumulation on this bit.
    pub fn has_patchy_fog(&self) -> bool {
        self.flags.contains(AtmosphereFlags::PatchyFog)
    }

    /// Compute world-space sun direction (z-up) from pitch + heading.
    /// Returns the direction TO the sun.
    ///
    /// Engine `c_atmosphere_fog_interface::get_sun_parameters @ 0x1803AF990`
    /// override branch:
    /// ```c
    /// sun_direction.x = cos(phi_rad) * sin(theta_rad);   // phi  = m_dominant_light_phi   = "Heading"
    /// sun_direction.y = sin(phi_rad) * sin(theta_rad);   // theta = m_dominant_light_theta = "Pitch"
    /// sun_direction.z = cos(theta_rad);
    /// ```
    /// Pitch is interpreted as a **polar angle from zenith** (astronomical
    /// "zenith angle"), NOT elevation from horizon — pitch=0 puts the sun
    /// straight up, pitch=90 puts it on the horizon. The schema label
    /// "Pitch [0 to 90]" is misleading; engine field name is
    /// `m_dominant_light_theta` (theta = polar). An earlier port treated
    /// pitch as elevation, which inverted z and shifted x/y by 90°.
    pub fn sun_direction(&self) -> RealPoint3d {
        let phi_rad   = self.sun_heading.to_radians();
        let theta_rad = self.sun_pitch.to_radians();
        let sin_theta = theta_rad.sin();
        RealPoint3d {
            x: phi_rad.cos() * sin_theta,
            y: phi_rad.sin() * sin_theta,
            z: theta_rad.cos(),
        }
    }

    /// True iff the "Enable Atmosphere" flag is set.
    pub fn is_enabled(&self) -> bool {
        self.flags.contains(AtmosphereFlags::EnableAtmosphere)
    }
}

/// One underwater setting — water_physics_NN names + murkiness + fog
/// color. Used by water_fx.hlsl when `k_is_camera_underwater`.
#[derive(Debug, Clone, Default)]
pub struct UnderwaterSettings {
    pub name: String,
    pub murkiness: f32,
    pub fog_color: RealRgbColor,
}

impl UnderwaterSettings {
    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            name: s.read_string_id("Name").unwrap_or_default(),
            murkiness: s.read_real("Murkiness").unwrap_or(1.0),
            fog_color: s.read_rgb("Fog Color"),
        }
    }
}

/// Decoded `sky_atm_parameters` tag.
#[derive(Debug, Clone, Default)]
pub struct SkyAtmosphere {
    /// Indexed by name; typically 4 elements (haze_level, haze_skydome,
    /// not_used, haze_skydome_alt).
    pub atmosphere_settings: Vec<AtmosphereSettings>,
    /// Per-water-physics-volume underwater fog settings.
    pub underwater_settings: Vec<UnderwaterSettings>,

    // --- Global patchy fog parameters ---
    // Per-tag (not per-cluster). Engine reads via global pointer
    // `c_atmosphere_fog_interface::get_global_atmosphere_parameters()`.
    /// Relative path to the noise bitmap used by `patchy_fog.hlsl`.
    /// Empty string = no patchy fog texture authored.
    pub patchy_fog_texture: String,
    /// `g_shadow_pixel_size`-style texture-space repeat applied to the
    /// noise bitmap UVs.
    pub texture_repeat_rate: f32,
    /// World-space spacing between adjacent fog sheets along the camera
    /// forward axis. Engine default order ~10 wu.
    pub distance_between_sheets: f32,
    /// Higher values = sharper near-surface fade (PS `1 -
    /// exp(-depth_diff × depth_fade_factor)`).
    pub depth_fade_factor: f32,
    /// World-units search radius for cross-fading neighbouring cluster
    /// settings. Default 25.
    pub cluster_search_radius: f32,
    /// World-units distance from cluster boundary at which influence
    /// starts fading. Default 5.
    pub falloff_start_distance: f32,
    /// Power applied to the cluster influence falloff curve. Default 2.
    pub distance_falloff_power: f32,
    /// World-units depth at which the patchy-fog effect is sorted into
    /// the transparency renderer (`c_player_view::queue_patchy_fog`).
    pub transparent_sort_distance: f32,
    /// `e_global_sort_layer` enum byte. 0 means "use _normal" per
    /// `c_player_view::queue_patchy_fog`.
    pub transparent_sort_layer: u8,
}

impl SkyAtmosphere {
    pub fn from_tag(tag: &TagFile) -> Result<Self, SkyAtmosphereError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != SKY_ATM_GROUP {
            return Err(SkyAtmosphereError::WrongGroup { expected: SKY_ATM_GROUP, actual });
        }
        Ok(Self::from_struct(&tag.root()))
    }

    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        let atmosphere_settings = s
            .field("atmosphere settings")
            .and_then(|f| f.as_block())
            .map(|b| {
                let mut out = Vec::with_capacity(b.len());
                for i in 0..b.len() {
                    if let Some(elem) = b.element(i) {
                        out.push(AtmosphereSettings::from_struct(&elem));
                    }
                }
                out
            })
            .unwrap_or_default();
        let underwater_settings = s
            .field("underwater settings")
            .and_then(|f| f.as_block())
            .map(|b| {
                let mut out = Vec::with_capacity(b.len());
                for i in 0..b.len() {
                    if let Some(elem) = b.element(i) {
                        out.push(UnderwaterSettings::from_struct(&elem));
                    }
                }
                out
            })
            .unwrap_or_default();
        let patchy_fog_texture = s
            .read_tag_ref_path("Fog Bitmap")
            .unwrap_or_default();
        let texture_repeat_rate = s.read_real("Texture repeat rate").unwrap_or(1.0);
        let distance_between_sheets = s.read_real("Distance between sheets").unwrap_or(10.0);
        let depth_fade_factor = s.read_real("Depth fade factor").unwrap_or(1.0);
        let cluster_search_radius = s.read_real("Cluster search radius").unwrap_or(25.0);
        let falloff_start_distance = s.read_real("Falloff start distance").unwrap_or(5.0);
        let distance_falloff_power = s.read_real("Distance falloff power").unwrap_or(2.0);
        let transparent_sort_distance = s.read_real("Transparent sort distance").unwrap_or(100.0);
        let transparent_sort_layer = s
            .read_int_any("Transparent sort layer")
            .unwrap_or(0) as u8;
        Self {
            atmosphere_settings,
            underwater_settings,
            patchy_fog_texture,
            texture_repeat_rate,
            distance_between_sheets,
            depth_fade_factor,
            cluster_search_radius,
            falloff_start_distance,
            distance_falloff_power,
            transparent_sort_distance,
            transparent_sort_layer,
        }
    }

    /// Find the first enabled atmosphere setting (Enable Atmosphere
    /// flag set). Falls back to the first element if none are flagged
    /// — some scenarios have all flags set.
    pub fn primary_setting(&self) -> Option<&AtmosphereSettings> {
        self.atmosphere_settings
            .iter()
            .find(|s| s.is_enabled())
            .or_else(|| self.atmosphere_settings.first())
    }
}
