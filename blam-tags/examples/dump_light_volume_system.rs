//! Dump a light_volume_system (ltvl) tag.

use blam_tags::effects_properties::EditableProperty;
use blam_tags::file::TagFile;
use blam_tags::light_volume_system::LightVolumeSystem;

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump_light_volume_system <path>");
    let tag = TagFile::read(&path).expect("read");
    let lv = LightVolumeSystem::from_tag(&tag).expect("walk");
    println!("=== {} ===", path);
    println!("definitions[{}]:", lv.definitions.len());
    for (i, d) in lv.definitions.iter().enumerate() {
        println!("  [{}] name={:?}", i, d.name);
        println!("      shader={}", if d.shader.is_some() {"present"} else {"none"});
        println!("      appearance_flags=0x{:04x}  brightness_ratio={:.3}",
            d.appearance_flags, d.brightness_ratio);
        println!("      runtime: const_props=0x{:08x} used_states=0x{:08x} max_profiles={}",
            d.runtime_constant_per_profile_properties,
            d.runtime_used_states,
            d.runtime_max_profile_count);
        print_prop("length            ", &d.length);
        print_prop("offset            ", &d.offset);
        print_prop("profile_density   ", &d.profile_density);
        print_prop("profile_length    ", &d.profile_length);
        print_prop("profile_thickness ", &d.profile_thickness);
        print_prop("profile_color     ", &d.profile_color);
        print_prop("profile_alpha     ", &d.profile_alpha);
        print_prop("profile_intensity ", &d.profile_intensity);
    }
}

fn print_prop(label: &str, p: &EditableProperty) {
    println!("      {label}: input={} range={} omod={} omod_in={}  const={:.4} flags=0x{:02x} fn={}",
        p.input_index, p.range_input_index,
        p.output_modifier_type, p.output_modifier_input_index,
        p.constant_value, p.runtime_flags,
        if p.function.is_some() {"yes"} else {"no"});
}
