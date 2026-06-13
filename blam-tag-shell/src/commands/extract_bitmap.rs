//! `extract-bitmap` — write each image of a `.bitmap` tag as a TIFF
//! or DDS file. Pure-tag-file extraction: pulls bytes from the tag's
//! `processed pixel data` blob (no resource-cache indirection).
//!
//! Format selection:
//!   - `--format tif` (default) — Tool-importable RGBA8 TIFF.
//!     Decompresses to BGRA-then-swizzled-to-RGBA. Phase 2 is
//!     2D-only; cube / array / 3D and BC-compressed formats error
//!     until later phases.
//!   - `--format dds` — legacy debug DDS dump. Preserves original
//!     pixel bytes (no decode); not re-importable into Tool.
//!
//! `--output` is overloaded based on what's passed:
//!   - ends in `.tif` / `.tiff` / `.dds` → write to that exact
//!     filename (single-image tags only). The extension picks the
//!     format and overrides `--format`.
//!   - any other path → directory target. 1-image tags emit
//!     `<dir>/<tag_stem>.<ext>`; N-image tags emit
//!     `<dir>/<tag_stem>/<i>.<ext>`.
//!   - omitted → directory target = current working directory.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use blam_tags::Bitmap;

use crate::context::CliContext;
use blam_tags::paths::tag_stem;

#[derive(Debug, Clone, Copy)]
enum OutFormat { Tif, Dds }

impl OutFormat {
    fn ext(self) -> &'static str {
        match self {
            Self::Tif => "tif",
            Self::Dds => "dds",
        }
    }
}

fn parse_format(s: &str) -> Result<OutFormat> {
    match s.to_ascii_lowercase().as_str() {
        "tif" | "tiff" => Ok(OutFormat::Tif),
        "dds" => Ok(OutFormat::Dds),
        other => anyhow::bail!("unknown --format `{other}`; expected `tif` or `dds`"),
    }
}

/// If a path's extension picks a format (e.g. user wrote
/// `--output foo.tif`), return it. Otherwise `None` (caller falls
/// back to the `--format` arg).
fn format_from_extension(path: &Path) -> Option<OutFormat> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "tif" | "tiff" => Some(OutFormat::Tif),
        "dds" => Some(OutFormat::Dds),
        _ => None,
    }
}

pub fn run(ctx: &mut CliContext, output: Option<&str>, format: &str) -> Result<()> {
    let cli_format = parse_format(format)?;

    let loaded = ctx.loaded("extract-bitmap")?;
    let stem = tag_stem(&loaded.path, "bitmap");

    // Classic Halo CE / Halo 2 bitmaps carry the artist *color plate* —
    // the lossless, re-importable source sheet. Always prefer it over the
    // compiled `processed pixel data` (a lossy DXT/mip/palettized
    // derivative): the source recovers every format including P8/p8-bump.
    // gen3+ MCC tags have no color plate and use the processed pixels;
    // a source-stripped classic tag also falls through to them.
    if loaded.tag.classic_engine().is_some() {
        if let Some(cp) =
            blam_tags::bitmap::color_plate(&loaded.tag).context("decode color plate")?
        {
            return write_color_plate(output, &stem, &cp, cli_format);
        }
    }

    let bitmap = Bitmap::new(&loaded.tag)
        .context("tag does not look like a .bitmap (no `bitmaps` block / `processed pixel data`)")?;

    let count = bitmap.len();
    if count == 0 {
        println!("no images in tag");
        return Ok(());
    }

    let output_path = output.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));

    // If the user named an explicit file with a recognized
    // extension, that picks both the destination and the format.
    if let Some(ext_format) = format_from_extension(&output_path) {
        return run_to_file(&output_path, &bitmap, count, ext_format);
    }

    // Otherwise treat as a directory and use the --format flag.
    run_to_dir(&output_path, &stem, &bitmap, count, cli_format)
}

/// Write a classic bitmap's single color plate. Mirrors the per-image
/// path's `--output` overloading: an `--output` ending in `.tif`/`.dds`
/// is the exact file (and its extension picks the format); any other
/// path is a directory (`<dir>/<stem>.<ext>`); omitted means
/// `./<stem>.<ext>` using the `--format` flag.
fn write_color_plate(
    output: Option<&str>,
    stem: &str,
    cp: &blam_tags::bitmap::ColorPlate,
    cli_format: OutFormat,
) -> Result<()> {
    let (target, format) = match output {
        Some(o) if format_from_extension(Path::new(o)).is_some() => {
            (PathBuf::from(o), format_from_extension(Path::new(o)).unwrap())
        }
        Some(o) => (Path::new(o).join(format!("{stem}.{}", cli_format.ext())), cli_format),
        None => (PathBuf::from(format!("{stem}.{}", cli_format.ext())), cli_format),
    };

    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
    }
    let file = File::create(&target).with_context(|| format!("create {}", target.display()))?;
    let mut writer = BufWriter::new(file);
    match format {
        OutFormat::Tif => cp.write_tiff(&mut writer)?,
        OutFormat::Dds => cp.write_dds(&mut writer)?,
    }
    println!("{}: {}×{} color plate (source)", target.display(), cp.width, cp.height);
    Ok(())
}

fn run_to_file(target: &Path, bitmap: &Bitmap<'_>, count: usize, format: OutFormat) -> Result<()> {
    if count > 1 {
        anyhow::bail!(
            "tag has {count} images; --output as a `.{ext}` filename only works for \
             single-image tags. Pass a directory path instead.",
            ext = format.ext(),
        );
    }
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
    }
    let image = bitmap.image(0).expect("count >= 1");
    let summary = write_one(target, image, format)?;
    println!("{}: {summary}", target.display());
    Ok(())
}

fn run_to_dir(dir: &Path, stem: &str, bitmap: &Bitmap<'_>, count: usize, format: OutFormat) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;

    // Per-image output dir for multi-image tags so siblings don't
    // collide on the same `<stem>.<ext>` filename.
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
            format!("{i}.{}", format.ext())
        } else {
            format!("{stem}.{}", format.ext())
        };
        let path = out_dir.join(&filename);

        match write_one(&path, image, format) {
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

fn write_one(path: &Path, image: blam_tags::BitmapImage<'_>, format: OutFormat) -> Result<String> {
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
    match format {
        OutFormat::Tif => image.write_tiff(&mut writer)?,
        OutFormat::Dds => image.write_dds(&mut writer)?,
    }
    Ok(summary)
}
