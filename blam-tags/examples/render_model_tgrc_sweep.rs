//! Sweep `.render_model` tags and tally how many carry a `tgrc`
//! (Exploded) resource — i.e. an out-of-tag vertex/index buffer
//! payload that any future JMS extractor would need to walk.
//!
//! Reports per-engine breakdown:
//! - tags with at least one tgrc resource (typical case for visible meshes)
//! - tags with only Null / Xsync resources (no out-of-tag payload)
//! - tags with no resource fields at all (rare; some render_model variants)
//! - read failures
//!
//! Writes the no-tgrc paths to `<output>/render_model_no_tgrc.txt`
//! for follow-up investigation.
//!
//! Usage: render_model_tgrc_sweep <OUTPUT_DIR> <DIR> [<DIR>...]

use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use blam_tags::{TagFile, TagResourceKind, TagStruct};

#[derive(Default)]
struct Counts {
    total: u64,
    read_failed: u64,
    has_tgrc: u64,
    only_null_or_xsync: u64,
    no_resource_fields: u64,
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() { return Ok(()); }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("render_model") {
            out.push(path);
        }
    }
    Ok(())
}

/// Walk every nested struct/block/array, classifying each tag_resource
/// field encountered. Returns `(saw_resource_field, saw_tgrc)`.
fn classify_resources(s: TagStruct<'_>) -> (bool, bool) {
    let mut saw_field = false;
    let mut saw_tgrc = false;
    visit(s, &mut saw_field, &mut saw_tgrc);
    (saw_field, saw_tgrc)
}

fn visit(s: TagStruct<'_>, saw_field: &mut bool, saw_tgrc: &mut bool) {
    for field in s.fields() {
        if let Some(resource) = field.as_resource() {
            *saw_field = true;
            if matches!(resource.kind(), TagResourceKind::Exploded) {
                *saw_tgrc = true;
            }
            // Walk into the exploded resource's header struct too;
            // some render_models nest further resources inside.
            if let Some(nested) = resource.as_struct() {
                visit(nested, saw_field, saw_tgrc);
            }
        } else if let Some(nested) = field.as_struct() {
            visit(nested, saw_field, saw_tgrc);
        } else if let Some(block) = field.as_block() {
            for elem in block.iter() {
                visit(elem, saw_field, saw_tgrc);
            }
        } else if let Some(array) = field.as_array() {
            for elem in array.iter() {
                visit(elem, saw_field, saw_tgrc);
            }
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let output_dir = PathBuf::from(args.next().ok_or("usage: render_model_tgrc_sweep <OUTPUT_DIR> <DIR>...")?);
    let dirs: Vec<PathBuf> = args.map(PathBuf::from).collect();
    if dirs.is_empty() {
        eprintln!("usage: render_model_tgrc_sweep <OUTPUT_DIR> <DIR>...");
        std::process::exit(2);
    }
    std::fs::create_dir_all(&output_dir)?;
    let no_tgrc_path = output_dir.join("render_model_no_tgrc.txt");
    let mut no_tgrc = BufWriter::new(File::create(&no_tgrc_path)?);

    let mut overall = Counts::default();
    let mut per_engine: Vec<(String, Counts)> = Vec::new();
    for dir in &dirs {
        eprintln!("scanning {}", dir.display());
        let engine_label = dir.components()
            .filter_map(|c| c.as_os_str().to_str())
            .find(|s| s.ends_with("_mcc") || s.ends_with("_xbox360") || s.ends_with("_xbox"))
            .unwrap_or("unknown")
            .to_owned();

        let mut paths = Vec::new();
        collect(dir, &mut paths)?;
        eprintln!("  found {} render_model tags", paths.len());

        let mut counts = Counts::default();
        for (i, path) in paths.iter().enumerate() {
            if i % 500 == 0 && i > 0 {
                eprintln!("    progress: {} / {}", i, paths.len());
            }
            counts.total += 1;
            overall.total += 1;
            let tag = match TagFile::read(path) {
                Ok(t) => t,
                Err(e) => {
                    counts.read_failed += 1;
                    overall.read_failed += 1;
                    eprintln!("  read failed: {} ({e})", path.display());
                    continue;
                }
            };
            let (saw_field, saw_tgrc) = classify_resources(tag.root());
            if saw_tgrc {
                counts.has_tgrc += 1;
                overall.has_tgrc += 1;
            } else if saw_field {
                counts.only_null_or_xsync += 1;
                overall.only_null_or_xsync += 1;
                writeln!(no_tgrc, "{}", path.display())?;
            } else {
                counts.no_resource_fields += 1;
                overall.no_resource_fields += 1;
                writeln!(no_tgrc, "{}", path.display())?;
            }
        }
        per_engine.push((engine_label, counts));
    }
    no_tgrc.flush()?;

    println!();
    let print = |label: &str, c: &Counts| {
        let pct = |n: u64| if c.total > 0 { 100.0 * n as f64 / c.total as f64 } else { 0.0 };
        println!("== {label} ==");
        println!("  total                  : {}", c.total);
        println!("  read failed            : {}", c.read_failed);
        println!("  has tgrc resource      : {:>6}  ({:.2}%)", c.has_tgrc, pct(c.has_tgrc));
        println!("  null/xsync only        : {:>6}  ({:.2}%)", c.only_null_or_xsync, pct(c.only_null_or_xsync));
        println!("  no resource fields     : {:>6}  ({:.2}%)", c.no_resource_fields, pct(c.no_resource_fields));
    };
    for (label, c) in &per_engine {
        print(label, c);
    }
    if per_engine.len() > 1 {
        println!();
        print("ALL", &overall);
    }
    println!();
    println!("no-tgrc list           : {}", no_tgrc_path.display());
    Ok(())
}
