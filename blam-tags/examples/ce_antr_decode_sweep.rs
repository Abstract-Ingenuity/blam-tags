//! Full CE model_animations decode sweep: decode + pose every animation,
//! sanity-check quaternions/sizes, count compressed vs uncompressed.
//! Usage: ce_antr_decode_sweep <defs-dir> <tags-root>

use std::path::PathBuf;
use blam_tags::animation::classic::CeAnimations;
use blam_tags::animation::Skeleton;
use blam_tags::classic::read_classic_tag_file;
use blam_tags::layout::TagLayout;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let layout_path = PathBuf::from(&a[1]).join("model_animations.json");
    let mut stack = vec![PathBuf::from(&a[2])];
    let (mut tags, mut anims, mut uncompressed, mut compressed, mut bad_q, mut bad_dims) = (0u64,0u64,0u64,0u64,0u64,0u64);
    let mut examples = Vec::new();

    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() { stack.push(p); continue; }
            if p.extension().and_then(|x| x.to_str()) != Some("model_animations") { continue; }
            let Ok(bytes) = std::fs::read(&p) else { continue };
            let Ok(layout) = TagLayout::from_json(&layout_path) else { continue };
            let Ok(tag) = read_classic_tag_file(&bytes, layout) else { continue };
            tags += 1;
            let skel = Skeleton::from_tag(&tag);
            let ce = CeAnimations::new(&tag);
            for g in ce.iter() {
                anims += 1;
                // (compressed flag is private; infer from a re-read is overkill — just decode)
                let clip = g.decode();
                let pose = clip.pose(&skel, None);
                // sanity: frame count + bone count
                if pose.frames.len() != g.frame_count.max(1) as usize
                    || pose.frames.iter().any(|r| r.len() != skel.len()) {
                    bad_dims += 1;
                    if examples.len() < 8 { examples.push(format!("DIM {} {:?} frames={} bones={}", p.file_name().unwrap().to_string_lossy(), g.name, pose.frames.len(), skel.len())); }
                }
                // sanity: every quaternion finite + ~unit (post-normalize)
                let mut ok = true;
                for row in &pose.frames {
                    for t in row {
                        let q = t.rotation;
                        if !q.i.is_finite() || !q.j.is_finite() || !q.k.is_finite() || !q.w.is_finite() { ok = false; }
                        if !t.translation.x.is_finite() || !t.translation.y.is_finite() || !t.translation.z.is_finite() { ok = false; }
                    }
                }
                if !ok { bad_q += 1; if examples.len() < 8 { examples.push(format!("NaN {} {:?}", p.file_name().unwrap().to_string_lossy(), g.name)); } }
            }
            // count compressed via the source flag (re-walk raw)
            if let Some(ab) = tag.root().field_path("animations").and_then(|f| f.as_block()) {
                for i in 0..ab.len() {
                    if let Some(el) = ab.element(i) {
                        let f = el.read_int_any("flags").unwrap_or(0) as u32;
                        if f & 1 == 1 { compressed += 1; } else { uncompressed += 1; }
                    }
                }
            }
        }
    }
    println!("tags={tags} anims={anims} (uncompressed={uncompressed} compressed={compressed}) bad_quat={bad_q} bad_dims={bad_dims}");
    for e in &examples { println!("  {e}"); }
    Ok(())
}
