//! Render-method parameter resolution — Bungie-runtime mirror.
//!
//! This is the "walker" that takes an `rmsh` + its `rmdf` + the
//! per-category `rmop` tags and produces a flat
//! `name → ResolvedParameter` map ready for a renderer to consume.
//!
//! Mirrors the resolution layer of
//! `render_method_submit_volatile_per_node`
//! (Ares `render_method_submit.cpp:700`, IDA `0x180683AF0`) — minus
//! the rmt2 cbuffer-routing step, which is a separate pass.
//!
//! ## Resolution rules (per parameter declared by the active rmops)
//!
//! 1. If the rmop declares a non-`None` `source_extern`, the parameter
//!    is engine-bound — emit [`ParameterSource::Extern`] and the
//!    caller resolves it at draw time via [`ExternResolver`].
//! 2. Otherwise, look up the parameter by name in `rmsh.parameters`:
//!    - If present and animated, evaluate the function at `(0, 0)`
//!      (the static-resolve case; renderers that animate call
//!      [`ResolvedRenderMethod::resolve_with_time`] instead).
//!    - If present and static, take the inline `real_parameter` /
//!      `bitmap_path`.
//!    - If absent, fall back to the rmop's default.
//!
//! ## Active-options selection
//!
//! `rmsh.options[i]` is the chosen option index for `rmdf.categories[i]`.
//! When `options` is shorter than `categories` (a common case — newer
//! rmdfs add categories), missing entries default to option index 0.

use std::collections::BTreeMap;

use crate::math::ArgbColor;
use crate::tag_function::{ColorGraphType, TagFunction};

use super::types::{
    BitmapAddressMode, BitmapFilterMode, RenderMethod, RenderMethodAnimatedParameter,
    RenderMethodDefinition, RenderMethodExtern, RenderMethodOption, RenderMethodOptionParameter,
    RenderMethodParameter, RenderMethodParameterType,
};

// =============================================================================
// Output types
// =============================================================================

/// One resolved parameter — the unit a renderer consumes per-material.
#[derive(Debug, Clone)]
pub struct ResolvedParameter {
    pub name: String,
    pub parameter_type: RenderMethodParameterType,
    pub source: ParameterSource,
}

/// Either a value baked in at resolve time or an extern that the
/// renderer pulls from engine state at draw time.
#[derive(Debug, Clone)]
pub enum ParameterSource {
    /// The value is fully resolved here. The variant inside matches
    /// the parameter's `RenderMethodParameterType`.
    Inline(ResolvedValue),
    /// The runtime sources this parameter from engine state. The
    /// renderer must call its [`ExternResolver`] at draw time.
    Extern(RenderMethodExtern),
}

/// Resolved per-parameter value. The variant axis matches Bungie's
/// `e_render_method_parameter_type`.
#[derive(Debug, Clone)]
pub enum ResolvedValue {
    Bitmap(BitmapBinding),
    /// `Color` and `ArgbColor` parameters both land here. The 4 slots
    /// are A, R, G, B in `[0, 1]`; bit-pack via [`ArgbColor`] if you
    /// need the original `u32`.
    Color([f32; 4]),
    Real(f32),
    Int(i32),
    Bool(bool),
}

/// Per-bitmap-parameter binding info. Mirrors the union of fields the
/// runtime samples per-texture (path + sampler state + extern mode).
#[derive(Debug, Clone)]
pub struct BitmapBinding {
    /// Tag-relative path to the `.bitmap` (e.g.,
    /// `objects\characters\grunt\bitmaps\grunt`). Empty when
    /// `extern_texture_mode` is non-zero (the texture comes from
    /// engine state instead).
    pub bitmap_path: String,
    /// Index into the bitmap tag's images block — most rmop defaults
    /// use index 0; rmsh overrides may select an alternate image.
    pub bitmap_index: i16,
    pub filter_mode: BitmapFilterMode,
    pub address_mode: BitmapAddressMode,
    /// When non-zero, the texture is sourced from a runtime render
    /// target (camera/refraction/mirror/scope) — see
    /// `e_render_method_extern_mode`.
    pub extern_texture_mode: u8,
    /// Anisotropy override; 0 means "use sampler default".
    pub anisotropy_amount: i16,
}

// =============================================================================
// Extern resolver trait
// =============================================================================

