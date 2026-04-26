//! Reconstructs every `.render_model` under the given root(s) into
//! an in-memory `JmsFile`, parses the embedded source JMS in
//! `import_info` where present, and reports per-tag and aggregate
//! agreement metrics. Used to validate the geometry-walk approach
//! across the corpus.
//!
//! Usage: jms_corpus_sweep <DIR> [<DIR>...]
//!
//! Per-tag the sweep records:
//! - reconstruction success/failure (with the JmsError if it failed)
//! - whether an embedded source JMS exists and parses cleanly
//! - count agreement (vertices, triangles, materials, markers)
//! - position-bbox match (within 0.5 units)
//! - position-set overlap (rounded to 1 decimal place — generous to
//!   floating-point noise from bounds dequantization)
//!
//! Embedded source JMS: H3 render_models keep the artist's original
//! JMS zlib-compressed in `import_info/files[]/zipped data`. ~99% of
//! H3 has it; Reach mostly migrated to .gr2 and the field's empty
//! there. Without a source we still emit the per-tag reconstruction
//! status.

use std::error::Error;
use std::io::Read;
use std::path::{Path, PathBuf};

use blam_tags::{JmsFile, TagFile};
use flate2::read::ZlibDecoder;

fn main() -> Result<(), Box<dyn Error>> {
    let dirs: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if dirs.is_empty() {
        return Err("usage: jms_corpus_sweep <DIR> [<DIR>...]".into());
    }

    let mut paths = Vec::new();
    for d in &dirs { collect_jms_eligible_tags(d, &mut paths); }
    paths.sort();
    eprintln!("scanning {} render/collision/physics tags", paths.len());

    let mut stats = SweepStats::default();
    for p in &paths {
        process(p, &mut stats);
    }
    stats.report();
    Ok(())
}

