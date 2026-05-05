//! Dump a `scenario_lightmap_bsp_data` tag.
//!
//! Usage:
//!   cargo run --example dump_lightmap -- <path/to/foo.scenario_lightmap_bsp_data>

use std::path::PathBuf;

use blam_tags::TagFile;
use blam_tags::scenario_lightmap::{LightmapBspData, LightmapPolicy};

fn main() {
    let Some(path_str) = std::env::args().nth(1) else {
        eprintln!("usage: dump_lightmap <path/to/foo.scenario_lightmap_bsp_data>");
        std::process::exit(2);
    };
    let path = PathBuf::from(&path_str);
    let tag = TagFile::read(&path).unwrap_or_else(|e| {
        eprintln!("failed to read {}: {e}", path.display());
        std::process::exit(1);
    });
    let lm = LightmapBspData::from_tag(&tag).expect("LightmapBspData::from_tag failed");

    println!("scenario_lightmap_bsp_data: {}", path.display());
    println!("  flags:                 0x{:04x}", lm.flags);
    println!("  bsp_reference_index:   {}", lm.bsp_reference_index);
    println!("  bsp_import_checksum:   0x{:08x}", lm.structure_bsp_import_checksum as u32);
    println!("  lightprobe_texture:    {}", lm.lightprobe_texture);
    println!("  dominant_light_tex:    {}", lm.dominant_light_intensity_texture);
    println!();

    println!("compression_vectors[18]:");
    for (i, v) in lm.compression_vectors.iter().enumerate() {
        if v.i.abs() > 1e-6 || v.j.abs() > 1e-6 || v.k.abs() > 1e-6 {
            println!("  [{i:2}] ({:.4}, {:.4}, {:.4})", v.i, v.j, v.k);
        }
    }
    println!();

    let mut policy_counts = [0usize; 4];
    for c in &lm.clusters {
        let idx = match c.policy() {
            LightmapPolicy::PerPixel => 0,
            LightmapPolicy::PerVertex => 1,
            LightmapPolicy::SingleProbe => 2,
            LightmapPolicy::Fallback => 3,
        };
        policy_counts[idx] += 1;
    }
    println!("clusters[{}]:  per_pixel={}  per_vertex={}  single_probe={}  fallback={}",
        lm.clusters.len(), policy_counts[0], policy_counts[1], policy_counts[2], policy_counts[3]);

    let mut policy_counts = [0usize; 4];
    for inst in &lm.instances {
        let idx = match inst.policy() {
            LightmapPolicy::PerPixel => 0,
            LightmapPolicy::PerVertex => 1,
            LightmapPolicy::SingleProbe => 2,
            LightmapPolicy::Fallback => 3,
        };
        policy_counts[idx] += 1;
    }
    println!("instances[{}]:  per_pixel={}  per_vertex={}  single_probe={}  fallback={}",
        lm.instances.len(), policy_counts[0], policy_counts[1], policy_counts[2], policy_counts[3]);

    println!();
    println!("probes[{}] (single):", lm.probes.len());
    for (i, p) in lm.probes.iter().take(3).enumerate() {
        println!(
            "  [{i}] dom_dir={:?} dom_intensity={:?}",
            p.dominant_light_direction, p.dominant_light_intensity,
        );
        println!(
            "      r_terms[0..3]={:?} g_terms[0..3]={:?} b_terms[0..3]={:?}",
            &p.red_terms[..3], &p.green_terms[..3], &p.blue_terms[..3],
        );
    }
    println!();

    println!("bsp_per_vertex_data[{}]:", lm.bsp_per_vertex_data.len());
    let total: usize = lm.bsp_per_vertex_data.iter().map(|b| b.lightprobe_data.len()).sum();
    println!("  total per-vertex probes: {total}");
    println!();

    println!("scenery_probes[{}]   airprobes[{}]   device_machine_probes[{}]",
        lm.scenery_probes.len(), lm.airprobes.len(), lm.device_machine_probes.len());
}