/// Renderer-supplied resolver for engine-bound externs.
///
/// The walker produces [`ParameterSource::Extern`] entries that name
/// which extern is requested; the renderer implements this trait to
/// inject engine state (sun direction, change colors, env map, etc.)
/// at draw time.
///
/// Default impls return zero / empty so callers can selectively
/// override only the externs that matter to their pipeline.
pub trait ExternResolver {
    fn resolve_real4(&self, _ext: RenderMethodExtern) -> [f32; 4] { [0.0; 4] }
    fn resolve_real(&self, ext: RenderMethodExtern) -> f32 { self.resolve_real4(ext)[0] }
    fn resolve_int(&self, _ext: RenderMethodExtern) -> i32 { 0 }
    fn resolve_bool(&self, _ext: RenderMethodExtern) -> bool { false }
    fn resolve_bitmap(&self, _ext: RenderMethodExtern) -> Option<BitmapBinding> { None }
}

/// No-op resolver — every extern returns its default. Useful for
/// static / offline analysis where engine state isn't available.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullExternResolver;

impl ExternResolver for NullExternResolver {}

// =============================================================================
// Walker
// =============================================================================

/// A fully-resolved render_method, ready for a renderer to consume.
#[derive(Debug, Clone, Default)]
pub struct ResolvedRenderMethod {
    /// Lookup by Bungie parameter name (e.g., `"base_map"`,
    /// `"diffuse_coefficient"`). Insertion order follows the rmdf
    /// category order, then rmop parameter order — same order the
    /// runtime walks.
    pub parameters: Vec<ResolvedParameter>,
    /// FOURCC of the source rm** tag — `'rmsh'`, `'rmtr'`, `'rmw '`,
    /// etc. Threaded from `RenderMethod.group_tag`. The runtime
    /// `render_method_submit` chain ignores this (verified via
    /// dllcache decomp), but shader assemblers need it to dispatch
    /// to the right WGSL fragments. See
    /// `reference_rmtr_runtime_distinction.md`.
    pub group_tag: u32,
}

impl ResolvedRenderMethod {
    /// Static resolve — evaluates animated functions at `(input, range)
    /// = (0, 0)`, which is what the runtime does at load time and what
    /// 99% of rmsh tags actually need (constant params).
    pub fn resolve(
        rm: &RenderMethod,
        rmdf: &RenderMethodDefinition,
        load_rmop: impl FnMut(&str) -> Option<RenderMethodOption>,
    ) -> Self {
        Self::resolve_with_time(rm, rmdf, 0.0, load_rmop)
    }

    /// Time-aware resolve — evaluates animated functions at
    /// `(input, range) = (time_seconds, time_seconds)`. Color
    /// gradients still return a stub-white from
    /// [`TagFunction::evaluate_color`] until that path is implemented.
    pub fn resolve_with_time(
        rm: &RenderMethod,
        rmdf: &RenderMethodDefinition,
        time_seconds: f32,
        mut load_rmop: impl FnMut(&str) -> Option<RenderMethodOption>,
    ) -> Self {
        let mut parameters = Vec::new();
        let mut seen = BTreeMap::<String, ()>::new();

        // Walk categories in rmdf order; for each, find the chosen
        // option from rmsh.options[i] (defaults to 0 when missing),
        // load the rmop, and emit one entry per declared parameter.
        for (cat_idx, category) in rmdf.categories.iter().enumerate() {
            let opt_idx = rm.options.get(cat_idx).copied().unwrap_or(0).max(0) as usize;
            let Some(category_option) = category.options.get(opt_idx) else { continue };
            if category_option.option_path.is_empty() { continue }
            let Some(rmop) = load_rmop(&category_option.option_path) else { continue };

            for op_param in &rmop.parameters {
                if op_param.parameter_name.is_empty() { continue }
                if seen.insert(op_param.parameter_name.clone(), ()).is_some() {
                    // First declaration wins — matches the runtime
                    // behavior where `c_render_method::find_parameter_index`
                    // scans linearly and returns the first match.
                    continue;
                }
                parameters.push(resolve_one(rm, op_param, time_seconds));
            }
        }

        Self { parameters, group_tag: rm.group_tag }
    }

    /// O(N) lookup by Bungie parameter name. For renderers that need
    /// random-access by-name, build their own `HashMap` once.
    pub fn find(&self, name: &str) -> Option<&ResolvedParameter> {
        self.parameters.iter().find(|p| p.name == name)
    }
}

