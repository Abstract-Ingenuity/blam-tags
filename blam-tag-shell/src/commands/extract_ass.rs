//! `extract-ass` — extract a `.scenario` tag's structure BSPs as ASS
//! files (Bungie Amalgam, version 7 for H3) — the static-scene
//! counterpart to JMS.
//!
//! Walks `scenario.structure_bsps[]` — each entry references a
//! `.scenario_structure_bsp` plus a paired
//! `.scenario_structure_lighting_info` (.stli). One ASS file is
//! emitted per BSP, mirroring H3EK's `data/levels/<map>/structure/<bsp>.ASS`
//! source-tree convention.
//!
//! Routing through the scenario is mandatory: the lighting (and
//! eventually the per-BSP design data, sky bindings, etc) all hang
//! off the scenario's structure_bsps entries — the sbsp tag alone
//! doesn't know its own stli/sky/design pairings.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::PathBuf;

use anyhow::{Context, Result};
use blam_tags::{AssFile, TagFile};

use crate::context::CliContext;
use crate::paths::{derive_tags_root, resolve_tag_path, tag_ref_path, tag_stem};

pub fn run(ctx: &mut CliContext, output: Option<&str>, flat: bool) -> Result<()> {
    let loaded = ctx.loaded("extract-ass")?;
    let group = loaded.tag.header.group_tag.to_be_bytes();
    if &group != b"scnr" {
        anyhow::bail!(
            "extract-ass requires a `.scenario` (scnr) input — got group `{}`. \
             Tag-direct extraction is available via the library function \
             `AssFile::from_scenario_structure_bsp` for sbsp tags.",
            std::str::from_utf8(&group).unwrap_or("?"),
        );
    }

    let tags_root = derive_tags_root(&loaded.path)
        .context("failed to derive tags root from input path — input must live under a `tags/` directory")?;
    let scenario_stem = tag_stem(&loaded.path, "scenario");
    let out_root = output.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));

    let bsps_block = loaded.tag.root().field_path("structure bsps").and_then(|f| f.as_block())
        .context("scenario has no `structure bsps` block")?;
    if bsps_block.len() == 0 {
        anyhow::bail!("scenario has zero structure_bsps entries — nothing to extract");
    }

    let mut emitted = Vec::new();
    let mut warnings = Vec::new();

    for bi in 0..bsps_block.len() {
        let entry = bsps_block.element(bi).unwrap();
        let bsp_ref_path = tag_ref_path(&entry, "structure bsp");
        let lighting_ref_path = tag_ref_path(&entry, "structure lighting_info");

        let Some(bsp_rel) = bsp_ref_path else {
            warnings.push(format!("structure_bsps[{bi}]: no structure_bsp ref — skipped"));
            continue;
        };
        let bsp_abs = resolve_tag_path(&tags_root, &bsp_rel, "scenario_structure_bsp");
        let bsp_tag = match TagFile::read(&bsp_abs) {
            Ok(t) => t,
            Err(e) => {
                warnings.push(format!("structure_bsps[{bi}]: read {} failed — {}", bsp_abs.display(), e));
                continue;
            }
        };

        let mut ass = AssFile::from_scenario_structure_bsp(&bsp_tag)
            .with_context(|| format!("structure_bsps[{bi}]: build ASS from {}", bsp_abs.display()))?;

        // Layer in lighting from the paired stli tag.
        if let Some(lighting_rel) = lighting_ref_path {
            let lighting_abs = resolve_tag_path(&tags_root, &lighting_rel, "scenario_structure_lighting_info");
            match TagFile::read(&lighting_abs) {
                Ok(stli) => {
                    if let Err(e) = ass.add_lights_from_stli(&stli) {
                        warnings.push(format!("structure_bsps[{bi}]: lighting layer failed — {e}"));
                    }
                }
                Err(e) => warnings.push(format!(
                    "structure_bsps[{bi}]: lighting tag {} unreadable — {e}", lighting_abs.display()
                )),
            }
        } else {
            warnings.push(format!("structure_bsps[{bi}]: no lighting_info ref — emitting without lights"));
        }

        let bsp_stem = bsp_abs.file_stem().and_then(|s| s.to_str()).unwrap_or("bsp").to_owned();
        let path = if flat {
            out_root.join(format!("{scenario_stem}.{bsp_stem}.ass"))
        } else {
            out_root.join(&scenario_stem).join("structure").join(format!("{bsp_stem}.ASS"))
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let mut writer = BufWriter::new(File::create(&path)
            .with_context(|| format!("create {}", path.display()))?);
        ass.write(&mut writer)?;
        let total_verts: usize = ass.objects.iter().map(|o| o.vertices_len()).sum();
        let total_tris: usize = ass.objects.iter().map(|o| o.triangles_len()).sum();
        let light_count = ass.objects.iter().filter(|o| matches!(&o.payload, blam_tags::AssObjectPayload::GenericLight(_))).count();
        emitted.push((bi, path, format!(
            "{} mats, {} objects ({} lights), {} instances, {} verts, {} tris",
            ass.materials.len(), ass.objects.len(), light_count,
            ass.instances.len(), total_verts, total_tris,
        )));
    }

    for (bi, path, summary) in &emitted {
        println!("{}: [bsp{bi}] {}", path.display(), summary);
    }
    for w in &warnings {
        eprintln!("warning: {w}");
    }
    if emitted.is_empty() {
        anyhow::bail!("no ASS files emitted — all structure_bsps entries failed to load");
    }
    Ok(())
}

