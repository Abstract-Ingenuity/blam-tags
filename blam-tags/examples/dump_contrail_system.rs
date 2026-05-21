//! Dump a contrail_system (cntl) tag.

use blam_tags::contrail_system::ContrailSystem;
use blam_tags::effects_properties::EditableProperty;
use blam_tags::file::TagFile;

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump_contrail_system <path>");
    let tag = TagFile::read(&path).expect("read");
    let cs = ContrailSystem::from_tag(&tag).expect("walk");
    println!("=== {} ===", path);
    println!("definitions[{}]:", cs.definitions.len());
    for (i, d) in cs.definitions.iter().enumerate() {
        println!("  [{i}] name={:?}", d.name);
        println!("       shader={}", if d.shader.is_some() {"present"} else {"none"});
        println!("       appearance_flags=0x{:04x} profile={:?} ngon_sides={}",
            d.appearance_flags, d.profile_shape, d.ngon_sides);
        println!("       uv_tiling=({:.3},{:.3}) uv_scroll=({:.3},{:.3})",
            d.uv_tiling.i, d.uv_tiling.j, d.uv_scrolling.i, d.uv_scrolling.j);
        println!("       origin_fade range={:.3} cutoff={:.3}  edge_fade range={:.3}deg cutoff={:.3}deg",
            d.origin_fade_range, d.origin_fade_cutoff,
            d.edge_fade_range_degrees, d.edge_fade_cutoff_degrees);
        println!("       lod: in={:.2} feather_in={:.2} out={:.2} feather_out={:.2}",
            d.lod_in_distance, d.lod_feather_in_distance,
            d.lod_out_distance, d.lod_feather_out_distance);
        println!("       runtime: const_props=0x{:08x} used_states=0x{:08x}",
            d.runtime_constant_per_profile_properties, d.runtime_used_states);
        p("emission_rate     ", &d.emission_rate);
        p("profile_lifespan  ", &d.profile_lifespan);
        p("profile_self_accel", &d.profile_self_acceleration);
        p("profile_size      ", &d.profile_size);
        p("profile_offset    ", &d.profile_offset);
        p("profile_rotation  ", &d.profile_rotation);
        p("profile_rot_rate  ", &d.profile_rotation_rate);
        p("profile_color     ", &d.profile_color);
        p("profile_alpha     ", &d.profile_alpha);
        p("profile_alpha2    ", &d.profile_secondary_alpha);
        p("profile_black_pt  ", &d.profile_black_point);
        p("profile_palette   ", &d.profile_palette);
        p("profile_intensity ", &d.profile_intensity);
    }
}

fn p(label: &str, e: &EditableProperty) {
    println!("       {label}: in={} rng={} omod={} omod_in={} const={:.4} flags=0x{:02x} fn={}",
        e.input_index, e.range_input_index, e.output_modifier_type,
        e.output_modifier_input_index, e.constant_value, e.runtime_flags,
        if e.function.is_some() {"yes"} else {"no"});
}
