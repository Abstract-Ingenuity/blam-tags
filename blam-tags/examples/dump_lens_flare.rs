//! Dump a lens_flare (lens) tag.

use blam_tags::file::TagFile;
use blam_tags::lens_flare::LensFlare;

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump_lens_flare <path>");
    let tag = TagFile::read(&path).expect("read");
    let l = LensFlare::from_tag(&tag).expect("walk");
    println!("=== {} ===", path);
    println!("falloff/cutoff angles:   {:.2} / {:.2} deg",
        l.falloff_angle_degrees, l.cutoff_angle_degrees);
    println!("occlusion: ref_idx={} offset_dist={:.3} dir={} radius_scale={}",
        l.occlusion_reflection_index, l.occlusion_offset_distance,
        l.occlusion_offset_direction, l.occlusion_inner_radius_scale);
    println!("fade: near_begin={:.2} near_end={:.2} near={:.2} far={:.2}",
        l.near_fade_begin_distance, l.near_fade_end_distance,
        l.near_fade_distance, l.far_fade_distance);
    println!("bitmap:                  {:?}", l.bitmap);
    println!("flags/runtime_flags:     0x{:04x} / 0x{:04x}", l.flags, l.runtime_flags);
    println!("rotation: fn={} scale={:.2}deg  falloff_fn={}",
        l.rotation_function, l.rotation_function_scale_degrees, l.falloff_function);
    println!();
    println!("animation_flags:         0x{:04x}", l.animation_flags);
    println!("  time_brightness curves: {}", l.time_brightness.len());
    println!("  age_brightness curves:  {}", l.age_brightness.len());
    println!("  time_color funcs:       {}", l.time_color.len());
    println!("  age_color funcs:        {}", l.age_color.len());
    println!("  time_rotation curves:   {}", l.time_rotation.len());
    println!("  age_rotation curves:    {}", l.age_rotation.len());
    println!();
    println!("reflections[{}]:", l.reflections.len());
    for (i, r) in l.reflections.iter().enumerate() {
        println!("  [{i}] flags=0x{:04x}  bitmap_idx={}  rot_off={:.2}  axis_off={:.3}",
            r.flags, r.bitmap_index, r.rotation_offset_degrees, r.axis_offset);
        println!("       radius=[{:.3},{:.3}]  brightness=[{:.3},{:.3}]",
            r.radius_world_units.lower, r.radius_world_units.upper,
            r.brightness.lower, r.brightness.upper);
        println!("       color=({:.3},{:.3},{:.3}) mod={:.3} tint_pow={:.2}",
            r.color.red, r.color.green, r.color.blue,
            r.modulation_factor, r.tint_power);
        println!("       curves: radius={} sx={} sy={} bright={}",
            if r.radius_curve.is_some() {"fn"} else {"-"},
            if r.scale_curve_x.is_some() {"fn"} else {"-"},
            if r.scale_curve_y.is_some() {"fn"} else {"-"},
            if r.brightness_curve.is_some() {"fn"} else {"-"});
        if let Some(o) = &r.bitmap_override { println!("       bitmap_override: {o}"); }
    }
}
