//! Walks every `.scenario` tag under the given roots and validates
//! that `extract-ass` reconstructs an ASS for every `structure_bsps[]`
//! entry without errors. Reports per-scenario BSP counts + aggregate
//! stats. Doesn't write files — purely an in-memory sweep.
//!
//! Surfaces failures at any of: scenario read, sbsp resolution, sbsp
//! read, ASS build, stli load.
//!
//! Usage: ass_corpus_sweep <DIR> [<DIR>...]

use std::error::Error;
use std::path::{Path, PathBuf};

use blam_tags::{AssFile, AssObjectPayload, TagFieldData, TagFile};

fn main() -> Result<(), Box<dyn Error>> {
    let dirs: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if dirs.is_empty() {
        return Err("usage: ass_corpus_sweep <DIR> [<DIR>...]".into());
    }

    let mut paths = Vec::new();
    for d in &dirs { collect_scenarios(d, &mut paths); }
    paths.sort();
    eprintln!("scanning {} scenarios", paths.len());

    let mut stats = SweepStats::default();
    for p in &paths {
        process(p, &mut stats);
    }
    stats.report();
    Ok(())
}

fn collect_scenarios(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return; };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() { collect_scenarios(&p, out); }
        else if p.extension().and_then(|s| s.to_str()) == Some("scenario") {
            out.push(p);
        }
    }
}

fn process(path: &Path, stats: &mut SweepStats) {
    stats.total_scenarios += 1;
    let scenario = match TagFile::read(path) {
        Ok(t) => t,
        Err(e) => {
            stats.scenario_read_failed += 1;
            stats.failure_examples.push(format!("scenario read: {} — {}", path.display(), e));
            return;
        }
    };
    let Some(tags_root) = derive_tags_root(path) else {
        stats.no_tags_root += 1;
        return;
    };
    let Some(bsps) = scenario.root().field_path("structure bsps").and_then(|f| f.as_block()) else {
        stats.no_structure_bsps_block += 1;
        return;
    };
    if bsps.len() == 0 {
        stats.zero_structure_bsps += 1;
        return;
    }
    *stats.scenarios_by_bsp_count.entry(bsps.len()).or_insert(0) += 1;
    stats.total_bsps += bsps.len();

    for bi in 0..bsps.len() {
        let entry = bsps.element(bi).unwrap();
        let bsp_ref = ref_path(&entry, "structure bsp");
        let lighting_ref = ref_path(&entry, "structure lighting_info");
        let Some(bsp_rel) = bsp_ref else {
            stats.bsp_no_ref += 1;
            continue;
        };
        let bsp_abs = resolve(&tags_root, &bsp_rel, "scenario_structure_bsp");
        let bsp_tag = match TagFile::read(&bsp_abs) {
            Ok(t) => t,
            Err(e) => {
                stats.bsp_read_failed += 1;
                stats.failure_examples.push(format!("bsp read: {} — {}", bsp_abs.display(), e));
                continue;
            }
        };
        let mut ass = match AssFile::from_scenario_structure_bsp(&bsp_tag) {
            Ok(a) => a,
            Err(e) => {
                stats.ass_build_failed += 1;
                stats.failure_examples.push(format!("ASS build: {} — {}", bsp_abs.display(), e));
                continue;
            }
        };
        let mut had_lights = false;
        if let Some(lighting_rel) = lighting_ref {
            let lighting_abs = resolve(&tags_root, &lighting_rel, "scenario_structure_lighting_info");
            match TagFile::read(&lighting_abs) {
                Ok(stli) => {
                    if let Err(e) = ass.add_lights_from_stli(&stli) {
                        stats.lighting_layer_failed += 1;
                        stats.failure_examples.push(format!("lighting: {} — {}", lighting_abs.display(), e));
                    } else {
                        had_lights = true;
                    }
                }
                Err(e) => {
                    stats.lighting_read_failed += 1;
                    stats.failure_examples.push(format!("lighting tag {}: {}", lighting_abs.display(), e));
                }
            }
        } else {
            stats.lighting_missing_ref += 1;
        }

        stats.bsps_built_ok += 1;
        if had_lights { stats.bsps_with_lights += 1; }
        let mesh_count = ass.objects.iter().filter(|o| matches!(&o.payload, AssObjectPayload::Mesh { .. })).count();
        let light_count = ass.objects.iter().filter(|o| matches!(&o.payload, AssObjectPayload::GenericLight(_))).count();
        let total_verts: usize = ass.objects.iter().map(|o| o.vertices_len()).sum();
        let total_tris: usize = ass.objects.iter().map(|o| o.triangles_len()).sum();
        stats.total_meshes += mesh_count;
        stats.total_lights += light_count;
        stats.total_instances += ass.instances.len();
        stats.total_verts += total_verts as u64;
        stats.total_tris += total_tris as u64;
    }
}