fn process(path: &Path, stats: &mut SweepStats) {
    stats.total += 1;
    let tag = match TagFile::read(path) {
        Ok(t) => t,
        Err(e) => {
            stats.tag_read_failed += 1;
            stats.failure_examples.push(format!("read failed: {} — {}", path.display(), e));
            return;
        }
    };
    let group = tag.header.group_tag.to_be_bytes();
    let (kind, jms_result) = match &group {
        b"mode" => ("mode", JmsFile::from_render_model(&tag)),
        b"coll" => {
            // Auto-discover sibling render_model so we can place
            // BSP vertices in world space (same convention the
            // embedded source JMS uses).
            let sibling = path.with_extension("render_model");
            let result = if sibling.exists() {
                if let Ok(rt) = TagFile::read(&sibling) {
                    if let Ok(skel) = JmsFile::from_render_model(&rt) {
                        JmsFile::from_collision_model_with_skeleton(&tag, &skel.nodes)
                    } else { JmsFile::from_collision_model(&tag) }
                } else { JmsFile::from_collision_model(&tag) }
            } else { JmsFile::from_collision_model(&tag) };
            ("coll", result)
        }
        b"phmo" => ("phmo", JmsFile::from_physics_model(&tag)),
        _ => return,
    };
    let k = stats.per_kind.entry(kind).or_default();
    k.total += 1;
    let jms = match jms_result {
        Ok(j) => j,
        Err(e) => {
            stats.reconstruct_failed += 1;
            stats.failure_examples.push(format!("reconstruct failed ({kind}): {} — {}", path.display(), e));
            return;
        }
    };
    stats.reconstruct_ok += 1;
    k.reconstruct_ok += 1;
    stats.vertex_total += jms.vertices.len() as u64;
    stats.triangle_total += jms.triangles.len() as u64;

    // Try to extract an embedded source JMS for comparison.
    let Some(source_bytes) = extract_embedded_jms_bytes(&tag) else {
        stats.no_embedded_source += 1;
        k.no_embedded_source += 1;
        return;
    };
    let parsed = match parse_jms_summary(&source_bytes) {
        Ok(s) => s,
        Err(e) => {
            stats.embedded_parse_failed += 1;
            k.embedded_parse_failed += 1;
            stats.failure_examples.push(format!("embedded JMS parse: {} — {}", path.display(), e));
            return;
        }
    };
    stats.compared += 1;
    k.compared += 1;

    // Agreement checks. JMS materials/markers can be authored
    // differently than the tag stores them (the `(N)` slot index,
    // marker variants beyond what the tag holds, etc), so we compare
    // counts as a sanity bound rather than bit-exact identity.
    let nodes_match = jms.nodes.len() == parsed.nodes;
    let materials_match = jms.materials.len() == parsed.materials;
    let markers_match = jms.markers.len() == parsed.markers;
    if nodes_match { stats.nodes_match += 1; k.nodes_match += 1; }
    if materials_match { stats.materials_match += 1; k.materials_match += 1; }
    if markers_match { stats.markers_match += 1; k.markers_match += 1; }

    // Vertex counts can legitimately diverge: transparent parts get
    // back-faces baked into per_mesh_temporary by MCC import, doubling
    // the count vs the artist source. We accept count >= source as
    // OK; source > rebuilt is a problem.
    let verts_ok = jms.vertices.len() >= parsed.vertices;
    if verts_ok { stats.verts_at_least_source += 1; k.verts_at_least_source += 1; }

    // Position-set overlap. We compare at two precision tiers
    // because bounds-decompression quantization scales with the
    // bbox: small props (cm-scale) match at 0.1cm but sky models
    // (km-scale) lose ~0.5-1cm to f32 round-trip and only match at
    // 10cm. Coverage at the looser tier proves "geometry equivalent
    // up to quantization"; the tighter tier proves "byte-equivalent
    // dequantization".
    let parsed_strict = parsed.positions_rounded(1);   // 0.1cm
    let parsed_loose  = parsed.positions_rounded(-1);  // 10cm
    let rebuilt_strict: std::collections::HashSet<_> =
        jms.vertices.iter().map(|v| round_pos(v.position, 1)).collect();
    let rebuilt_loose: std::collections::HashSet<_> =
        jms.vertices.iter().map(|v| round_pos(v.position, -1)).collect();
    let cov_strict = if parsed_strict.is_empty() { 1.0 }
        else { parsed_strict.intersection(&rebuilt_strict).count() as f64 / parsed_strict.len() as f64 };
    let cov_loose = if parsed_loose.is_empty() { 1.0 }
        else { parsed_loose.intersection(&rebuilt_loose).count() as f64 / parsed_loose.len() as f64 };
    if cov_strict >= 0.99 { stats.positions_strict_99 += 1; k.positions_strict_99 += 1; }
    if cov_loose  >= 0.99 { stats.positions_loose_99 += 1; k.positions_loose_99 += 1; }
    if cov_loose  >= 0.999 { stats.positions_loose_999 += 1; k.positions_loose_999 += 1; }

    // Bbox match (within 0.5cm slack).
    let bbox_match = parsed.bbox.map_or(true, |sb| {
        let rb = rebuilt_bbox(&jms);
        bbox_close(sb, rb, 0.5)
    });
    if bbox_match { stats.bbox_match += 1; k.bbox_match += 1; }

    // Phmo per-shape comparisons. Pair items by name (source and
    // rebuild can use different node orderings, so name-keyed
    // comparison is robust against that). Translation match is
    // within 0.5cm, size match is within 0.5cm.
    if kind == "phmo" {
        let s = &mut k.shape;
        compare_phmo_shapes(s, &jms, &parsed);
    }

    if !nodes_match || cov_loose < 0.99 || !verts_ok || !bbox_match {
        stats.failure_examples.push(format!(
            "{}: nodes {}/{} mats {}/{} markers {}/{} verts {}/{} cov_strict {:.1}% cov_loose {:.1}% bbox_match={}",
            path.display(),
            jms.nodes.len(), parsed.nodes,
            jms.materials.len(), parsed.materials,
            jms.markers.len(), parsed.markers,
            jms.vertices.len(), parsed.vertices,
            cov_strict * 100.0, cov_loose * 100.0, bbox_match,
        ));
    }
}

// ---- corpus walk ----

fn collect_jms_eligible_tags(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return; };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() { collect_jms_eligible_tags(&p, out); }
        else if matches!(p.extension().and_then(|s| s.to_str()),
            Some("render_model") | Some("collision_model") | Some("physics_model"))
        {
            out.push(p);
        }
    }
}

// ---- embedded source extraction + parse ----

fn extract_embedded_jms_bytes(tag: &TagFile) -> Option<Vec<u8>> {
    let info = tag.import_info()?;
    let files = info.field_path("files").and_then(|f| f.as_block())?;
    for i in 0..files.len() {
        let elem = files.element(i)?;
        let zipped = elem.field("zipped data").and_then(|f| f.as_data()).unwrap_or(&[]);
        if zipped.is_empty() { continue; }
        let mut decoded = Vec::with_capacity(zipped.len() * 4);
        if ZlibDecoder::new(zipped).read_to_end(&mut decoded).is_err() { continue; }
        // Heuristic: a JMS source starts with ";### VERSION ###".
        // Skip non-JMS embedded payloads (.gr2, etc).
        if decoded.starts_with(b";### VERSION ###") {
            return Some(decoded);
        }
    }
    None
}

