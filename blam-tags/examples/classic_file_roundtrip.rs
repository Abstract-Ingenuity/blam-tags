//! Full-file classic write roundtrip: read a classic tag into a TagFile
//! (header + body), write it back out via TagFile::write_to_bytes (which
//! reconstructs the 64-byte header + recomputes the checksum), and assert
//! the whole file reproduces byte-for-byte.
//!
//! Usage: classic_file_roundtrip <def.json> <tags-root> <extension>

use blam_tags::classic::read_classic_tag_file;
use blam_tags::layout::TagLayout;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: classic_file_roundtrip <def.json> <tags-root> <extension>");
        std::process::exit(2);
    }
    let (def_path, root, ext) = (&args[1], &args[2], &args[3]);

    let mut total = 0;
    let mut ok = 0;
    let mut fails = Vec::new();

    for entry in walkdir(root) {
        if entry.extension().and_then(|s| s.to_str()) != Some(ext.as_str()) {
            continue;
        }
        total += 1;
        let bytes = match std::fs::read(&entry) {
            Ok(b) => b,
            Err(_) => continue,
        };
        // Reload layout per file (from_json consumes it).
        let layout = match TagLayout::from_json(def_path) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("layout load failed: {e}");
                std::process::exit(1);
            }
        };
        match read_classic_tag_file(&bytes, layout) {
            Ok(tag) => match tag.write_to_bytes() {
                Ok(re) if re == bytes => ok += 1,
                Ok(re) => {
                    let at = re.iter().zip(&bytes).position(|(a, b)| a != b);
                    if fails.len() < 10 {
                        fails.push(format!(
                            "MISMATCH {} (len {} vs {}, first diff @ {:?})",
                            entry.display(), re.len(), bytes.len(), at
                        ));
                    }
                }
                Err(e) => if fails.len() < 10 { fails.push(format!("WRITE-ERR {}: {e}", entry.display())) },
            },
            Err(e) => if fails.len() < 10 { fails.push(format!("READ-ERR {}: {e}", entry.display())) },
        }
    }

    for f in &fails {
        println!("{f}");
    }
    println!("\n{total} tags | {ok} full-file byte-exact | {} failed", total - ok);
    if ok != total {
        std::process::exit(1);
    }
}

fn walkdir(root: &str) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(root)];
    while let Some(dir) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() { stack.push(p); } else { out.push(p); }
            }
        }
    }
    out
}
