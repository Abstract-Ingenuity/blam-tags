//! Coverage sweep for classic (CE / H2) bitmap extraction. Walks a tag
//! tree, routes each `.bitmap` via `_meta.json` → def → classic decoder,
//! then tries to DDS-encode every image and tallies outcomes (overall +
//! per-image), bucketing failures by reason / format name.
//!
//! Usage: classic_bitmap_sweep <defs-dir> <tags-root>

use std::collections::BTreeMap;
use std::path::PathBuf;

use blam_tags::bitmap::{Bitmap, BitmapError};
use blam_tags::classic::{read_classic_tag_file, ClassicHeader};
use blam_tags::layout::TagLayout;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let defs_dir = PathBuf::from(&args[1]);
    let root = &args[2];

    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(defs_dir.join("_meta.json")).unwrap()).unwrap();
    let mut name_for: BTreeMap<[u8; 4], String> = BTreeMap::new();
    for (k, v) in meta["tag_index"].as_object().unwrap() {
        let mut key = [b' '; 4];
        for (i, b) in k.bytes().take(4).enumerate() {
            key[i] = b;
        }
        name_for.insert(key, v.as_str().unwrap().to_owned());
    }

    let (mut tags, mut tag_ok, mut tag_partial, mut tag_fail) = (0usize, 0, 0, 0);
    let (mut img_total, mut img_ok) = (0usize, 0usize);
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();

    for entry in walkdir(root) {
        if !entry.to_string_lossy().ends_with(".bitmap") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&entry) else { continue };
        let Some((header, _engine)) = ClassicHeader::parse(&bytes) else { continue };
        if name_for.get(&header.group_tag).map(|s| s.as_str()) != Some("bitmap") {
            continue;
        }
        let Ok(l) = TagLayout::from_json(defs_dir.join("bitmap.json")) else { continue };
        let tag = match read_classic_tag_file(&bytes, l) {
            Ok(t) => t,
            Err(e) => {
                *reasons.entry(format!("decode: {e}")).or_default() += 1;
                tag_fail += 1;
                continue;
            }
        };
        let bm = match Bitmap::new(&tag) {
            Ok(b) => b,
            Err(e) => {
                *reasons.entry(reason(&e)).or_default() += 1;
                tag_fail += 1;
                continue;
            }
        };
        tags += 1;
        let mut ok = 0usize;
        let n = bm.len();
        for i in 0..n {
            img_total += 1;
            let Some(img) = bm.image(i) else { continue };
            let mut sink = Vec::new();
            match img.write_dds(&mut sink) {
                Ok(()) => {
                    ok += 1;
                    img_ok += 1;
                }
                Err(e) => *reasons.entry(reason(&e)).or_default() += 1,
            }
        }
        if ok == n {
            tag_ok += 1;
        } else if ok > 0 {
            tag_partial += 1;
        } else {
            tag_fail += 1;
        }
    }

    println!(
        "=== {tags} bitmap tags | {tag_ok} all-images-ok | {tag_partial} partial | {tag_fail} failed ==="
    );
    println!("    images: {img_ok}/{img_total} DDS-encodable");
    println!("--- failure reasons (count) ---");
    let mut sorted: Vec<_> = reasons.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    for (r, c) in sorted {
        println!("  {c:>7}  {r}");
    }
}

fn reason(e: &BitmapError) -> String {
    match e {
        BitmapError::FormatNotSupported(n) => format!("format-unsupported: {n}"),
        BitmapError::UnsupportedTextureType(n) => format!("type-unsupported: {n}"),
        BitmapError::PixelSliceOutOfBounds { .. } => "pixel-slice-oob".into(),
        BitmapError::NotABitmapTag => "not-a-bitmap".into(),
        other => format!("{other}"),
    }
}

fn walkdir(root: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::from(root)];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}
