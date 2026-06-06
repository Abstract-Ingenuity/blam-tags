//! Dump exposure fields from a .camera_fx_settings tag for diagnosis.

use blam_tags::camera_fx_settings::{CameraFxAutoAdjustFlags, CameraFxSettings};
use blam_tags::file::TagFile;

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump_camera_fx <path>");
    let tag = TagFile::read(&path).expect("failed to read tag");
    let cfx = CameraFxSettings::from_tag(&tag).expect("failed to parse cfxs");

    println!("=== {} ===", path);
    println!("exposure:");
    let f = &cfx.exposure.flags;
    println!("  flags:                            {:?}", f);
    use CameraFxAutoAdjustFlags::*;
    println!("    USE_DEFAULT:                    {}", f.contains(UseDefault));
    println!("    MAX_CHANGE_IS_RELATIVE:         {}", f.contains(MaxChangeIsRelative));
    println!("    AUTO_ADJUST_TARGET:             {}", f.contains(AutoAdjustTarget));
    println!("    BIT3:                           {}", f.contains(Bit3));
    println!("    FIXED:                          {}", f.contains(Fixed));
    println!("  exposure (static target stops):   {:.4}", cfx.exposure.exposure);
    println!("  maximum_change:                   {:.4}", cfx.exposure.maximum_change);
    println!("  blend_speed:                      {:.4}", cfx.exposure.blend_speed);
    println!("  minimum (stops clamp):            {:.4}", cfx.exposure.minimum);
    println!("  maximum (stops clamp):            {:.4}", cfx.exposure.maximum);
    println!("  auto_exposure_screen_brightness:  {:.4}", cfx.exposure.auto_exposure_screen_brightness);
    println!("  auto_exposure_delay:              {:.4}", cfx.exposure.auto_exposure_delay);
    println!();
    println!("bloom_point:                        flags={:?} value={:.4}", cfx.bloom_point.flags, cfx.bloom_point.value);
    println!("bloom_inherent:                     flags={:?} value={:.4}", cfx.bloom_inherent.flags, cfx.bloom_inherent.value);
    println!("bloom_intensity:                    flags={:?} value={:.4}", cfx.bloom_intensity.flags, cfx.bloom_intensity.value);
    println!("auto_exposure_anti_bloom:           flags={:?} value={:.4}", cfx.auto_exposure_anti_bloom.flags, cfx.auto_exposure_anti_bloom.value);
    println!("auto_exposure_sensitivity:          flags={:?} value={:.4}", cfx.auto_exposure_sensitivity.flags, cfx.auto_exposure_sensitivity.value);
    println!("self_illum_preferred:               flags={:?} value={:.4}", cfx.self_illum_preferred.flags, cfx.self_illum_preferred.value);
    println!("self_illum_scale:                   flags={:?} value={:.4}", cfx.self_illum_scale.flags, cfx.self_illum_scale.value);
}