#[derive(Debug, Default)]
struct ParsedJms {
    nodes: usize,
    materials: usize,
    markers: usize,
    vertices: usize,
    triangles: usize,
    positions: Vec<[f32; 3]>,
    bbox: Option<([f32; 3], [f32; 3])>,
    // Collision-shape sections (populated for phmo / combined exports).
    capsules: Vec<JmsCapsuleSummary>,
    convex_shapes: Vec<JmsConvexSummary>,
    ragdolls: Vec<JmsRagdollSummary>,
    hinges: Vec<JmsHingeSummary>,
    boxes: Vec<JmsBoxSummary>,
    spheres: Vec<JmsSphereSummary>,
    node_names: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct JmsCapsuleSummary {
    name: String,
    parent_name: String,
    translation: [f32; 3],
    height: f32,
    radius: f32,
}

#[derive(Debug, Clone, Default)]
struct JmsConvexSummary {
    name: String,
    parent_name: String,
    vertex_count: usize,
}

#[derive(Debug, Clone, Default)]
struct JmsRagdollSummary {
    name: String,
    attached_translation: [f32; 3],
    referenced_translation: [f32; 3],
}

#[derive(Debug, Clone, Default)]
struct JmsHingeSummary {
    name: String,
}

#[derive(Debug, Clone, Default)]
struct JmsBoxSummary {
    name: String,
    parent_name: String,
    translation: [f32; 3],
    width: f32, length: f32, height: f32,
}

#[derive(Debug, Clone, Default)]
struct JmsSphereSummary {
    name: String,
    parent_name: String,
    radius: f32,
}

impl ParsedJms {
    fn positions_rounded(&self, decimals: i32) -> std::collections::HashSet<(i32, i32, i32)> {
        self.positions.iter().map(|p| round_pos(*p, decimals)).collect()
    }
}

/// Minimal JMS parser that walks just the section headers + counts
/// + the VERTICES section's positions. Doesn't reconstruct the full
/// scene — only what the sweep needs to compute agreement metrics.
fn parse_jms_summary(bytes: &[u8]) -> Result<ParsedJms, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| format!("utf8: {e}"))?;
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut out = ParsedJms::default();

    // Section headers are normally `;### LABEL ###`, but the Saber
    // 3D exports embedded under `s3d_*` levels emit a double-`;`
    // prefix (`;;### LABEL ###`). Tolerate any leading-`;` count.
    let section_count = |label: &str| -> Option<usize> {
        let needle = format!("### {label} ###");
        let idx = lines.iter().position(|l|
            l.trim_start_matches(';') == needle
        )?;
        let count_line = lines.get(idx + 1)?;
        count_line.parse::<usize>().ok()
    };

    out.nodes = section_count("NODES").ok_or("no NODES section")?;
    out.materials = section_count("MATERIALS").ok_or("no MATERIALS section")?;
    out.markers = section_count("MARKERS").ok_or("no MARKERS section")?;
    out.vertices = section_count("VERTICES").ok_or("no VERTICES section")?;
    out.triangles = section_count("TRIANGLES").ok_or("no TRIANGLES section")?;

    // Walk the VERTICES section to pull positions. Format per
    // record (8213): `;VERTEX i\nposition\nnormal\nnode_count\n
    // <node_count>×{idx, weight}\nuv_count\n<uv_count>×uv\n
    // (blank)\nradius`. We only need position.
    let v_idx = lines.iter().position(|l|
        l.trim_start_matches(';') == "### VERTICES ###"
    ).ok_or("no vertices")?;
    let mut cur = v_idx + 2;
    while cur < lines.len() && !lines[cur].trim_start_matches(';').starts_with("VERTEX 0") { cur += 1; }
    let mut positions = Vec::with_capacity(out.vertices);
    for _ in 0..out.vertices {
        if cur >= lines.len() { break; }
        cur += 1; // ;VERTEX i
        if cur >= lines.len() { break; }
        positions.push(parse_float_triple(lines[cur])?);
        cur += 2; // position + normal
        if cur >= lines.len() { break; }
        let node_count: usize = lines[cur].parse().map_err(|_| "node count")?;
        cur += 1 + node_count * 2;
        if cur >= lines.len() { break; }
        let uv_count: usize = lines[cur].parse().map_err(|_| "uv count")?;
        cur += 1 + uv_count;
        cur += 1; // vertex color
        while cur < lines.len() && lines[cur].is_empty() { cur += 1; }
    }
    out.bbox = bbox_of(&positions);
    out.positions = positions;

    // Pull node names so we can resolve parent_index → bone name
    // for collision-shape comparisons (source and rebuilt may use
    // different node orderings; name-keyed comparison is robust).
    out.node_names = parse_node_names(&lines, out.nodes).unwrap_or_default();

    // Collision-shape sections. Each is optional; absent sections
    // produce empty vecs.
    out.capsules = parse_capsules_section(&lines, &out.node_names).unwrap_or_default();
    out.convex_shapes = parse_convex_shapes_section(&lines, &out.node_names).unwrap_or_default();
    out.ragdolls = parse_ragdolls_section(&lines).unwrap_or_default();
    out.hinges = parse_hinges_section(&lines).unwrap_or_default();
    out.boxes = parse_boxes_section(&lines, &out.node_names).unwrap_or_default();
    out.spheres = parse_spheres_section(&lines, &out.node_names).unwrap_or_default();
    Ok(out)
}

