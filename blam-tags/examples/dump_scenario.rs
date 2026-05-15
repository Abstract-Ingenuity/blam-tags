//! Dump a Halo scenario tag's rendering-relevant fields.
//!
//! Usage:
//!   cargo run --example dump_scenario -- <path/to/level.scenario>

use std::path::PathBuf;

use blam_tags::TagFile;
use blam_tags::scenario::{ObjectPlacement, Scenario, TagReferencePalette};

fn main() {
    let Some(path_str) = std::env::args().nth(1) else {
        eprintln!("usage: dump_scenario <path/to/level.scenario>");
        std::process::exit(2);
    };
    let path = PathBuf::from(&path_str);
    let tag = TagFile::read(&path).unwrap_or_else(|e| {
        eprintln!("failed to read {}: {e}", path.display());
        std::process::exit(1);
    });
    let scnr = Scenario::from_tag(&tag).expect("Scenario::from_tag failed");

    println!("scenario: {}", path.display());
    println!("  type:        {}", scnr.scenario_type);
    println!("  map id:      {}", scnr.map_id);
    println!("  local north: {:.3} rad", scnr.local_north);
    println!();

    println!("structure_bsps: [{}]", scnr.structure_bsps.len());
    for (i, bsp) in scnr.structure_bsps.iter().enumerate() {
        println!("  [{i}] sbsp:        {}", short(&bsp.structure_bsp));
        println!("      design:      {}", short(&bsp.structure_design));
        println!("      lighting:    {}", short(&bsp.structure_lighting_info));
        println!("      cubemaps:    {}", short(&bsp.cubemap_bitmap_group));
        println!("      wind:        {}", short(&bsp.wind));
        println!("      flags:       0x{:04x}", bsp.flags);
        println!("      default_sky: {}", bsp.default_sky_index);
    }
    println!();

    println!("skies: [{}]", scnr.skies.len());
    for (i, sky) in scnr.skies.iter().enumerate() {
        println!(
            "  [{i}] sky={}  active_on_bsps=0x{:04x}",
            short(&sky.sky),
            sky.active_on_bsp_flags,
        );
    }
    println!();

    println!("zone_sets: [{}]", scnr.zone_sets.len());
    for (i, zs) in scnr.zone_sets.iter().enumerate() {
        println!(
            "  [{i}] {:?}  pvs={}  bsp_zone_flags=0x{:08x}  required_designer=0x{:08x}",
            zs.name, zs.pvs_index, zs.bsp_zone_flags, zs.required_designer_zone_flags,
        );
        println!("      active BSPs: {:?}", scnr.zone_set_active_bsps(i));
    }
    println!();

    print_palette_and_placements("scenery", &scnr.scenery_palette, &scnr.scenery);
    print_palette_and_placements("biped", &scnr.biped_palette, &scnr.bipeds);
    print_palette_and_placements("vehicle", &scnr.vehicle_palette, &scnr.vehicles);
    print_palette_and_placements("equipment", &scnr.equipment_palette, &scnr.equipment);
    print_palette_and_placements("weapon", &scnr.weapon_palette, &scnr.weapons);
    print_palette_and_placements("machine", &scnr.machine_palette, &scnr.machines);
    print_palette_and_placements("control", &scnr.control_palette, &scnr.controls);
    print_palette_and_placements("sound_scenery", &scnr.sound_scenery_palette, &scnr.sound_scenery);
    print_palette_and_placements("crate", &scnr.crate_palette, &scnr.crates);
    print_palette_and_placements("light", &scnr.light_palette, &scnr.lights);

    println!("decorators: [{}]", scnr.decorators.len());
    for (i, dec) in scnr.decorators.iter().enumerate() {
        println!(
            "  [{i}] count={} bsp_count={} palettes={} sets={}",
            dec.decorator_count,
            dec.current_bsp_count,
            dec.palettes.len(),
            dec.sets.len(),
        );
        for (j, set) in dec.sets.iter().enumerate() {
            println!(
                "      set[{j}] {} placements={}",
                short(&set.decorator_set),
                set.placements.len(),
            );
        }
    }
    println!();

    println!("cubemaps: [{}]", scnr.cubemaps.len());
    for (i, cm) in scnr.cubemaps.iter().enumerate() {
        println!(
            "  [{i}] pos=({:.2}, {:.2}, {:.2}) res={}",
            cm.position.x, cm.position.y, cm.position.z, cm.resolution_pixels,
        );
    }
    println!();

    println!("new_lightmaps:   {}", short(&scnr.new_lightmaps));
    println!("structure_seams: {}", short(&scnr.structure_seams));
}

fn print_palette_and_placements(label: &str, palette: &[TagReferencePalette], placements: &[ObjectPlacement]) {
    if palette.is_empty() && placements.is_empty() {
        return;
    }
    println!("{label}: palette=[{}] placements=[{}]", palette.len(), placements.len());
    for (i, p) in palette.iter().enumerate() {
        println!("  palette[{i}] {}", short(&p.tag_path));
    }
    for (i, pl) in placements.iter().take(8).enumerate() {
        let palette_path = pl
            .palette_index
            .checked_sub(0)
            .and_then(|idx| palette.get(idx as usize))
            .map(|p| p.tag_path.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("<INVALID>");
        let pos = pl.object_data.position;
        let rot = pl.object_data.rotation;
        let var = if pl.permutation_data.variant_name.is_empty() {
            "<default>"
        } else {
            pl.permutation_data.variant_name.as_str()
        };
        println!(
            "  [{i}] type={} ({}) pos=({:.2}, {:.2}, {:.2}) yaw={:.2} variant={}",
            pl.palette_index, short(palette_path), pos.x, pos.y, pos.z, rot.yaw, var,
        );
    }
    if placements.len() > 8 {
        println!("  ... +{} more", placements.len() - 8);
    }
    println!();
}

fn short(s: &str) -> &str {
    if s.is_empty() {
        "(none)"
    } else {
        s
    }
}
