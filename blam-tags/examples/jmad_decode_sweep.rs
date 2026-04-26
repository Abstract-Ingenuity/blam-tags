//! Sweep `.model_animation_graph` tags and call `decode()` on every
//! animation. Tally success / failure by codec slot. Codecs that
//! aren't implemented (currently just `SharedStatic`) surface as
//! `UnsupportedCodec`; those aren't decode failures, just unimplemented
//! by design.
//!
//! Failure ledger written to `<output_dir>/jmad_decode_failures.txt`.
//!
//! Usage: jmad_decode_sweep <OUTPUT_DIR> <DIR> [<DIR>...]

use std::collections::BTreeMap;
use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use blam_tags::{Animation, AnimatedStreamStatus, AnimationError, TagFile};

fn collect_jmads(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() { return Ok(()); }
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
    let output_dir = PathBuf::from(args.next().ok_or("usage: jmad_decode_sweep <OUTPUT_DIR> <DIR> [<DIR>...]")?);
    let dirs: Vec<PathBuf> = args.map(PathBuf::from).collect();
    if dirs.is_empty() {
        eprintln!("usage: jmad_decode_sweep <OUTPUT_DIR> <DIR> [<DIR>...]");
        std::process::exit(2);
    }
    std::fs::create_dir_all(&output_dir)?;
    let failures_path = output_dir.join("jmad_decode_failures.txt");
    let mut failures = BufWriter::new(File::create(&failures_path)?);

    let mut paths = Vec::new();
    for dir in &dirs {
        eprintln!("scanning {}", dir.display());
        collect_jmads(dir, &mut paths)?;
    }
    eprintln!("found {} .model_animation_graph tags", paths.len());

    let mut animations_total = 0u64;
    let mut decode_ok = 0u64;
    let mut decode_unsupported = 0u64;
    let mut decode_no_payload = 0u64;
    let mut decode_other_err = 0u64;
    let mut by_codec_ok: BTreeMap<i16, u64> = BTreeMap::new();
    let mut by_codec_err: BTreeMap<i16, u64> = BTreeMap::new();
    let mut anim_status: BTreeMap<String, u64> = BTreeMap::new();

    for (i, path) in paths.iter().enumerate() {
        if i % 500 == 0 && i > 0 {
            eprintln!("  progress: {} / {}", i, paths.len());
        }
        let tag = match TagFile::read(path) { Ok(t) => t, Err(_) => continue };
        let animation = match Animation::new(&tag) { Ok(a) => a, Err(_) => continue };
        for g in animation.iter() {
            animations_total += 1;
            let codec_key = g.codec_byte.map(|b| b as i16).unwrap_or(-1);
            match g.decode() {
                Ok(clip) => {
                    decode_ok += 1;
                    *by_codec_ok.entry(codec_key).or_insert(0) += 1;
                    let key = match &clip.animated_status {
                        AnimatedStreamStatus::NoAnimatedStream => "static_only".to_string(),
                        AnimatedStreamStatus::Decoded => match &clip.animated_tracks {
                            Some(t) => format!("decoded {:?}", t.codec),
                            None => "decoded (no tracks)".to_string(),
                        },
                        AnimatedStreamStatus::Unsupported(c) => format!("unsupported {c:?}"),
                        AnimatedStreamStatus::Unknown(b) => format!("unknown_byte 0x{b:02x}"),
                    };
                    *anim_status.entry(key).or_insert(0) += 1;
                }
                Err(AnimationError::NoCodecPayload) => {
                    decode_no_payload += 1;
                }
                Err(AnimationError::UnsupportedCodec(_)) => {
                    decode_unsupported += 1;
                    *by_codec_err.entry(codec_key).or_insert(0) += 1;
                }
                Err(e) => {
                    decode_other_err += 1;
                    *by_codec_err.entry(codec_key).or_insert(0) += 1;
                    writeln!(failures, "{}\t[anim {}]\t{e}", path.display(), g.index)?;
                }
            }
        }
    }
    failures.flush()?;

    println!();
    println!("animations total      : {animations_total}");
    println!("decode ok             : {decode_ok}");
    println!("decode unsupported    : {decode_unsupported}  (codec not yet implemented)");
    println!("decode no payload     : {decode_no_payload}   (inherited)");
    println!("decode other error    : {decode_other_err}   (see failures list)");
    println!();
    println!("ok by codec:");
    for (k, v) in &by_codec_ok {
        println!("  {k:>3}  {v:>10}");
    }
    println!();
    println!("animated stream status (within decode-ok):");
    for (k, v) in &anim_status {
        println!("  {k:<32}  {v:>10}");
    }
    if !by_codec_err.is_empty() {
        println!();
        println!("non-ok by codec:");
        for (k, v) in &by_codec_err {
            println!("  {k:>3}  {v:>10}");
        }
    }
    println!();
    println!("failures list         : {}", failures_path.display());

    Ok(())
}
