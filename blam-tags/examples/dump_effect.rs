//! Smoke test for the effect tag walker (P1.T1).
//!
//! Reads one or more `.effect` tags and prints a tree summary:
//! locations, events, parts, accelerations, particle_systems +
//! emitters.
//!
//! Usage:
//!   cargo run --example dump_effect -- <path/to/foo.effect> [more...]
//!
//! Default if no args: dump all 7 riverworld effect tags.

use std::path::PathBuf;

use blam_tags::effect::{EffectDefinition, EffectPartType};
use blam_tags::TagFile;

fn main() {
    let paths: Vec<PathBuf> = if std::env::args().len() > 1 {
        std::env::args().skip(1).map(PathBuf::from).collect()
    } else {
        let base = "/Users/camden/Halo/halo3_mcc/tags/levels/multi/riverworld/fx";
        vec![
            format!("{base}/waterfall/waterfall_top.effect"),
            format!("{base}/waterfall/waterfall_mid.effect"),
            format!("{base}/waterfall/waterfall_base.effect"),
            format!("{base}/tower_pulse/tower_pulse.effect"),
            format!("{base}/tower_pulse/projectile.effect"),
            format!("{base}/man_cannon/man_cannon.effect"),
            format!("{base}/man_cannon/mini_man_cannon.effect"),
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect()
    };

    for path in paths {
        match dump_one(&path) {
            Ok(()) => {}
            Err(e) => eprintln!("{}: ERROR {e}", path.display()),
        }
        println!();
    }
}

fn dump_one(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let tag = TagFile::read(path)?;
    let efx = EffectDefinition::from_tag(&tag)?;
    println!("=== {} ===", path.display());
    println!(
        "flags=0x{:08x} priority={:?} loop_start_event={} fixed_seed={} death_delay={}",
        efx.flags,
        efx.priority,
        efx.loop_start_event,
        efx.fixed_random_seed,
        efx.death_delay,
    );
    println!(
        "restart_if_within={} continue_if_within={} always_play={} never_play={}",
        efx.restart_if_within,
        efx.continue_if_within,
        efx.always_play_distance,
        efx.never_play_distance,
    );
    if !efx.looping_sound_tag_path.is_empty() {
        println!(
            "  looping_sound: {} (loc#{}, scale_bind_event#{})",
            efx.looping_sound_tag_path,
            efx.looping_sound_location,
            efx.looping_sound_bind_scale_to_event,
        );
    }

    println!("locations[{}]:", efx.locations.len());
    for (i, loc) in efx.locations.iter().enumerate() {
        println!(
            "  [{i}] marker='{}' flags=0x{:x} prio={:?}",
            loc.marker_name, loc.flags, loc.priority,
        );
    }

    println!("events[{}]:", efx.events.len());
    for (ei, ev) in efx.events.iter().enumerate() {
        println!(
            "  event[{ei}] name='{}' flags=0x{:x} prio={:?} skip={} delay=[{},{}] duration=[{},{}]",
            ev.name,
            ev.flags,
            ev.priority,
            ev.skip_fraction,
            ev.delay_bounds.lower,
            ev.delay_bounds.upper,
            ev.duration_bounds.lower,
            ev.duration_bounds.upper,
        );

        for (pi, part) in ev.parts.iter().enumerate() {
            let fourcc = std::str::from_utf8(&part.runtime_base_group_tag)
                .unwrap_or("????")
                .to_string();
            let type_fourcc =
                std::str::from_utf8(&part.type_group).unwrap_or("????").to_string();
            println!(
                "    part[{pi}] type={:?}({fourcc}) loc#{} env={:?} mode={:?} flags=0x{:x} target='{type_fourcc}:{}'",
                part.part_type,
                part.location,
                part.environment,
                part.violence_mode,
                part.flags,
                part.type_tag_path,
            );
        }

        for (ai, acc) in ev.accelerations.iter().enumerate() {
            println!(
                "    accel[{ai}] loc#{} accel={} cone=[{}..{}] env={:?}",
                acc.location,
                acc.acceleration,
                acc.inner_cone_angle_degrees,
                acc.outer_cone_angle_degrees,
                acc.environment,
            );
        }

        for (psi, ps) in ev.particle_systems.iter().enumerate() {
            println!(
                "    psys[{psi}] particle='{}' loc#{} coord={:?} env={:?} cam={:?} sort={} flags=0x{:x} budget_ms={} lod=[{}..{}]",
                ps.particle_tag_path,
                ps.location,
                ps.coordinate_system,
                ps.environment,
                ps.camera_mode,
                ps.sort_bias,
                ps.flags,
                ps.pixel_budget_ms,
                ps.lod_in_distance,
                ps.lod_out_distance,
            );
            for (emi, em) in ps.emitters.iter().enumerate() {
                println!(
                    "      emitter[{emi}] name='{}' shape={} flags=0x{:x} bound=[est={} ovr={}]",
                    em.name,
                    em.emission_shape,
                    em.flags,
                    em.bounding_radius_estimate,
                    em.bounding_radius_override,
                );
            }
        }
    }

    if !efx.conical_distribution.is_empty() {
        println!("conical_distribution[{}]:", efx.conical_distribution.len());
        for (ci, cd) in efx.conical_distribution.iter().enumerate() {
            println!(
                "  [{ci}] yaw={} pitch={} exp={} spread={}°",
                cd.yaw_count, cd.pitch_count, cd.distribution_exponent, cd.spread_degrees,
            );
        }
    }

    // Sanity: enumerate the dispatch types we'd hand the multiplexer.
    let mut by_type: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for ev in &efx.events {
        for part in &ev.parts {
            let key = match part.part_type {
                EffectPartType::Empty => "(empty)".to_string(),
                EffectPartType::Unknown(b) => {
                    format!("unknown({})", std::str::from_utf8(&b).unwrap_or("?"))
                }
                t => format!("{t:?}"),
            };
            *by_type.entry(key).or_insert(0) += 1;
        }
    }
    if !by_type.is_empty() {
        println!("dispatch_summary:");
        for (k, n) in &by_type {
            println!("  {n} × {k}");
        }
    }
    Ok(())
}
