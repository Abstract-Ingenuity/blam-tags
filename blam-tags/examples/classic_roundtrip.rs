//! Classic (Halo CE / Halo 2) tag byte-exact roundtrip gate.
//!
//! Reads each tag of a given group, parses the classic header, decodes
//! the body via the synthesized JSON layout, re-encodes, and asserts the
//! output equals the input byte-for-byte.
//!
//! Usage:
//!   classic_roundtrip <def.json> <tags-root> [<extension>]
//!
//! Example:
//!   classic_roundtrip definitions/haloce_mcc/bitmap.json \
//!       /Users/camden/Halo/haloce_mcc/tags bitmap

use blam_tags::classic::{classic_roundtrip, ClassicHeader};
use blam_tags::layout::TagLayout;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: classic_roundtrip <def.json> <tags-root> [<extension>]");
        std::process::exit(2);
    }
    let def_path = &args[1];
    let root = &args[2];
    let ext = args.get(3).cloned();

    let layout = match TagLayout::from_json(def_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("failed to load layout {def_path}: {e}");
            std::process::exit(1);
        }
    };

    let mut total = 0usize;
    let mut ok = 0usize;
    let mut header_skip = 0usize;
    let mut decode_fail = 0usize;
    let mut mismatch = 0usize;
    let mut first_failures: Vec<String> = Vec::new();

    let want_ext = ext.as_deref();
    for entry in walkdir(root) {
        if let Some(want) = want_ext {
            if entry.extension().and_then(|s| s.to_str()) != Some(want) {
                continue;
            }
        }
        total += 1;
        let bytes = match std::fs::read(&entry) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let (_, engine) = match ClassicHeader::parse(&bytes) {
            Some(h) => h,
            None => {
                header_skip += 1;
                continue;
            }
        };
        let body = &bytes[64..];
        match classic_roundtrip(body, &layout, engine) {
            Ok(re) => {
                if re == body {
                    ok += 1;
                } else {
                    mismatch += 1;
                    if first_failures.len() < 10 {
                        let diff_at = re
                            .iter()
                            .zip(body.iter())
                            .position(|(a, b)| a != b)
                            .map(|p| p as i64)
                            .unwrap_or(-1);
                        first_failures.push(format!(
                            "MISMATCH {} (len {} vs {}, first diff @ {})",
                            entry.display(),
                            re.len(),
                            body.len(),
                            diff_at
                        ));
                    }
                }
            }
            Err(e) => {
                decode_fail += 1;
                if first_failures.len() < 10 {
                    first_failures.push(format!("DECODE-FAIL {}: {e}", entry.display()));
                }
            }
        }
    }

    for f in &first_failures {
        println!("{f}");
    }
    println!(
        "\n{total} tags | {ok} byte-exact | {mismatch} mismatch | {decode_fail} decode-fail | {header_skip} non-classic-header"
    );
    if mismatch > 0 || decode_fail > 0 {
        std::process::exit(1);
    }
}

fn walkdir(root: &str) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(root)];
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
