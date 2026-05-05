//! Resolves the pixel user cbuffer for grunt's albedo entry point and
//! prints the layout + values. Verifies the routing model end-to-end.
//!
//! Usage:
//!   cargo run --example probe_cbuffer

use std::path::PathBuf;

use blam_tags::TagFile;
use blam_tags::render_method::{
    resolve_pixel_user_cbuffer, RenderMethod, RenderMethodTemplate,
};

fn main() {
    let tags_root = std::env::var("HALO3_TAGS")
        .unwrap_or_else(|_| "/Users/camden/Halo/halo3_mcc/tags".into());

    let rmsh_path: PathBuf = [
        &tags_root,
        "objects/characters/grunt/shaders/grunt_armor.shader",
    ]
    .iter()
    .collect();

    let rmsh_tag = TagFile::read(&rmsh_path).expect("read rmsh");
    let rmsh = RenderMethod::from_tag(&rmsh_tag).expect("parse rmsh");

    let template_path = rmsh
        .postprocess_definition
        .as_ref()
        .map(|pp| pp.template_path.as_str())
        .filter(|s| !s.is_empty())
        .expect("postprocess.template missing");

    // Halo paths use backslashes; on disk they're forward slashes.
    // The rmsh's stored template path may be truncated (older schema
    // dropped trailing zero options) — fall back to a prefix glob.
    let normalized = template_path.replace('\\', "/");
    let exact: PathBuf =
        [&tags_root, &format!("{}.render_method_template", normalized)]
            .iter()
            .collect();
    let rmt2_full: PathBuf = if exact.exists() {
        exact
    } else {
        // Strip trailing _0 segments and pad until a match is found.
        let dir = exact.parent().unwrap().to_path_buf();
        let stem_prefix = exact
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .trim_end_matches("_0")
            .to_string();
        let mut found = None;
        for entry in std::fs::read_dir(&dir).expect("read shader_templates dir") {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("render_method_template") {
                continue;
            }
            let stem = path.file_stem().unwrap().to_string_lossy().to_string();
            if stem.starts_with(&stem_prefix) && stem.trim_start_matches(&stem_prefix).chars().all(|c| c == '_' || c == '0') {
                found = Some(path);
                break;
            }
        }
        found.unwrap_or_else(|| {
            panic!("no rmt2 matching prefix '{}' in {}", stem_prefix, dir.display())
        })
    };
    eprintln!("rmt2: {}", rmt2_full.display());
    let rmt2_tag = TagFile::read(&rmt2_full).expect("read rmt2");
    let rmt2 = RenderMethodTemplate::from_tag(&rmt2_tag).expect("parse rmt2");

    println!("rmt2: {}", template_path);
    println!("  passes:               {}", rmt2.passes.len());
    println!("  routing_info:         {}", rmt2.routing_info.len());
    println!("  float_constants:      {}", rmt2.float_constants.len());
    for (i, name) in rmt2.float_constants.iter().enumerate() {
        println!("    [{}] {}", i, name);
    }

    println!();
    println!("rmsh.parameters:");
    for p in &rmsh.parameters {
        println!(
            "  {:24} type={:?} animated=[{}] real={}",
            p.parameter_name,
            p.parameter_type,
            p.animated_parameters.len(),
            p.real_parameter,
        );
    }

    println!();
    let cb = resolve_pixel_user_cbuffer(&rmsh, &rmt2);
    println!("cbuffer:");
    println!("  total_bytes: {}", cb.total_bytes);
    println!("  slots:");
    for slot in &cb.slots {
        println!(
            "    [{:>3}] src='{}' xform={} value={:?}",
            slot.byte_offset / 16,
            slot.source_name,
            slot.is_xform,
            slot.value,
        );
    }
}
