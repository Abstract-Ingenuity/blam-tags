//! Smoke test for the decal tag schema port (Phase D1).
//!
//! Reads a scenario, prints its `decals[]` / `decal_palette[]` count
//! distribution, then loads each unique `.decal_system` palette entry
//! and prints a one-line summary of its `c_decal_definition` array.
//!
//! Usage:
//!   cargo run --example dump_decals -- <path/to/level.scenario>

use std::collections::HashMap;
use std::path::PathBuf;

use blam_tags::TagFile;
use blam_tags::decal_system::DecalSystem;
use blam_tags::paths::{derive_tags_root, resolve_tag_path};
use blam_tags::scenario::Scenario;

fn main() {
    let Some(path_str) = std::env::args().nth(1) else {
        eprintln!("usage: dump_decals <path/to/level.scenario>");
        std::process::exit(2);
    };
    let scnr_path = PathBuf::from(&path_str);
    let tag = TagFile::read(&scnr_path).unwrap_or_else(|e| {
        eprintln!("failed to read {}: {e}", scnr_path.display());
        std::process::exit(1);
    });
    let scnr = Scenario::from_tag(&tag).expect("Scenario::from_tag failed");

    println!("scenario: {}", scnr_path.display());
    println!("  decal_palette: [{}]", scnr.decal_palette.len());
    println!("  decals:        [{}]", scnr.decals.len());
    println!();

    let mut palette_use: HashMap<i16, usize> = HashMap::new();
    let mut scale_min = f32::INFINITY;
    let mut scale_max = f32::NEG_INFINITY;
    for d in &scnr.decals {
        *palette_use.entry(d.palette_index).or_insert(0) += 1;
        scale_min = scale_min.min(d.scale);
        scale_max = scale_max.max(d.scale);
    }
    if !scnr.decals.is_empty() {
        println!("  scale range:   [{scale_min:.3} .. {scale_max:.3}]");
        let mut histo: Vec<_> = palette_use.iter().collect();
        histo.sort_by_key(|(idx, _)| *idx);
        println!("  per-palette usage (placement count):");
        for (idx, count) in histo {
            let name = if let Some(entry) = scnr.decal_palette.get(*idx as usize) {
                short(&entry.decal_system)
            } else {
                "<oob>".to_string()
            };
            println!("    [{idx:>3}] x{count:<4} {name}");
        }
        println!();
    }

    let tags_root = derive_tags_root(&scnr_path).unwrap_or_else(|| {
        eprintln!("could not derive tags root from {}", scnr_path.display());
        std::process::exit(1);
    });

    let mut ok = 0usize;
    let mut empty = 0usize;
    let mut failed = 0usize;
    for (i, entry) in scnr.decal_palette.iter().enumerate() {
        if entry.decal_system.is_empty() {
            empty += 1;
            continue;
        }
        let path = resolve_tag_path(&tags_root, &entry.decal_system, "decal_system");
        let Ok(t) = TagFile::read(&path) else {
            eprintln!("    [{i:>3}] FAILED to read {}", path.display());
            failed += 1;
            continue;
        };
        match DecalSystem::from_tag(&t) {
            Ok(ds) => {
                ok += 1;
                println!(
                    "    [{i:>3}] decs flags=0x{:08x} max_overlap={} fade=({:.2},{:.2}) max_r={:.2} \
                     defs=[{}] {}",
                    ds.flags,
                    ds.max_overlapping,
                    ds.distance_fade_range.0,
                    ds.distance_fade_range.1,
                    ds.runtime_max_radius,
                    ds.definitions.len(),
                    short(&entry.decal_system),
                );
                for (j, def) in ds.definitions.iter().enumerate() {
                    let shader_path = def
                        .shader
                        .as_ref()
                        .map(|rm| short(&rm.definition_path))
                        .unwrap_or_else(|| "<no shader>".into());
                    println!(
                        "        def[{j}] name={} flags=0x{:08x} pass={:?} radius=({:.2},{:.2}) \
                         clamp={:.1}° cull={:.1}° rmdf={shader_path}",
                        def.name,
                        def.flags,
                        def.pass,
                        def.radius.0,
                        def.radius.1,
                        def.clamp_angle_degrees,
                        def.cull_angle_degrees,
                    );
                }
            }
            Err(e) => {
                eprintln!("    [{i:>3}] PARSE FAILED {}: {e}", path.display());
                failed += 1;
            }
        }
    }
    println!();
    println!("summary: ok={ok} empty={empty} failed={failed}");
}

fn short(path: &str) -> String {
    path.rsplit_once(['/', '\\']).map(|(_, t)| t).unwrap_or(path).to_string()
}