fn find_section_start(lines: &[&str], label: &str) -> Option<(usize, usize)> {
    let needle = format!("### {label} ###");
    let idx = lines.iter().position(|l| l.trim_start_matches(';') == needle)?;
    let count: usize = lines.get(idx + 1)?.parse().ok()?;
    Some((idx + 2, count))
}

fn skip_to_first_record(lines: &[&str], start: usize, label: &str) -> usize {
    let needle_with_zero = format!("{label} 0");
    let needle_with_zero_no_space = format!("{}0", label);
    let mut cur = start;
    while cur < lines.len() {
        let l = lines[cur].trim_start_matches(';');
        if l == needle_with_zero || l == needle_with_zero_no_space { break; }
        cur += 1;
    }
    cur
}

fn skip_blanks(lines: &[&str], mut cur: usize) -> usize {
    while cur < lines.len() && lines[cur].is_empty() { cur += 1; }
    cur
}

fn lookup_parent_name(idx: i32, names: &[String]) -> String {
    if idx >= 0 && (idx as usize) < names.len() {
        names[idx as usize].clone()
    } else {
        String::new()
    }
}

fn parse_node_names(lines: &[&str], n: usize) -> Result<Vec<String>, String> {
    let Some((start, _count)) = find_section_start(lines, "NODES") else { return Ok(Vec::new()); };
    let mut cur = skip_to_first_record(lines, start, "NODE");
    let mut names = Vec::with_capacity(n);
    for _ in 0..n {
        if cur >= lines.len() { break; }
        cur += 1; // ;NODE i
        if cur >= lines.len() { break; }
        names.push(lines[cur].to_string()); cur += 1;
        cur += 1; // parent
        cur += 1; // rotation
        cur += 1; // translation
        cur = skip_blanks(lines, cur);
    }
    Ok(names)
}

fn parse_capsules_section(lines: &[&str], node_names: &[String]) -> Result<Vec<JmsCapsuleSummary>, String> {
    let Some((start, count)) = find_section_start(lines, "CAPSULES") else { return Ok(Vec::new()); };
    let mut cur = skip_to_first_record(lines, start, "CAPSULE");
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if cur >= lines.len() { break; }
        cur += 1; // ;CAPSULE i
        let name = lines.get(cur).copied().unwrap_or("").to_string(); cur += 1;
        let parent: i32 = lines.get(cur).and_then(|s| s.parse().ok()).unwrap_or(-1); cur += 1;
        let _material: i32 = lines.get(cur).and_then(|s| s.parse().ok()).unwrap_or(-1); cur += 1;
        cur += 1; // rotation
        let translation = parse_float_triple(lines.get(cur).copied().unwrap_or(""))?; cur += 1;
        let height: f32 = lines.get(cur).and_then(|s| s.parse().ok()).unwrap_or(0.0); cur += 1;
        let radius: f32 = lines.get(cur).and_then(|s| s.parse().ok()).unwrap_or(0.0); cur += 1;
        cur = skip_blanks(lines, cur);
        out.push(JmsCapsuleSummary {
            name,
            parent_name: lookup_parent_name(parent, node_names),
            translation, height, radius,
        });
    }
    Ok(out)
}

fn parse_convex_shapes_section(lines: &[&str], node_names: &[String]) -> Result<Vec<JmsConvexSummary>, String> {
    let Some((start, count)) = find_section_start(lines, "CONVEX SHAPES") else { return Ok(Vec::new()); };
    let mut cur = skip_to_first_record(lines, start, "CONVEX SHAPE");
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if cur >= lines.len() { break; }
        cur += 1; // ;CONVEX SHAPE i
        let name = lines.get(cur).copied().unwrap_or("").to_string(); cur += 1;
        let parent: i32 = lines.get(cur).and_then(|s| s.parse().ok()).unwrap_or(-1); cur += 1;
        let _material: i32 = lines.get(cur).and_then(|s| s.parse().ok()).unwrap_or(-1); cur += 1;
        cur += 1; // rotation
        cur += 1; // translation
        let vertex_count: usize = lines.get(cur).and_then(|s| s.parse().ok()).unwrap_or(0); cur += 1;
        cur += vertex_count; // vertex lines
        cur = skip_blanks(lines, cur);
        out.push(JmsConvexSummary {
            name,
            parent_name: lookup_parent_name(parent, node_names),
            vertex_count,
        });
    }
    Ok(out)
}

