//! Dump all env-mapping-related parameters baked into an rmsh's
//! ResolvedRenderMethod. Source of truth for what tool.exe baked.
use blam_tags::file::TagFile;
use blam_tags::paths::{derive_tags_root, resolve_tag_path};
use blam_tags::render_method::{
    RenderMethod, RenderMethodDefinition, RenderMethodOption, ResolvedRenderMethod,
};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(std::env::args().nth(1).ok_or("usage: <rmsh path>")?);
    let tag = TagFile::read(&path)?;
    let rmsh = RenderMethod::from_tag(&tag)?;
    // `resolve` now needs the rmdf (for the category→option chain) and a
    // loader for each rmop tag; resolve their paths against the tags root
    // derived from the rmsh's own location.
    let tags_root = derive_tags_root(&path).ok_or("could not derive tags root from rmsh path")?;
    let rmdf_path = resolve_tag_path(&tags_root, &rmsh.definition_path, "render_method_definition");
    let rmdf = RenderMethodDefinition::from_tag(&TagFile::read(&rmdf_path)?)?;
    let load_rmop = |opt_path: &str| -> Option<RenderMethodOption> {
        let abs = resolve_tag_path(&tags_root, opt_path, "render_method_option");
        RenderMethodOption::from_tag(&TagFile::read(&abs).ok()?).ok()
    };
    let rm = ResolvedRenderMethod::resolve(&rmsh, &rmdf, load_rmop);
    let env_names = [
        "env_tint_color",
        "env_bias",
        "env_topcoat_color",
        "env_topcoat_bias",
        "env_roughness_scale",
        "environment_map_specular_contribution",
        "specular_coefficient",
        "analytical_specular_contribution",
        "area_specular_contribution",
        "diffuse_coefficient",
        "specular_mask_texture",
        "normal_specular_power",
        "glancing_specular_power",
        "normal_specular_tint",
        "glancing_specular_tint",
        "albedo_specular_tint_blend",
        "fresnel_curve_steepness",
        "analytical_anti_shadow_control",
    ];
    println!("rmsh = {}", path.display());
    println!("group_tag = {:08x}", rm.group_tag);
    println!();
    for p in &rm.parameters {
        let lc = p.name.to_lowercase();
        if env_names.iter().any(|&n| lc == n) {
            println!("  {:42} source={:?}", p.name, p.source);
        }
    }
    Ok(())
}
