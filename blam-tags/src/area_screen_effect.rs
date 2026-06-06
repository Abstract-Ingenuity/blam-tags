//! `area_screen_effect` (`sefc`) tag walker — runtime screen-effect
//! collection authored as a tag and applied either via the scenario's
//! `global_screen_effect` reference (always at falloff = 1.0) or via
//! a placed `screen_effect_data` runtime instance (per-instance falloff
//! evaluated against distance / age / view-angle).
//!
//! Drives the engine's per-frame composite hue/saturation/contrast/gamma
//! pipeline. Reference path:
//!
//! 1. **`screen_effect_sample @ 0x1803A4E90`** — per-frame walker that
//!    accumulates active 'sefc' instances into an `s_screen_effect_settings`
//!    via `accumulate_settings @ 0x1803A4600` (per-field MAX, except
//!    `color_filter` which is MIN-blend toward identity).
//!
//! 2. **`c_screen_postprocess::postprocess_player_view @ 0x1806B4160`** —
//!    consumes the accumulated settings to drive
//!    `c_hue_saturation_control::set_hue_saturaton_and_color_filters @ 0x1806956B0`
//!    (Reach `0x826A78F0` is authoritative — MCC stripped color_filter/floor).
//!
//! Schema reference: `definitions/halo3_mcc/area_screen_effect.json`.
//! IDA cross-checks: `s_single_screen_effect_definition` size 132,
//! `s_area_screen_effect_definition` size 12.

use crate::api::TagStruct;
use crate::fields::TagFieldType;
use crate::file::TagFile;
use crate::math::RealRgbColor;
use crate::tag_function::TagFunction;
use crate::typed_enums::Flags;

/// Errors from `area_screen_effect` tag walking.
#[derive(Debug)]
pub enum AreaScreenEffectError {
    WrongGroup { expected: [u8; 4], actual: [u8; 4] },
}

impl std::fmt::Display for AreaScreenEffectError {
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

impl std::error::Error for AreaScreenEffectError {}

const SEFC_GROUP: [u8; 4] = *b"sefc";

/// `area_screen_effect_flags_definition` — 4 bits authored on each
/// `single_screen_effect`.
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum AreaScreenEffectFlags {
    #[strum(serialize = "debug disable")] DebugDisable = 0,
    #[strum(serialize = "allow effect outside radius")] AllowEffectOutsideRadius = 1,
    #[strum(serialize = "first person only")] FirstPersonOnly = 2,
    #[strum(serialize = "third person only")] ThirdPersonOnly = 3,
}

/// One `single_screen_effect` (132B authored) — one entry of the
/// area_screen_effect's `screen_effects` block. A tag carries up to 8.
#[derive(Debug, Clone, Default)]
pub struct SingleScreenEffect {
    /// `string_id` authored name. Empty when unauthored.
    pub name: String,

    /// `area_screen_effect_flags_definition`.
    pub flags: Flags<AreaScreenEffectFlags, u32>,

    /// World-units distance at which the effect is fully attenuated.
    /// Engine: `*((float*)effect + 2)`. Not used for the scenario-global
    /// effect (which always applies at falloff = 1.0).
    pub maximum_distance: f32,

    /// `distance_falloff` curve — input is `distance / maximum_distance`
    /// in `[0, 1]`, output is the per-instance falloff multiplier. `None`
    /// when the curve blob is missing; engine-equivalent of "constant
    /// 1.0" (falloff is fully on at every distance within the radius).
    pub distance_falloff: Option<TagFunction>,

    /// Authored lifetime in seconds. `0` = effect never dies. The engine
    /// gates `effect.lifetime > 0 && effect.lifetime < datum.age` to
    /// expire instances.
    pub lifetime: f32,

    /// `time_falloff` curve — input is `age / lifetime` in `[0, 1]`.
    pub time_falloff: Option<TagFunction>,

    /// `angle_falloff` curve — input is the angle (in radians or a
    /// normalized `[0, 1]` mapping; verify at Phase 2 wire-up against
    /// `s_single_screen_effect_definition::evaluate_falloff`) between
    /// the camera forward vector and the vector from camera to instance.
    pub angle_falloff: Option<TagFunction>,

    /// `s_screen_effect_settings` payload — the actual hue/sat/contrast
    /// values fed into the runtime accumulator. Defaults to identity.
    pub settings: ScreenEffectSettings,
}

/// `s_screen_effect_settings` (56 bytes per Ares) — the payload
/// embedded in each `single_screen_effect`. Scalar fields default to 0;
/// `color_filter` defaults to `(1, 1, 1)`; `color_floor` defaults to
/// `(0, 0, 0)`. With those defaults the engine's
/// `set_hue_saturaton_and_color_filters` produces the identity matrix.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ScreenEffectSettings {
    pub exposure_boost: f32,
    pub hue_left: f32,
    pub hue_right: f32,
    pub saturate: f32,
    pub desaturate: f32,
    pub contrast_enhance: f32,
    pub gamma_enhance: f32,
    pub gamma_reduce: f32,
    pub color_filter: RealRgbColor,
    pub color_floor: RealRgbColor,
}