/// Build the flat rmop parameter list for a `(rmsh, rmdf)` pair —
/// equivalent to `c_render_method_definition::build_parameter_list
/// @ 0x826E31D8` (Reach tag-debug). Walks the rmdf's categories in
/// declared order, loads each chosen rmop, and concatenates all
/// rmop parameters. The result is what
/// [`crate::render_method::cbuffer::compile_real_constant`] queries by
/// name to apply Stage 1 rmop defaults.
///
/// Note: differs from [`ResolvedRenderMethod::resolve`] which DEDUPES
/// by name (first-wins). The cache builder DOES allow duplicates
/// (multiple rmops in the chain may declare the same parameter name);
/// the lookup at slot-time naturally takes the first match by linear
/// scan, which matches the engine's `find_parameter_list_entry_by_name`.
pub fn build_rmop_param_list(
    rm: &RenderMethod,
    rmdf: &RenderMethodDefinition,
    mut load_rmop: impl FnMut(&str) -> Option<RenderMethodOption>,
) -> Vec<RenderMethodOptionParameter> {
    let mut out: Vec<RenderMethodOptionParameter> = Vec::new();
    for (cat_idx, category) in rmdf.categories.iter().enumerate() {
        let opt_idx = rm.options.get(cat_idx).copied().unwrap_or(0).max(0) as usize;
        let Some(category_option) = category.options.get(opt_idx) else { continue };
        if category_option.option_path.is_empty() { continue }
        let Some(rmop) = load_rmop(&category_option.option_path) else { continue };
        for op_param in rmop.parameters {
            if op_param.parameter_name.is_empty() { continue }
            out.push(op_param);
        }
    }
    out
}

// =============================================================================
// Per-parameter resolution
// =============================================================================

fn resolve_one(
    rm: &RenderMethod,
    op_param: &RenderMethodOptionParameter,
    time_seconds: f32,
) -> ResolvedParameter {
    let parameter_type = op_param.parameter_type.unwrap_or(RenderMethodParameterType::Real);

    // 1. Extern wins.
    if let Some(ext) = op_param.source_extern {
        if ext != RenderMethodExtern::None {
            return ResolvedParameter {
                name: op_param.parameter_name.clone(),
                parameter_type,
                source: ParameterSource::Extern(ext),
            };
        }
    }

    // 2. rmsh override by name.
    let rm_param = rm.parameters.iter().find(|p| p.parameter_name == op_param.parameter_name);

    let value = match parameter_type {
        RenderMethodParameterType::Bitmap => {
            ResolvedValue::Bitmap(resolve_bitmap(op_param, rm_param))
        }
        RenderMethodParameterType::Color | RenderMethodParameterType::ArgbColor => {
            ResolvedValue::Color(resolve_color(op_param, rm_param, time_seconds))
        }
        RenderMethodParameterType::Real => {
            ResolvedValue::Real(resolve_real(op_param, rm_param, time_seconds))
        }
        RenderMethodParameterType::Int => {
            ResolvedValue::Int(resolve_int(op_param, rm_param))
        }
        RenderMethodParameterType::Bool => {
            ResolvedValue::Bool(resolve_bool(op_param, rm_param))
        }
    };

    ResolvedParameter {
        name: op_param.parameter_name.clone(),
        parameter_type,
        source: ParameterSource::Inline(value),
    }
}

fn resolve_bitmap(
    op_param: &RenderMethodOptionParameter,
    rm_param: Option<&RenderMethodParameter>,
) -> BitmapBinding {
    // rmsh override: any non-empty bitmap_path wins; sampler state
    // overrides only when the rmsh actually supplies non-zero values
    // (the schema's "0" defaults map to "use rmop's value").
    let path = rm_param
        .map(|p| p.bitmap_path.as_str())
        .filter(|p| !p.is_empty())
        .unwrap_or(op_param.default_bitmap_path.as_str())
        .to_string();

    let bitmap_index = rm_param
        .map(|p| if p.bitmap_extern_mode != 0 { 0 } else { 0 }) // schema doesn't carry index in rmsh
        .unwrap_or(0);

    let filter_mode = rm_param
        .and_then(|p| BitmapFilterMode::from_index(p.bitmap_filter_mode as i64))
        .unwrap_or(op_param.default_filter_mode);
    let address_mode = rm_param
        .and_then(|p| BitmapAddressMode::from_index(p.bitmap_address_mode as i64))
        .unwrap_or(op_param.default_address_mode);
    let extern_texture_mode = rm_param
        .map(|p| p.bitmap_extern_mode as u8)
        .unwrap_or(0);
    let anisotropy_amount = rm_param
        .map(|p| p.bitmap_anisotropy_amount)
        .unwrap_or(op_param.anisotropy_amount);

    BitmapBinding { bitmap_path: path, bitmap_index, filter_mode, address_mode, extern_texture_mode, anisotropy_amount }
}

