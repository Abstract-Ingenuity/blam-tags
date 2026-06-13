//! Single-pass classic (CE / H2) byte-exact body-roundtrip sweep over a
//! whole tag tree. Walks the corpus once, routes each tag to its group's
//! synthesized layout via the header group tag + the def dir's
//! `_meta.json` (group tag -> name), and tallies pass/fail per group.
//!
//! Usage: classic_corpus_sweep <defs-dir> <tags-root>
//!   classic_corpus_sweep definitions/halo2_mcc ~/Halo/halo2_mcc/tags

use std::collections::BTreeMap;
use std::path::PathBuf;

use blam_tags::classic::{classic_roundtrip, ClassicHeader};
use blam_tags::layout::TagLayout;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: classic_corpus_sweep <defs-dir> <tags-root>");
        std::process::exit(2);
    }
    let defs_dir = PathBuf::from(&args[1]);
    let root = &args[2];

    // _meta.json -> { "ssce" : "scenery", ... } (keys are 4-char padded).
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(defs_dir.join("_meta.json")).expect("read _meta"))
            .expect("parse _meta");
    let mut name_for: BTreeMap<[u8; 4], String> = BTreeMap::new();
    for (k, v) in meta["tag_index"].as_object().expect("tag_index") {
        let mut key = [b' '; 4];
        for (i, b) in k.bytes().take(4).enumerate() {
            key[i] = b;
        }
        name_for.insert(key, v.as_str().unwrap().to_owned());
    }

    let mut layouts: BTreeMap<String, Option<TagLayout>> = BTreeMap::new();
    let mut per_group: BTreeMap<String, [usize; 4]> = BTreeMap::new(); // [ok, mm, df, total]
    let (mut tot, mut ok, mut mm, mut df, mut skip) = (0usize, 0, 0, 0, 0);

    for entry in walkdir(root) {
        let bytes = match std::fs::read(&entry) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let (header, engine) = match ClassicHeader::parse(&bytes) {
            Some(h) => h,
            None => continue, // not a classic tag header
        };
        let name = match name_for.get(&header.group_tag) {
            Some(n) => n.clone(),
            None => {
                skip += 1;
                continue;
            }
        };
        let layout = layouts
            .entry(name.clone())
            .or_insert_with(|| TagLayout::from_json(defs_dir.join(format!("{name}.json"))).ok());
        let layout = match layout {
            Some(l) => l,
            None => {
                skip += 1;
                continue;
            }
        };

        tot += 1;
        let slot = per_group.entry(name).or_insert([0, 0, 0, 0]);
        slot[3] += 1;
        let body = &bytes[64..];
        match classic_roundtrip(body, layout, engine) {
            Ok(re) if re == body => {
                ok += 1;
                slot[0] += 1;
            }
            Ok(_) => {
                mm += 1;
                slot[1] += 1;
            }
            Err(_) => {
                df += 1;
                slot[2] += 1;
            }
        }
    }

    println!(
        "=== {tot} classic tags | {ok} byte-exact | {mm} mismatch | {df} decode-fail | {skip} skipped-no-def ==="
    );
    if tot > 0 {
        println!("pass rate: {:.2}%", 100.0 * ok as f64 / tot as f64);
    }
    println!("--- groups with failures (ok/total mm df) ---");
    for (g, [o, m, d, t]) in &per_group {
        if *m > 0 || *d > 0 {
            println!("  {g:<32} {o}/{t}  mm={m} df={d}");
        }
    }
}

fn walkdir(root: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::from(root)];
    while let Some(dir) = stack.pop() {
        let rd = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
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
