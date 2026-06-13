//! Print the first N failing tags (with error / first-mismatch offset) for
//! one group. Usage: classic_fail_samples <defs-dir> <tags-root> <group> [N]

use blam_tags::classic::{classic_roundtrip, ClassicHeader};
use blam_tags::layout::TagLayout;
use std::path::PathBuf;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let defs_dir = PathBuf::from(&a[1]);
    let root = &a[2];
    let group = &a[3];
    let limit: usize = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(8);

    let layout = TagLayout::from_json(defs_dir.join(format!("{group}.json"))).expect("layout");
    let ext = format!(".{group}");
    let mut shown = 0;
    let (mut ok, mut mm, mut df) = (0, 0, 0);
    let mut stack = vec![PathBuf::from(root)];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if !p.to_string_lossy().ends_with(&ext) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&p) else { continue };
            let Some((_h, engine)) = ClassicHeader::parse(&bytes) else { continue };
            let body = &bytes[64..];
            match classic_roundtrip(body, &layout, engine) {
                Ok(re) if re == body => ok += 1,
                Ok(re) => {
                    mm += 1;
                    if shown < limit {
                        shown += 1;
                        let n = body.len().min(re.len());
                        let f = (0..n).find(|&i| body[i] != re[i]).unwrap_or(n);
                        println!("MM @{f}/{} {}", body.len(), p.strip_prefix(root).unwrap().display());
                        let lo = f.saturating_sub(6);
                        let hi = (f + 6).min(n);
                        println!("   body {:02x?}", &body[lo..hi]);
                        println!("   re   {:02x?}", &re[lo..hi]);
                    }
                }
                Err(err) => {
                    df += 1;
                    if shown < limit {
                        shown += 1;
                        println!("DF  {}  :: {err}", p.strip_prefix(root).unwrap().display());
                    }
                }
            }
        }
    }
    println!("--- {group}: ok={ok} mm={mm} df={df} ---");
}