fn ref_path(entry: &blam_tags::TagStruct<'_>, field: &str) -> Option<String> {
    let f = entry.field(field)?;
    let TagFieldData::TagReference(r) = f.value()? else { return None };
    let (_g, p) = r.group_tag_and_name?;
    if p.is_empty() { None } else { Some(p) }
}

fn resolve(tags_root: &Path, rel: &str, ext: &str) -> PathBuf {
    let rel_path: PathBuf = rel.split('\\').collect();
    let mut p = tags_root.join(&rel_path);
    p.set_extension(ext);
    p
}

fn derive_tags_root(path: &Path) -> Option<PathBuf> {
    let abs = path.canonicalize().ok()?;
    let mut acc = PathBuf::new();
    let mut found = None;
    for component in abs.components() {
        acc.push(component);
        if matches!(component, std::path::Component::Normal(s) if s == "tags") {
            found = Some(acc.clone());
        }
    }
    found
}

#[derive(Default)]
struct SweepStats {
    total_scenarios: usize,
    scenario_read_failed: usize,
    no_tags_root: usize,
    no_structure_bsps_block: usize,
    zero_structure_bsps: usize,
    scenarios_by_bsp_count: std::collections::BTreeMap<usize, usize>,

    total_bsps: usize,
    bsp_no_ref: usize,
    bsp_read_failed: usize,
    ass_build_failed: usize,
    bsps_built_ok: usize,
    bsps_with_lights: usize,
    lighting_missing_ref: usize,
    lighting_read_failed: usize,
    lighting_layer_failed: usize,

    total_meshes: usize,
    total_lights: usize,
    total_instances: usize,
    total_verts: u64,
    total_tris: u64,
    failure_examples: Vec<String>,
}

impl SweepStats {
    fn report(&self) {
        eprintln!();
        eprintln!("=== scenarios ===");
        eprintln!("  total:                  {}", self.total_scenarios);
        eprintln!("  read failed:            {}", self.scenario_read_failed);
        eprintln!("  no tags root derived:   {}", self.no_tags_root);
        eprintln!("  no structure_bsps blk:  {}", self.no_structure_bsps_block);
        eprintln!("  zero structure_bsps:    {}", self.zero_structure_bsps);
        eprintln!("  by BSP count:");
        for (n, c) in &self.scenarios_by_bsp_count {
            eprintln!("    {n}-bsp scenarios:     {c}");
        }
        eprintln!();
        eprintln!("=== BSPs (across all scenarios) ===");
        eprintln!("  total BSP refs:         {}", self.total_bsps);
        eprintln!("  no bsp ref (skipped):   {}", self.bsp_no_ref);
        eprintln!("  bsp read failed:        {}", self.bsp_read_failed);
        eprintln!("  ASS build failed:       {}", self.ass_build_failed);
        eprintln!("  ASS built ok:           {} ({:.1}%)", self.bsps_built_ok, pct(self.bsps_built_ok, self.total_bsps));
        eprintln!("  with stli lights:       {}", self.bsps_with_lights);
        eprintln!("  lighting missing ref:   {}", self.lighting_missing_ref);
        eprintln!("  lighting read failed:   {}", self.lighting_read_failed);
        eprintln!("  lighting layer failed:  {}", self.lighting_layer_failed);
        eprintln!();
        eprintln!("=== aggregate output ===");
        eprintln!("  MESH OBJECTs:           {}", self.total_meshes);
        eprintln!("  GENERIC_LIGHT OBJECTs:  {}", self.total_lights);
        eprintln!("  INSTANCEs:              {}", self.total_instances);
        eprintln!("  vertices:               {}", self.total_verts);
        eprintln!("  triangles:              {}", self.total_tris);

        if !self.failure_examples.is_empty() {
            eprintln!();
            eprintln!("=== first 30 issues ===");
            for ex in self.failure_examples.iter().take(30) {
                eprintln!("  {ex}");
            }
            if self.failure_examples.len() > 30 {
                eprintln!("  ... and {} more", self.failure_examples.len() - 30);
            }
        }
    }
}

fn pct(n: usize, d: usize) -> f64 {
    if d == 0 { 0.0 } else { 100.0 * n as f64 / d as f64 }
}
