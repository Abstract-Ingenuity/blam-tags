//! `extract-bitmap` — write each image of a `.bitmap` tag as a DDS
//! file. Pure-tag-file extraction: pulls bytes from the tag's
//! `processed pixel data` blob (no resource-cache indirection).
//!
//! Output naming:
//!   - 1 image  → `<tag_stem>.dds`
//!   - N images → `<tag_stem>/<i>.dds`

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use blam_tags::Bitmap;

use crate::context::CliContext;

pub fn run(ctx: &mut CliContext, output_dir: Option<&str>) -> Result<()> {
    let loaded = ctx.loaded("extract-bitmap")?;
    let bitmap = Bitmap::new(&loaded.tag)
        .context("tag does not look like a .bitmap (no `bitmaps` block / `processed pixel data`)")?;

    let stem = tag_stem(&loaded.path);
    let out_root: PathBuf = match output_dir {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from("."),
    };
    fs::create_dir_all(&out_root)?;

    let count = bitmap.len();
    if count == 0 {
        println!("no images in tag");
        return Ok(());
    }

    // Per-image output dir for multi-image tags so we don't fight
    // siblings over the same `<stem>.dds` filename.
    let out_dir = if count > 1 {
        let d = out_root.join(&stem);
        fs::create_dir_all(&d)?;
        d
    } else {
        out_root.clone()
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