fn parse_ragdolls_section(lines: &[&str]) -> Result<Vec<JmsRagdollSummary>, String> {
    let Some((start, count)) = find_section_start(lines, "RAGDOLLS") else { return Ok(Vec::new()); };
    let mut cur = skip_to_first_record(lines, start, "RAGDOLL");
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if cur >= lines.len() { break; }
        cur += 1; // ;RAGDOLL i
        let name = lines.get(cur).copied().unwrap_or("").to_string(); cur += 1;
        cur += 2; // attached/referenced index
        cur += 1; // attached rotation
        let a_trans = parse_float_triple(lines.get(cur).copied().unwrap_or(""))?; cur += 1;
        cur += 1; // referenced rotation
        let r_trans = parse_float_triple(lines.get(cur).copied().unwrap_or(""))?; cur += 1;
        cur += 7; // 6 limit floats + friction_limit
        cur = skip_blanks(lines, cur);
        out.push(JmsRagdollSummary {
            name,
            attached_translation: a_trans,
            referenced_translation: r_trans,
        });
    }
    Ok(out)
}

fn parse_hinges_section(lines: &[&str]) -> Result<Vec<JmsHingeSummary>, String> {
    let Some((start, count)) = find_section_start(lines, "HINGES") else { return Ok(Vec::new()); };
    let mut cur = skip_to_first_record(lines, start, "HINGE");
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if cur >= lines.len() { break; }
        cur += 1; // ;HINGE i
        let name = lines.get(cur).copied().unwrap_or("").to_string(); cur += 1;
        cur += 2; // body_a/b indices
        cur += 4; // body_a rot+trans, body_b rot+trans
        cur += 4; // is_limited, friction_limit, min_angle, max_angle
        cur = skip_blanks(lines, cur);
        out.push(JmsHingeSummary { name });
    }
    Ok(out)
}

fn parse_boxes_section(lines: &[&str], node_names: &[String]) -> Result<Vec<JmsBoxSummary>, String> {
    let Some((start, count)) = find_section_start(lines, "BOXES") else { return Ok(Vec::new()); };
    let mut cur = skip_to_first_record(lines, start, "BOX");
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if cur >= lines.len() { break; }
        cur += 1; // ;BOX i
        let name = lines.get(cur).copied().unwrap_or("").to_string(); cur += 1;
        let parent: i32 = lines.get(cur).and_then(|s| s.parse().ok()).unwrap_or(-1); cur += 1;
        let _material: i32 = lines.get(cur).and_then(|s| s.parse().ok()).unwrap_or(-1); cur += 1;
        cur += 1; // rotation
        let translation = parse_float_triple(lines.get(cur).copied().unwrap_or(""))?; cur += 1;
        let width: f32 = lines.get(cur).and_then(|s| s.parse().ok()).unwrap_or(0.0); cur += 1;
        let length: f32 = lines.get(cur).and_then(|s| s.parse().ok()).unwrap_or(0.0); cur += 1;
        let height: f32 = lines.get(cur).and_then(|s| s.parse().ok()).unwrap_or(0.0); cur += 1;
        cur = skip_blanks(lines, cur);
        out.push(JmsBoxSummary {
            name, parent_name: lookup_parent_name(parent, node_names),
            translation, width, length, height,
        });
    }
    Ok(out)
}

fn parse_spheres_section(lines: &[&str], node_names: &[String]) -> Result<Vec<JmsSphereSummary>, String> {
    let Some((start, count)) = find_section_start(lines, "SPHERES") else { return Ok(Vec::new()); };
    let mut cur = skip_to_first_record(lines, start, "SPHERE");
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if cur >= lines.len() { break; }
        cur += 1; // ;SPHERE i
        let name = lines.get(cur).copied().unwrap_or("").to_string(); cur += 1;
        let parent: i32 = lines.get(cur).and_then(|s| s.parse().ok()).unwrap_or(-1); cur += 1;
        let _material: i32 = lines.get(cur).and_then(|s| s.parse().ok()).unwrap_or(-1); cur += 1;
        cur += 1; // rotation
        cur += 1; // translation
        let radius: f32 = lines.get(cur).and_then(|s| s.parse().ok()).unwrap_or(0.0); cur += 1;
        cur = skip_blanks(lines, cur);
        out.push(JmsSphereSummary {
            name, parent_name: lookup_parent_name(parent, node_names), radius,
        });
    }
    Ok(out)
}

