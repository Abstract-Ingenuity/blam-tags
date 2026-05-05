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
use crate::math::{RealPoint3d, RealRgbColor};

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
    /// Bit 0: Enable Atmosphere; bit 1: Override Real Sun Values;
    /// bit 2: Patchy Fog.
    pub flags: u16,
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
}

impl AtmosphereSettings {
    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            flags: s.read_int_any("Flags").unwrap_or(0) as u16,
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
        }
    }

    /// Compute world-space sun direction (z-up) from pitch + heading.
    /// Returns the direction TO the sun.
    pub fn sun_direction(&self) -> RealPoint3d {
        let pitch_rad = self.sun_pitch.to_radians();
        let heading_rad = self.sun_heading.to_radians();
        let cos_p = pitch_rad.cos();
        RealPoint3d {
            x: heading_rad.sin() * cos_p,
            y: heading_rad.cos() * cos_p,
            z: pitch_rad.sin(),
        }
    }

    /// True iff the "Enable Atmosphere" flag is set.
    pub fn is_enabled(&self) -> bool {
        (self.flags & 0x0001) != 0
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
        Self { atmosphere_settings, underwater_settings }
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
