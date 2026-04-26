//! `extract-bitmap` — write each image of a `.bitmap` tag as a DDS
//! file. Pure-tag-file extraction: pulls bytes from the tag's
//! `processed pixel data` blob (no resource-cache indirection).
//!
//! `--output` is overloaded based on what's passed:
//!   - ends in `.dds` → write to that exact file (single-image tags
//!     only — multi-image tags can't all go to one filename).
//!   - any other path → directory target. 1-image tags emit
//!     `<dir>/<tag_stem>.dds`; N-image tags emit
//!     `<dir>/<tag_stem>/<i>.dds`.
//!   - omitted → directory target = current working directory.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use blam_tags::Bitmap;

use crate::context::CliContext;

pub fn run(ctx: &mut CliContext, output: Option<&str>) -> Result<()> {
    let loaded = ctx.loaded("extract-bitmap")?;
    let bitmap = Bitmap::new(&loaded.tag)
        .context("tag does not look like a .bitmap (no `bitmaps` block / `processed pixel data`)")?;

    let count = bitmap.len();
    if count == 0 {
        println!("no images in tag");
        return Ok(());
    }

    let stem = tag_stem(&loaded.path);
    let output_path = output.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));

    if is_dds_filename(&output_path) {
        return run_to_file(&output_path, &bitmap, count);
    }
    run_to_dir(&output_path, &stem, &bitmap, count)
}

fn run_to_file(target: &Path, bitmap: &Bitmap<'_>, count: usize) -> Result<()> {
    if count > 1 {
        anyhow::bail!(
            "tag has {count} images; --output as a `.dds` filename only works for \
             single-image tags. Pass a directory path instead."
        );
    }
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
    }
    let image = bitmap.image(0).expect("count >= 1");
    let summary = write_one(target, image)?;
    println!("{}: {summary}", target.display());
    Ok(())
}

fn run_to_dir(dir: &Path, stem: &str, bitmap: &Bitmap<'_>, count: usize) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;

    // Per-image output dir for multi-image tags so siblings don't
    // collide on the same `<stem>.dds` filename.
    let out_dir = if count > 1 {
        let d = dir.join(stem);
        fs::create_dir_all(&d).with_context(|| format!("create {}", d.display()))?;
        d
    } else {
        dir.to_path_buf()
    };

    let mut errors = 0usize;
    for (i, image) in bitmap.iter().enumerate() {
        let filename = if count > 1 {
            format!("{i}.dds")
        } else {
            format!("{stem}.dds")
        };
        let path = out_dir.join(&filename);

        match write_one(&path, image) {
            Ok(summary) => println!("{}: {summary}", path.display()),
            Err(e) => {
                eprintln!("{}: error: {e}", path.display());
                errors += 1;
            }
        }
    }

    if errors > 0 {
        anyhow::bail!("{errors} of {count} images failed");
    }
    Ok(())
}

fn is_dds_filename(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("dds"))
        .unwrap_or(false)
}

fn write_one(path: &Path, image: blam_tags::BitmapImage<'_>) -> Result<String> {
    let format_name = image.format_name().unwrap_or_else(|| "?".to_string());
    let type_name = image.type_name().unwrap_or_else(|| "?".to_string());
    let summary = format!(
        "{}×{} {} ({}, {} mip{})",
        image.width(),
        image.height(),
        format_name,
        type_name,
        image.mipmap_levels(),
        if image.mipmap_levels() == 1 { "" } else { "s" },
    );

    let file = File::create(path)
        .with_context(|| format!("create {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    image.write_dds(&mut writer)?;
    Ok(summary)
}

fn tag_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("bitmap")
        .to_owned()
}