fn parse_float_triple(line: &str) -> Result<[f32; 3], String> {
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() < 3 { return Err(format!("bad triple: {line:?}")); }
    let parse = |s: &str| -> Result<f32, String> {
        s.parse::<f32>().map_err(|e| format!("parse float {s:?}: {e}"))
    };
    Ok([parse(parts[0])?, parse(parts[1])?, parse(parts[2])?])
}

// ---- bbox math ----

fn bbox_of(ps: &[[f32; 3]]) -> Option<([f32; 3], [f32; 3])> {
    let mut iter = ps.iter();
    let first = iter.next()?;
    let mut min = *first;
    let mut max = *first;
    for p in iter {
        for i in 0..3 {
            if p[i] < min[i] { min[i] = p[i]; }
            if p[i] > max[i] { max[i] = p[i]; }
        }
    }
    Some((min, max))
}

fn rebuilt_bbox(jms: &JmsFile) -> Option<([f32; 3], [f32; 3])> {
    let positions: Vec<[f32; 3]> = jms.vertices.iter().map(|v| v.position).collect();
    bbox_of(&positions)
}

fn bbox_close(a: ([f32; 3], [f32; 3]), b: Option<([f32; 3], [f32; 3])>, slack: f32) -> bool {
    let Some(b) = b else { return false; };
    for i in 0..3 {
        if (a.0[i] - b.0[i]).abs() > slack { return false; }
        if (a.1[i] - b.1[i]).abs() > slack { return false; }
    }
    true
}

fn round_pos(p: [f32; 3], decimals: i32) -> (i32, i32, i32) {
    let scale = 10f32.powi(decimals);
    (
        (p[0] * scale).round() as i32,
        (p[1] * scale).round() as i32,
        (p[2] * scale).round() as i32,
    )
}

// ---- aggregate stats ----

#[derive(Default)]
struct KindStats {
    total: usize,
    reconstruct_ok: usize,
    no_embedded_source: usize,
    embedded_parse_failed: usize,
    compared: usize,
    nodes_match: usize,
    materials_match: usize,
    markers_match: usize,
    verts_at_least_source: usize,
    bbox_match: usize,
    positions_strict_99: usize,
    positions_loose_99: usize,
    positions_loose_999: usize,
    // Phmo per-shape counters (only populated for kind="phmo").
    shape: PhmoShapeStats,
}

#[derive(Default)]
struct PhmoShapeStats {
    capsules_count_match: usize, // tags with count match
    capsules_with_data: usize,   // tags where source has ≥1 capsule
    capsules_total: usize,       // total source capsules across all tags
    capsules_name_paired: usize, // capsules paired by name with rebuild
    capsules_translation_match: usize,  // ≤0.5cm
    capsules_size_match: usize,         // height & radius ≤0.5cm

    convex_count_match: usize,
    convex_with_data: usize,
    convex_total: usize,
    convex_name_paired: usize,

    ragdolls_count_match: usize,
    ragdolls_with_data: usize,
    ragdolls_total: usize,
    ragdolls_name_paired: usize,
    ragdolls_translation_match: usize,

    hinges_count_match: usize,
    hinges_with_data: usize,
    hinges_total: usize,
    hinges_name_paired: usize,

    boxes_count_match: usize,
    boxes_with_data: usize,
    boxes_total: usize,
    boxes_name_paired: usize,
    boxes_size_match: usize,

    spheres_count_match: usize,
    spheres_with_data: usize,
    spheres_total: usize,
    spheres_name_paired: usize,
}

#[derive(Default)]
struct SweepStats {
    total: usize,
    tag_read_failed: usize,
    reconstruct_failed: usize,
    reconstruct_ok: usize,
    no_embedded_source: usize,
    embedded_parse_failed: usize,
    compared: usize,
    nodes_match: usize,
    materials_match: usize,
    markers_match: usize,
    verts_at_least_source: usize,
    bbox_match: usize,
    positions_strict_99: usize,
    positions_loose_99: usize,
    positions_loose_999: usize,
    vertex_total: u64,
    triangle_total: u64,
    per_kind: std::collections::BTreeMap<&'static str, KindStats>,
    failure_examples: Vec<String>,
}