impl ScreenEffectSettings {
    /// Mirrors `s_screen_effect_settings::set_defaults @ 0x1803A45A0` —
    /// zeroes scalars; `color_filter = (1, 1, 1)`; `color_floor = (0, 0, 0)`.
    pub fn defaults() -> Self {
        Self {
            color_filter: RealRgbColor { red: 1.0, green: 1.0, blue: 1.0 },
            ..Self::default()
        }
    }
}

/// Top-level walked `area_screen_effect_struct_definition`. Owns up to
/// 8 `single_screen_effect` entries.
#[derive(Debug, Clone, Default)]
pub struct AreaScreenEffect {
    pub effects: Vec<SingleScreenEffect>,
}

impl AreaScreenEffect {
    pub fn from_tag(tag: &TagFile) -> Result<Self, AreaScreenEffectError> {
        let actual = tag.group().tag.to_be_bytes();
        if actual != SEFC_GROUP {
            return Err(AreaScreenEffectError::WrongGroup {
                expected: SEFC_GROUP,
                actual,
            });
        }
        Ok(Self::from_struct(&tag.root()))
    }

    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        let effects = s
            .field("screen effects")
            .and_then(|f| f.as_block())
            .map(|b| {
                let mut out = Vec::with_capacity(b.len());
                for i in 0..b.len() {
                    if let Some(entry) = b.element(i) {
                        out.push(SingleScreenEffect::from_struct(&entry));
                    }
                }
                out
            })
            .unwrap_or_default();
        Self { effects }
    }
}

impl SingleScreenEffect {
    pub fn from_struct(s: &TagStruct<'_>) -> Self {
        let name = s.read_string_id("name").unwrap_or_default();
        let flags = s.try_read_flags("flags").unwrap_or_default();

        let maximum_distance = s.read_real("maximum distance").unwrap_or(0.0);
        let distance_falloff = read_falloff(s, "distance falloff");

        let lifetime = s.read_real("lifetime").unwrap_or(0.0);
        let time_falloff = read_falloff(s, "time falloff");

        let angle_falloff = read_falloff(s, "angle falloff");

        let settings = ScreenEffectSettings {
            exposure_boost: s.read_real("exposure boost").unwrap_or(0.0),
            hue_left: s.read_real("hue left").unwrap_or(0.0),
            hue_right: s.read_real("hue right").unwrap_or(0.0),
            saturate: s.read_real("saturation").unwrap_or(0.0),
            desaturate: s.read_real("desaturation").unwrap_or(0.0),
            contrast_enhance: s.read_real("contrast enhance").unwrap_or(0.0),
            gamma_enhance: s.read_real("gamma enhance").unwrap_or(0.0),
            gamma_reduce: s.read_real("gamma reduce").unwrap_or(0.0),
            // `read_rgb` returns (0,0,0) when missing; sefc authors set
            // color_filter explicitly (the runtime accumulator's identity
            // baseline `(1,1,1)` lives in `ScreenEffectSettings::defaults`,
            // not at this walker level).
            color_filter: s.read_rgb("color filter"),
            color_floor: s.read_rgb("color floor"),
        };

        Self {
            name,
            flags,
            maximum_distance,
            distance_falloff,
            lifetime,
            time_falloff,
            angle_falloff,
            settings,
        }
    }

    /// True if `flags & first_person_only` is set. Engine path skips
    /// the effect for non-first-person observers in this case.
    pub fn first_person_only(&self) -> bool {
        self.flags.contains(AreaScreenEffectFlags::FirstPersonOnly)
    }

    /// True if `flags & third_person_only` is set.
    pub fn third_person_only(&self) -> bool {
        self.flags.contains(AreaScreenEffectFlags::ThirdPersonOnly)
    }

    /// True if `flags & debug_disable` — engine skips this effect.
    pub fn debug_disabled(&self) -> bool {
        self.flags.contains(AreaScreenEffectFlags::DebugDisable)
    }
}

/// Walk a `screen_effect_scalar_function_struct` field and return the
/// authored [`TagFunction`], or `None` when the curve blob is missing.
/// Caller treats `None` as constant-1.0 (fully-on falloff).
///
/// The schema declares two same-named "Mapping" fields — a `custom`
/// marker (group_tag `fned`) followed by the real `mapping_function`
/// struct. We walk by type instead of name to skip the marker.
/// (Same trick `light.rs::inner_mapping_function` uses.)
fn read_falloff(parent: &TagStruct<'_>, name: &str) -> Option<TagFunction> {
    let outer = parent.field(name).and_then(|f| f.as_struct())?;
    let mapping = outer
        .fields()
        .find(|f| f.field_type() == TagFieldType::Struct)?
        .as_struct()?;
    mapping.field("data").and_then(|f| f.as_function())
}

