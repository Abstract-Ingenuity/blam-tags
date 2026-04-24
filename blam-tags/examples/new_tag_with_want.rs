//! End-to-end smoke test: create a new tag from schema, attach an
//! empty want stream, optionally run rebuild_dependency_list, write
//! to disk, re-read, and print a summary.
//!
//! Usage:
//! ```text
//! cargo run --release -p blam-tags --example new_tag_with_want -- \
//!     <group_schema.json> <tag_dependency_list.json> <output.tag> [--rebuild]
//! ```

use std::error::Error;
use std::path::PathBuf;

use blam_tags::TagFile;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let group_schema = PathBuf::from(args.next().ok_or("group schema path required")?);
    let want_schema = PathBuf::from(args.next().ok_or("want schema path required")?);
    let out = PathBuf::from(args.next().ok_or("output path required")?);
    let rebuild = args.any(|a| a == "--rebuild");

    println!("creating tag from {}", group_schema.display());
    let mut tag = TagFile::new(&group_schema)?;

    println!("attaching empty want from {}", want_schema.display());
    tag.add_dependency_list(&want_schema)?;

    if rebuild {
        println!("rebuilding want from data...");
        tag.rebuild_dependency_list(&want_schema)?;
    }

    println!("writing to {}", out.display());
    tag.write(&out)?;

    println!("re-reading {}", out.display());
    let loaded = TagFile::read(&out)?;

    let sig_bytes = loaded.header.group_tag.to_be_bytes();
    println!(
        "  group_tag = {}  group_version = {}",
        std::str::from_utf8(&sig_bytes).unwrap_or("????"),
        loaded.header.group_version
    );

    let deps = loaded
        .dependency_list()
        .and_then(|r| r.field_path("dependencies"))
        .and_then(|f| f.as_block().map(|b| b.len()))
        .unwrap_or(0);
    println!(
        "  dependency_list_stream: {} ({} entries)",
        if loaded.dependency_list().is_some() { "present" } else { "absent" },
        deps
    );
    println!(
        "  import_info_stream: {}",
        if loaded.import_info().is_some() { "present" } else { "absent" }
    );

    Ok(())
}