impl SweepStats {
    fn report(&self) {
        eprintln!();
        eprintln!("=== reconstruction (overall) ===");
        eprintln!("  total tags:         {}", self.total);
        eprintln!("  tag read failed:    {}", self.tag_read_failed);
        eprintln!("  reconstruct ok:     {}", self.reconstruct_ok);
        eprintln!("  reconstruct failed: {}", self.reconstruct_failed);
        eprintln!("  total vertices:     {}", self.vertex_total);
        eprintln!("  total triangles:    {}", self.triangle_total);

        for (kind, k) in &self.per_kind {
            eprintln!();
            eprintln!("=== {kind} ===");
            eprintln!("  total tags:           {}", k.total);
            eprintln!("  reconstruct ok:       {} ({:.1}%)", k.reconstruct_ok, pct1(k.reconstruct_ok, k.total));
            eprintln!("  no embedded source:   {}", k.no_embedded_source);
            eprintln!("  embedded parse fail:  {}", k.embedded_parse_failed);
            eprintln!("  compared:             {}", k.compared);
            if k.compared > 0 {
                let p = |n: usize| pct1(n, k.compared);
                eprintln!("    nodes count match:           {} ({:.1}%)", k.nodes_match,           p(k.nodes_match));
                eprintln!("    materials count match:       {} ({:.1}%)", k.materials_match,       p(k.materials_match));
                eprintln!("    markers count match:         {} ({:.1}%)", k.markers_match,         p(k.markers_match));
                eprintln!("    verts >= source:             {} ({:.1}%)", k.verts_at_least_source, p(k.verts_at_least_source));
                eprintln!("    bbox match (≤0.5cm slack):   {} ({:.1}%)", k.bbox_match,            p(k.bbox_match));
                eprintln!("    pos cov @0.1cm  ≥ 99%:       {} ({:.1}%)", k.positions_strict_99,   p(k.positions_strict_99));
                eprintln!("    pos cov @10cm   ≥ 99%:       {} ({:.1}%)", k.positions_loose_99,    p(k.positions_loose_99));
                eprintln!("    pos cov @10cm   ≥ 99.9%:     {} ({:.1}%)", k.positions_loose_999,   p(k.positions_loose_999));
            }
            if *kind == "phmo" {
                let s = &k.shape;
                eprintln!("  --- phmo shape detail (per-tag count match | per-shape name pair / value match) ---");
                eprintln!("    capsules: {} tags w/ data; count match {}/{} ({:.1}%);  paired {}/{}, trans match {}/{}, size match {}/{}",
                    s.capsules_with_data, s.capsules_count_match, s.capsules_with_data, pct1(s.capsules_count_match, s.capsules_with_data),
                    s.capsules_name_paired, s.capsules_total,
                    s.capsules_translation_match, s.capsules_name_paired,
                    s.capsules_size_match, s.capsules_name_paired);
                eprintln!("    convex:   {} tags w/ data; count match {}/{} ({:.1}%);  paired {}/{}",
                    s.convex_with_data, s.convex_count_match, s.convex_with_data, pct1(s.convex_count_match, s.convex_with_data),
                    s.convex_name_paired, s.convex_total);
                eprintln!("    ragdolls: {} tags w/ data; count match {}/{} ({:.1}%);  paired {}/{}, trans match {}/{}",
                    s.ragdolls_with_data, s.ragdolls_count_match, s.ragdolls_with_data, pct1(s.ragdolls_count_match, s.ragdolls_with_data),
                    s.ragdolls_name_paired, s.ragdolls_total,
                    s.ragdolls_translation_match, s.ragdolls_name_paired);
                eprintln!("    hinges:   {} tags w/ data; count match {}/{} ({:.1}%);  paired {}/{}",
                    s.hinges_with_data, s.hinges_count_match, s.hinges_with_data, pct1(s.hinges_count_match, s.hinges_with_data),
                    s.hinges_name_paired, s.hinges_total);
                eprintln!("    boxes:    {} tags w/ data; count match {}/{} ({:.1}%);  paired {}/{}, size match {}/{}",
                    s.boxes_with_data, s.boxes_count_match, s.boxes_with_data, pct1(s.boxes_count_match, s.boxes_with_data),
                    s.boxes_name_paired, s.boxes_total,
                    s.boxes_size_match, s.boxes_name_paired);
                eprintln!("    spheres:  {} tags w/ data; count match {}/{} ({:.1}%);  paired {}/{}",
                    s.spheres_with_data, s.spheres_count_match, s.spheres_with_data, pct1(s.spheres_count_match, s.spheres_with_data),
                    s.spheres_name_paired, s.spheres_total);
            }
        }

        if !self.failure_examples.is_empty() {
            eprintln!();
            eprintln!("=== first 20 issues (across all kinds) ===");
            for ex in self.failure_examples.iter().take(20) {
                eprintln!("  {ex}");
            }
            if self.failure_examples.len() > 20 {
                eprintln!("  ... and {} more", self.failure_examples.len() - 20);
            }
        }
    }
}

fn pct1(n: usize, denom: usize) -> f64 {
    if denom == 0 { 0.0 } else { 100.0 * n as f64 / denom as f64 }
}

// ---- phmo per-shape comparison ----

