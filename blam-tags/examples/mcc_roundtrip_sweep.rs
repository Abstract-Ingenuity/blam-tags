//! Continuing MCC roundtrip sweep: read every tag under <root>, write it
//! back via `TagFile::write_to_bytes`, and byte-compare. Unlike
//! `roundtrip.rs` this does NOT abort — it tallies ok / mismatch /
//! read-error and lists the first N of each. A *mismatch* is a real
//! regression; a *read-error* is usually a malformed/truncated tag MCC
//! ships (orthogonal to write correctness).
//!
//! Usage: mcc_roundtrip_sweep <tags-root> [max-list]

use blam_tags::TagFile;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(&args[1]);
    let max_list: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20);

    let (mut total, mut ok, mut mismatch, mut read_err, mut panics) = (0usize, 0, 0, 0, 0);
    let mut mismatches: Vec<String> = Vec::new();
    let mut read_errs: Vec<String> = Vec::new();

    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            // Only attempt files that look like tags (have an extension and
            // a plausible size). TagFile::read rejects non-tags cleanly.
            let Ok(meta) = p.metadata() else { continue };
            if meta.len() < 64 {
                continue;
            }
            total += 1;
            let rel = p.strip_prefix(&root).unwrap_or(&p).display().to_string();
            let result = catch_unwind(AssertUnwindSafe(|| {
                let tf = TagFile::read(&p)?;
                let src = std::fs::read(&p)?;
                let out = tf.write_to_bytes()?;
                Ok::<bool, Box<dyn std::error::Error>>(out == src)
            }));
            match result {
                Ok(Ok(true)) => ok += 1,
                Ok(Ok(false)) => {
                    mismatch += 1;
                    if mismatches.len() < max_list {
                        mismatches.push(rel);
                    }
                }
                Ok(Err(err)) => {
                    read_err += 1;
                    if read_errs.len() < max_list {
                        read_errs.push(format!("{rel} :: {err}"));
                    }
                }
                Err(_) => {
                    panics += 1;
                    if mismatches.len() < max_list {
                        mismatches.push(format!("{rel} :: PANIC"));
                    }
                }
            }
        }
    }

    println!(
        "\n=== {total} tags | {ok} byte-exact | {mismatch} MISMATCH | {read_err} read-error | {panics} panic ==="
    );
    if !mismatches.is_empty() {
        println!("--- mismatches/panics (regressions) ---");
        for m in &mismatches {
            println!("  {m}");
        }
    }
    if !read_errs.is_empty() {
        println!("--- read-errors (first {}) ---", read_errs.len());
        for r in &read_errs {
            println!("  {r}");
        }
    }
    if mismatch + panics > 0 {
        std::process::exit(1);
    }
}
