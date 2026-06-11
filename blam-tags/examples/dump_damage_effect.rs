//! Dump a damage_effect (jpt!) tag.

use blam_tags::damage_effect::DamageEffect;
use blam_tags::file::TagFile;

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump_damage_effect <path>");
    let tag = TagFile::read(&path).expect("read");
    let d = DamageEffect::from_tag(&tag).expect("walk");
    println!("=== {} ===", path);
    println!("radius:               {:.3}..{:.3}", d.radius.lower, d.radius.upper);
    println!("cutoff_scale:         {:.3}", d.cutoff_scale);
    println!("effect_flags:         0x{:08x}", d.effect_flags);
    println!("side_effect/cat:      {} / {}", d.side_effect, d.category);
    println!("flags:                0x{:08x}", d.flags);
    println!("AOE core radius:      {:.3}", d.aoe_core_radius);
    println!("damage range:         {:.3} -> [{:.3}, {:.3}]",
        d.damage_lower_bound, d.damage_upper_bound.lower, d.damage_upper_bound.upper);
    println!("cone:                 inner={:.3} outer={:.3}",
        d.inner_cone_angle_radians, d.outer_cone_angle_radians);
    println!("stun:                 {:.3} max={:.3} time={:.3}s",
        d.stun, d.maximum_stun, d.stun_time_seconds);
    println!("EMP / shake radius:   {:.3} / {:.3}", d.emp_radius, d.shake_radius);
    println!();
    println!("camera impulse:       duration={:.3}s rot={:.3}deg pushback={:.3} jitter={:.3}..{:.3} fade={}",
        d.camera_impulse.duration_seconds,
        d.camera_impulse.rotation_degrees,
        d.camera_impulse.pushback,
        d.camera_impulse.jitter.lower,
        d.camera_impulse.jitter.upper,
        d.camera_impulse.fade_function);
    println!("camera shake:         duration={:.3}s trans={:.3} rot={:.3}deg falloff={} wobble={}/period={:.3}/wt={:.3}",
        d.camera_shake.duration_seconds,
        d.camera_shake.random_translation,
        d.camera_shake.random_rotation_degrees,
        d.camera_shake.falloff_function,
        d.camera_shake.wobble_function,
        d.camera_shake.wobble_period_seconds,
        d.camera_shake.wobble_weight);
    println!("sound:                {:?}", d.sound);
    println!();
    println!("breaking impulse:");
    println!("  forward:  v={:.3} r={:.3} exp={:.3}",
        d.breaking_impulse.forward_velocity,
        d.breaking_impulse.forward_radius,
        d.breaking_impulse.forward_exponent);
    println!("  outward:  v={:.3} r={:.3} exp={:.3}",
        d.breaking_impulse.outward_velocity,
        d.breaking_impulse.outward_radius,
        d.breaking_impulse.outward_exponent);
    println!();
    println!("player_responses[{}]:", d.player_responses.len());
    for (i, pr) in d.player_responses.iter().enumerate() {
        println!("  [{}] response_type={}", i, pr.response_type);
        println!("      screen_flash: type={} prio={} dur={:.3}s fade={} max={:.3} color=({:.3},{:.3},{:.3},{:.3})",
            pr.screen_flash.flash_type, pr.screen_flash.priority,
            pr.screen_flash.duration_seconds, pr.screen_flash.fade_function,
            pr.screen_flash.maximum_intensity,
            pr.screen_flash.color.alpha, pr.screen_flash.color.red,
            pr.screen_flash.color.green, pr.screen_flash.color.blue);
        println!("      rumble.low:   dur={:.3}s envelope={}",
            pr.rumble.low_frequency.duration_seconds,
            if pr.rumble.low_frequency.envelope.is_some() {"<fn>"} else {"none"});
        println!("      rumble.high:  dur={:.3}s envelope={}",
            pr.rumble.high_frequency.duration_seconds,
            if pr.rumble.high_frequency.envelope.is_some() {"<fn>"} else {"none"});
        println!("      sound_effect: name={:?} dur={:.3}s scale_fn={}",
            pr.sound_effect.effect_name, pr.sound_effect.duration_seconds,
            if pr.sound_effect.scale_function.is_some() {"<fn>"} else {"none"});
    }
}