fn resolve_real(
    op_param: &RenderMethodOptionParameter,
    rm_param: Option<&RenderMethodParameter>,
    time_seconds: f32,
) -> f32 {
    if let Some(p) = rm_param {
        // Animated parameter index 0 holds the value source — the runtime
        // never reads `real_parameter` for material params; that field is
        // a legacy fallback. Walk animated parameters first.
        if let Some(anim) = first_value_function(&p.animated_parameters) {
            return anim.evaluate(time_seconds, time_seconds);
        }
        // Static fallback for tags without animated_parameters at all.
        if p.real_parameter != 0.0 {
            return p.real_parameter;
        }
    }
    op_param.default_real_value
}

fn resolve_int(
    op_param: &RenderMethodOptionParameter,
    rm_param: Option<&RenderMethodParameter>,
) -> i32 {
    if let Some(p) = rm_param {
        if p.int_parameter != 0 {
            return p.int_parameter;
        }
    }
    op_param.default_int_bool_value
}

fn resolve_bool(
    op_param: &RenderMethodOptionParameter,
    rm_param: Option<&RenderMethodParameter>,
) -> bool {
    if let Some(p) = rm_param {
        if p.int_parameter != 0 {
            return p.int_parameter != 0;
        }
    }
    op_param.default_int_bool_value != 0
}

fn resolve_color(
    op_param: &RenderMethodOptionParameter,
    rm_param: Option<&RenderMethodParameter>,
    _time_seconds: f32,
) -> [f32; 4] {
    if let Some(p) = rm_param {
        if let Some(anim) = first_color_function(&p.animated_parameters) {
            if let Some(c) = anim.function.as_ref().and_then(extract_first_color) {
                return c;
            }
        }
    }
    argb_u32_to_f32(op_param.default_color)
}

// ---- TagFunction helpers ----

fn first_value_function(anims: &[RenderMethodAnimatedParameter]) -> Option<&TagFunction> {
    // Most material scalars have a single Value-typed animated_parameters[0].
    anims.first().and_then(|a| a.function.as_ref())
}

fn first_color_function(anims: &[RenderMethodAnimatedParameter]) -> Option<&RenderMethodAnimatedParameter> {
    anims.iter().find(|a| {
        a.function.as_ref().is_some_and(|f| f.color_graph_type() != ColorGraphType::Scalar)
    })
}

/// Extract the first (constant) color from a TagFunction's color graph,
/// returned as `[r, g, b, a]` in [0, 1]. Halo packs each entry as a
/// u32 ARGB; we unpack and normalize.
///
/// **Alpha defaults to 1.0** — the .shader tag stores the alpha byte
/// as 0 because the color-picker UI doesn't touch alpha for plain
/// color params (`normal_specular_tint`, `albedo_color`, etc.). Halo's
/// cache builder sources alpha from a SEPARATE `animated[Alpha]`
/// function (per `reference_canonical_cbuffer_merge.md`: "Color via
/// animated[Color] + animated[Alpha]") when authored, else defaults
/// to 1.0. We don't yet read animated[Alpha] from a second
/// animated_parameters entry; the 1.0 default matches TagTool's
/// `SetArgument <name> R G B 1` output for unauthored alphas.
fn extract_first_color(f: &TagFunction) -> Option<[f32; 4]> {
    if f.color_graph_type() == ColorGraphType::Scalar {
        return None;
    }
    let packed = f.header().colors[0];
    let mut rgba = argb_u32_to_f32(ArgbColor(packed));
    rgba[3] = 1.0;
    Some(rgba)
}

fn argb_u32_to_f32(c: ArgbColor) -> [f32; 4] {
    let v = c.0;
    let a = ((v >> 24) & 0xff) as f32 / 255.0;
    let r = ((v >> 16) & 0xff) as f32 / 255.0;
    let g = ((v >>  8) & 0xff) as f32 / 255.0;
    let b = ((v      ) & 0xff) as f32 / 255.0;
    [r, g, b, a]
}
