//! Diagnostic: list every field name in the effect_definition root,
//! events[0], events[0].parts[0], and events[0].particle_systems[0] —
//! shows exactly what gets stored in the layout for `find_field_by_name`.

use std::path::PathBuf;
use blam_tags::TagFile;

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| {
            "/Users/camden/Halo/halo3_mcc/tags/levels/multi/riverworld/fx/tower_pulse/tower_pulse.effect"
                .to_string()
        });
    let tag = TagFile::read(&PathBuf::from(&path)).unwrap();
    let root = tag.root();

    println!("=== root field names (fields_all) ===");
    for f in root.fields_all() {
        println!("  '{}'  type={:?}", f.name(), f.field_type());
    }

    if let Some(events) = root.field("events").and_then(|f| f.as_block())
        && let Some(ev0) = events.element(0)
    {
        println!();
        println!("=== events[0] field names ===");
        for f in ev0.fields() {
            println!("  '{}'  type={:?}", f.name(), f.field_type());
        }

        if let Some(parts) = ev0.field("parts").and_then(|f| f.as_block())
            && let Some(p0) = parts.element(0)
        {
            println!();
            println!("=== events[0].parts[0] field names ===");
            for f in p0.fields() {
                println!("  '{}'  type={:?}", f.name(), f.field_type());
            }
        }

        if let Some(psystems) =
            ev0.field("particle systems").and_then(|f| f.as_block())
            && let Some(ps0) = psystems.element(0)
        {
            println!();
            println!("=== events[0].particle_systems[0] field names ===");
            for f in ps0.fields() {
                println!("  '{}'  type={:?}", f.name(), f.field_type());
            }
        }
    }

    if let Some(locs) = root.field("locations").and_then(|f| f.as_block())
        && let Some(l0) = locs.element(0)
    {
        println!();
        println!("=== locations[0] field names (fields_all) ===");
        for f in l0.fields_all() {
            println!("  '{}'  type={:?}", f.name(), f.field_type());
        }
    }

    if let Some(cd) = root
        .field("conical distribution")
        .and_then(|f| f.as_block())
        && let Some(c0) = cd.element(0)
    {
        println!();
        println!("=== conical_distribution[0] field names ===");
        for f in c0.fields_all() {
            println!("  '{}'  type={:?}", f.name(), f.field_type());
        }
    }
}
