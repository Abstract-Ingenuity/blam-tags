//! Sweep `.model_animation_graph` tags and tally per-codec coverage,
//! recording any that we can't parse cleanly.
//!
//! Outputs:
//! - stdout: per-codec frequency table, plus tag-read / animation-walk
//!   failure counts.
//! - `<output_dir>/jmad_unsupported_layout.txt`: one line per tag that
//!   either failed `Animation::new` or surfaced anomalies (suspicious
//!   blob length vs `data sizes` total, codec byte ∉ 0..=11). Skipped
//!   tags from the no-tgrc list aren't included — they're handled by
//!   the existing `jmad_missing_tgrc` example.
//!
//! Usage: jmad_layout_sweep <OUTPUT_DIR> <DIR> [<DIR>...]

use std::collections::BTreeMap;
use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use blam_tags::{Animation, TagFile};

fn collect_jmads(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_jmads(&path, out)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("model_animation_graph") {
            out.push(path);
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let output_dir = PathBuf::from(args.next().ok_or("usage: jmad_layout_sweep <OUTPUT_DIR> <DIR> [<DIR>...]")?);
    let dirs: Vec<PathBuf> = args.map(PathBuf::from).collect();
    if dirs.is_empty() {
        eprintln!("usage: jmad_layout_sweep <OUTPUT_DIR> <DIR> [<DIR>...]");
        std::process::exit(2);
    }
    std::fs::create_dir_all(&output_dir)?;
    let unsupported_path = output_dir.join("jmad_unsupported_layout.txt");
    let mut unsupported = BufWriter::new(File::create(&unsupported_path)?);

    let mut paths = Vec::new();
    for dir in &dirs {
        eprintln!("scanning {}", dir.display());
        collect_jmads(dir, &mut paths)?;
    }
    eprintln!("found {} .model_animation_graph tags", paths.len());

    let mut tags_total = 0u64;
    let mut tags_read_failed = 0u64;
    let mut tags_anim_walk_failed = 0u64;
    let mut tags_inheriting = 0u64;
    let mut tags_with_anomalies = 0u64;
    let mut animations_total = 0u64;
    let mut animations_unresolved = 0u64;
    let mut by_codec: BTreeMap<i16, u64> = BTreeMap::new();
    let mut by_anim_codec: BTreeMap<i16, u64> = BTreeMap::new();
    let mut size_mismatch_samples: Vec<(PathBuf, usize, i64, usize)> = Vec::new();
    let mut bad_codec_samples: Vec<(PathBuf, usize, u8)> = Vec::new();

    for (i, path) in paths.iter().enumerate() {
        if i % 500 == 0 && i > 0 {
            eprintln!("  progress: {} / {}", i, paths.len());
        }
        tags_total += 1;

        let tag = match TagFile::read(path) {
            Ok(t) => t,
            Err(e) => {
                tags_read_failed += 1;
                writeln!(unsupported, "{}\tread_failed\t{e}", path.display())?;
                continue;
            }
        };

        let animation = match Animation::new(&tag) {
            Ok(a) => a,
            Err(e) => {
                tags_anim_walk_failed += 1;
                writeln!(unsupported, "{}\tanim_walk_failed\t{e}", path.display())?;
                continue;
            }
        };

        if animation.is_empty() && animation.parent().is_some() {
            tags_inheriting += 1;
            // Not an error — don't write to unsupported list.
        }

        let mut tag_anomalies = 0u32;
        for g in animation.iter() {
            animations_total += 1;
            if g.checksum.is_none() {
                animations_unresolved += 1;
            }
            // Codec byte tally — keyed as i16 so `-1` represents "no payload".
            let key = g.codec_byte.map(|b| b as i16).unwrap_or(-1);
            *by_codec.entry(key).or_insert(0) += 1;
            // Animated stream codec byte (the one after the static
            // rest-pose stream). -1 = static-only / no animated data.
            let anim_key = g.animated_codec_byte().map(|b| b as i16).unwrap_or(-1);
            *by_anim_codec.entry(anim_key).or_insert(0) += 1;

            if let Some(c) = g.codec_byte {
                if c > 11 {
                    tag_anomalies += 1;
                    if bad_codec_samples.len() < 25 {
                        bad_codec_samples.push((path.clone(), g.index, c));
                    }
                }
            }
            if let Some(sizes) = &g.data_sizes {
                let want = sizes.total();
                let have = g.blob.len() as i64;
                if want != have && have > 0 {
                    tag_anomalies += 1;
                    if size_mismatch_samples.len() < 25 {
                        size_mismatch_samples.push((path.clone(), g.index, want, have as usize));
                    }
                }
            }
        }
        if tag_anomalies > 0 {
            tags_with_anomalies += 1;
            writeln!(unsupported, "{}\tanomalies\t{tag_anomalies}", path.display())?;
        }
    }

    unsupported.flush()?;

    println!();
    println!("tags scanned        : {tags_total}");
    println!("read failed         : {tags_read_failed}");
    println!("anim walk failed    : {tags_anim_walk_failed}");
    println!("inheriting (empty)  : {tags_inheriting}");
    println!("with anomalies      : {tags_with_anomalies}");
    println!("animations total    : {animations_total}");
    println!("animations unres.   : {animations_unresolved}");
    println!();
    println!("codec frequency:");
    for (codec, count) in &by_codec {
        let label = match *codec {
            -1 => "no payload (inherited)".to_string(),
            0 => "0  no_compression".to_string(),
            1 => "1  uncompressed_static".to_string(),
            2 => "2  uncompressed_animated".to_string(),
            3 => "3  8byte_quantized_rotation_only".to_string(),
            4 => "4  byte_keyframe_lightly_quantized".to_string(),
            5 => "5  word_keyframe_lightly_quantized".to_string(),
            6 => "6  reverse_byte_keyframe".to_string(),
            7 => "7  reverse_word_keyframe".to_string(),
            8 => "8  blend_screen".to_string(),
            9 => "9  curve (Reach+)".to_string(),
            10 => "10 revised_curve (Reach+)".to_string(),
            11 => "11 shared_static (HO+)".to_string(),
            n => format!("?? unknown ({n})"),
        };
        println!("  {label:<35}  {count:>10}");
    }

    println!();
    println!("animated-stream codec frequency (byte at default_data offset):");
    for (codec, count) in &by_anim_codec {
        let label = match *codec {
            -1 => "no animated stream".to_string(),
            0 => "0  no_compression".to_string(),
            1 => "1  uncompressed_static".to_string(),
            2 => "2  uncompressed_animated".to_string(),
            3 => "3  8byte_quantized_rotation_only".to_string(),
            4 => "4  byte_keyframe_lightly_quantized".to_string(),
            5 => "5  word_keyframe_lightly_quantized".to_string(),
            6 => "6  reverse_byte_keyframe".to_string(),
            7 => "7  reverse_word_keyframe".to_string(),
            8 => "8  blend_screen".to_string(),
            9 => "9  curve (Reach+)".to_string(),
            10 => "10 revised_curve (Reach+)".to_string(),
            11 => "11 shared_static (HO+)".to_string(),
            n => format!("?? unknown ({n})"),
        };
        println!("  {label:<35}  {count:>10}");
    }

    if !size_mismatch_samples.is_empty() {
        println!();
        println!("data-sizes total != blob length (first {}):", size_mismatch_samples.len());
        for (path, idx, want, have) in &size_mismatch_samples {
            println!("  {} [anim {idx}] want={want} have={have}", path.display());
        }
    }
    if !bad_codec_samples.is_empty() {
        println!();
        println!("codec byte ∉ 0..=11 (first {}):", bad_codec_samples.len());
        for (path, idx, c) in &bad_codec_samples {
            println!("  {} [anim {idx}] codec={c}", path.display());
        }
    }

    println!();
    println!("unsupported list    : {}", unsupported_path.display());

    Ok(())
}