fn compare_phmo_shapes(s: &mut PhmoShapeStats, jms: &JmsFile, parsed: &ParsedJms) {
    // Capsules. Count match only counted for tags that ACTUALLY
    // have capsule data in the source (otherwise 0=0 swamps stats).
    if !parsed.capsules.is_empty() {
        s.capsules_with_data += 1;
        if jms.capsules.len() == parsed.capsules.len() { s.capsules_count_match += 1; }
    }
    s.capsules_total += parsed.capsules.len();
    let reb_caps_by_name: std::collections::HashMap<&str, &blam_tags::JmsCapsule> =
        jms.capsules.iter().map(|c| (c.name.as_str(), c)).collect();
    for src in &parsed.capsules {
        let Some(reb) = reb_caps_by_name.get(src.name.as_str()) else { continue; };
        s.capsules_name_paired += 1;
        let trans_diff = vec3_dist(src.translation, reb.translation);
        if trans_diff <= 0.5 { s.capsules_translation_match += 1; }
        let h_diff = (src.height - reb.height).abs();
        let r_diff = (src.radius - reb.radius).abs();
        if h_diff <= 0.5 && r_diff <= 0.5 { s.capsules_size_match += 1; }
    }

    if !parsed.convex_shapes.is_empty() {
        s.convex_with_data += 1;
        if jms.convex_shapes.len() == parsed.convex_shapes.len() { s.convex_count_match += 1; }
    }
    s.convex_total += parsed.convex_shapes.len();
    let reb_convex_by_name: std::collections::HashSet<&str> =
        jms.convex_shapes.iter().map(|c| c.name.as_str()).collect();
    for src in &parsed.convex_shapes {
        if reb_convex_by_name.contains(src.name.as_str()) { s.convex_name_paired += 1; }
    }

    if !parsed.ragdolls.is_empty() {
        s.ragdolls_with_data += 1;
        if jms.ragdolls.len() == parsed.ragdolls.len() { s.ragdolls_count_match += 1; }
    }
    s.ragdolls_total += parsed.ragdolls.len();
    let reb_rag_by_name: std::collections::HashMap<&str, &blam_tags::JmsRagdoll> =
        jms.ragdolls.iter().map(|r| (r.name.as_str(), r)).collect();
    for src in &parsed.ragdolls {
        let Some(reb) = reb_rag_by_name.get(src.name.as_str()) else { continue; };
        s.ragdolls_name_paired += 1;
        let a_diff = vec3_dist(src.attached_translation, reb.attached_translation);
        let r_diff = vec3_dist(src.referenced_translation, reb.referenced_translation);
        if a_diff <= 0.5 && r_diff <= 0.5 { s.ragdolls_translation_match += 1; }
    }

    if !parsed.hinges.is_empty() {
        s.hinges_with_data += 1;
        if jms.hinges.len() == parsed.hinges.len() { s.hinges_count_match += 1; }
    }
    s.hinges_total += parsed.hinges.len();
    let reb_hinge_by_name: std::collections::HashSet<&str> =
        jms.hinges.iter().map(|h| h.name.as_str()).collect();
    for src in &parsed.hinges {
        if reb_hinge_by_name.contains(src.name.as_str()) { s.hinges_name_paired += 1; }
    }

    if !parsed.boxes.is_empty() {
        s.boxes_with_data += 1;
        if jms.boxes.len() == parsed.boxes.len() { s.boxes_count_match += 1; }
    }
    s.boxes_total += parsed.boxes.len();
    let reb_box_by_name: std::collections::HashMap<&str, &blam_tags::JmsBox> =
        jms.boxes.iter().map(|b| (b.name.as_str(), b)).collect();
    for src in &parsed.boxes {
        let Some(reb) = reb_box_by_name.get(src.name.as_str()) else { continue; };
        s.boxes_name_paired += 1;
        let dw = (src.width - reb.width).abs();
        let dl = (src.length - reb.length).abs();
        let dh = (src.height - reb.height).abs();
        if dw <= 0.5 && dl <= 0.5 && dh <= 0.5 { s.boxes_size_match += 1; }
    }

    if !parsed.spheres.is_empty() {
        s.spheres_with_data += 1;
        if jms.spheres.len() == parsed.spheres.len() { s.spheres_count_match += 1; }
    }
    s.spheres_total += parsed.spheres.len();
    let reb_sphere_by_name: std::collections::HashSet<&str> =
        jms.spheres.iter().map(|s| s.name.as_str()).collect();
    for src in &parsed.spheres {
        if reb_sphere_by_name.contains(src.name.as_str()) { s.spheres_name_paired += 1; }
    }
}

fn vec3_dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = a[0] - b[0]; let dy = a[1] - b[1]; let dz = a[2] - b[2];
    (dx*dx + dy*dy + dz*dz).sqrt()
}
