//! Compare our jmad animation extraction against a TagTool-extracted
//! reference tree, convention-aware (TagTool vs Foundry differences are
//! expected: base/JMR +1 frame, JMO scale, movement folding into root).
//!
//! Metric: per-anim, frame-aligned, the max NON-ROOT (node>=1) rotation
//! diff (quaternion sign-agnostic) and translation diff. Rotation is the
//! explosion-relevant signal. Node 0 is excluded (movement folding into
//! the root legitimately differs from TagTool).
//!
//! Usage: anim_corpus_compare <defs-dir> <our-tags-root> <reference-game-dir>

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use blam_tags::animation::{Animation, AnimationGraph, JmaKind, NodeTransform, Skeleton};
use blam_tags::classic::read_classic_tag_file;
use blam_tags::layout::TagLayout;
use blam_tags::TagFile;

const ROT_TOL: f32 = 0.01; // ~0.5 degrees on a quaternion component
const TRN_TOL: f32 = 0.5; // 0.5 cm

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let defs = PathBuf::from(&a[1]);
    let tags_root = PathBuf::from(&a[2]);
    let ref_dir = PathBuf::from(&a[3]);
    let layout_path = defs.join("model_animation_graph.json");

    // Group reference anim files by their parent dir (= tag rel path under data/).
    let data_root = ref_dir.join("data");
    let all = walk(&data_root);
    let mut by_tag: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for f in all {
        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext.starts_with("jm") {
            if let Some(parent) = f.parent() {
                by_tag.entry(parent.to_path_buf()).or_default().push(f);
            }
        }
    }

    // [exact(<0.01), close(<0.1), total] for rotation, sign-agnostic.
    let mut per_type: BTreeMap<String, [usize; 3]> = BTreeMap::new();
    let mut tags_ok = 0usize;
    let mut tags_missing = 0usize;
    let mut worst: Vec<(f32, String, String)> = Vec::new();

    for (refdir, files) in &by_tag {
        let rel = refdir.strip_prefix(&data_root).unwrap();
        let tag_path = tags_root.join(rel).with_extension("model_animation_graph");
        if !tag_path.is_file() {
            tags_missing += 1;
            continue;
        }
        // MCC tags (H3/Reach/H4) carry their own layout — TagFile::read
        // resolves it; classic (CE/H2) need the JSON layout + classic reader.
        let tag = match TagFile::read(&tag_path) {
            Ok(t) => t,
            Err(_) => {
                let Ok(bytes) = std::fs::read(&tag_path) else { continue };
                let Ok(layout) = TagLayout::from_json(&layout_path) else { continue };
                match read_classic_tag_file(&bytes, layout) { Ok(t) => t, Err(_) => continue }
            }
        };
        tags_ok += 1;

        // Build our composed frames per anim, keyed by sanitized name.
        let ours = compose_all(&tag);

        for reffile in files {
            let stem = reffile.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let ext = reffile.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_uppercase();
            let key = stem.to_ascii_lowercase();
            let Some((nodes, our_frames)) = ours.get(&key) else { continue };
            let Some(ref_frames) = parse_jma(reffile, *nodes) else { continue };
            let (rd, _td) = compare(&ref_frames, our_frames, *nodes);
            let e = per_type.entry(ext.clone()).or_default();
            e[2] += 1;
            if rd < ROT_TOL { e[0] += 1; }
            if rd < 0.1 { e[1] += 1; }
            if rd >= 0.1 {
                worst.push((rd, ext.clone(), format!("{}/{}", rel.display(), stem)));
            }
        }
    }

    println!("=== {} ===", ref_dir.file_name().and_then(|s| s.to_str()).unwrap_or("?"));
    println!("tags: {tags_ok} matched, {tags_missing} reference tags with no corpus tag");
    println!("  type:  exact(<0.01) / close(<0.1) / total   [non-root rotation]");
    let (mut tr, mut tc, mut tn) = (0, 0, 0);
    for (ext, [r, c, n]) in &per_type {
        let pct = 100.0 * *c as f64 / *n as f64;
        println!("  .{ext:<4} {r:>6} / {c:>6} / {n}   ({pct:.1}% close)", r = r, c = c, n = n);
        tr += r; tc += c; tn += n;
    }
    let pct = 100.0 * tc as f64 / tn.max(1) as f64;
    println!("  TOTAL {tr:>6} / {tc:>6} / {tn}   ({pct:.1}% close)");
    worst.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    println!("  worst non-root rotation diffs:");
    for (d, ext, name) in worst.iter().take(15) {
        println!("    {d:.3}  .{ext}  {name}");
    }
}

/// Compose every animation in the tag to its written body frames, keyed
/// by lowercased, `:`→space-sanitized name. Returns (node_count, frames).
fn compose_all(tag: &TagFile) -> BTreeMap<String, (usize, Vec<Vec<NodeTransform>>)> {
    let mut out = BTreeMap::new();
    let Ok(animation) = Animation::new(tag) else { return out };
    let skeleton = Skeleton::from_tag(tag);
    let defaults = build_defaults(&skeleton, tag);
    let graph = AnimationGraph::from_tag(tag);

    for group in animation.iter() {
        let Some(name) = &group.name else { continue };
        let Ok(clip) = group.decode() else { continue };
        let kind = JmaKind::from_metadata(
            group.animation_type.as_deref(),
            group.frame_info_type.as_deref(),
            group.world_relative,
        );
        let base = match kind {
            JmaKind::Jmo | JmaKind::Jmr => animation
                .overlay_base_pose(&graph, group, &skeleton, &defaults)
                .unwrap_or_else(|| defaults.clone()),
            _ => defaults.clone(),
        };
        let frames = match kind {
            JmaKind::Jmo => clip.overlay_pose(&skeleton, &base).1.frames,
            JmaKind::Jmr => clip.replacement_pose(&skeleton, &base).frames,
            _ => clip.pose(&skeleton, Some(&defaults)).frames,
        };
        let key = name.replace(':', " ").to_ascii_lowercase();
        out.insert(key, (skeleton.len(), frames));
    }
    out
}

fn build_defaults(skeleton: &Skeleton, jmad: &TagFile) -> Vec<NodeTransform> {
    let mut by_name: BTreeMap<String, NodeTransform> = BTreeMap::new();
    if let Some(block) = jmad.root().field_path("additional node data").and_then(|f| f.as_block()) {
        for i in 0..block.len() {
            let Some(elem) = block.element(i) else { continue };
            let Some(nm) = elem.read_string_id("node name") else { continue };
            if nm.is_empty() { continue; }
            by_name.insert(nm, NodeTransform {
                translation: elem.read_point3d("default translation"),
                rotation: elem.read_quat("default rotation"),
                scale: elem.read_real("default scale").unwrap_or(1.0),
            });
        }
    }
    skeleton.nodes.iter()
        .map(|n| by_name.get(&n.name).copied().unwrap_or(NodeTransform::IDENTITY))
        .collect()
}

/// Best frame-aligned non-root (node>=1) max rotation + translation diff.
/// Rotation compared sign-agnostically (q ~ -q). Translations are ×100 to
/// match JMA cm units. Tries 3 alignments (0, our leading-frame, ref
/// leading-frame) and takes the minimum rotation diff.
fn compare(reff: &[Vec<NodeTransform>], ours: &[Vec<NodeTransform>], nodes: usize) -> (f32, f32) {
    let aligns: [(&[Vec<NodeTransform>], &[Vec<NodeTransform>]); 3] = [
        (reff, ours),
        (reff, ours.get(1..).unwrap_or(&[])),
        (reff.get(1..).unwrap_or(&[]), ours),
    ];
    let mut best = (f32::MAX, f32::MAX);
    for (r, o) in aligns {
        let n = r.len().min(o.len());
        if n == 0 { continue; }
        let (mut rd, mut td) = (0f32, 0f32);
        for f in 0..n {
            for node in 1..nodes.min(r[f].len()).min(o[f].len()) {
                let a = r[f][node];
                let b = o[f][node];
                let qd = quat_diff(a.rotation, b.rotation);
                rd = rd.max(qd);
                td = td.max((a.translation.x - b.translation.x * 100.0).abs());
                td = td.max((a.translation.y - b.translation.y * 100.0).abs());
                td = td.max((a.translation.z - b.translation.z * 100.0).abs());
            }
        }
        if rd < best.0 { best = (rd, td); }
    }
    best
}

fn quat_diff(a: blam_tags::math::RealQuaternion, b: blam_tags::math::RealQuaternion) -> f32 {
    // Our written quaternion is the conjugate (-i,-j,-k,w); the reference
    // stores the same. Compare sign-agnostically.
    let bc = [-b.i, -b.j, -b.k, b.w];
    let av = [a.i, a.j, a.k, a.w];
    let d_pos: f32 = av.iter().zip(bc).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max);
    let d_neg: f32 = av.iter().zip(bc).map(|(x, y)| (x + y).abs()).fold(0.0, f32::max);
    d_pos.min(d_neg)
}

/// Parse a reference JMA: skip header + node block, read `[T(3) R(4) S(1)]`
/// rows into per-frame node arrays. Reference rotation is on-disk i,j,k,w.
fn parse_jma(path: &Path, nodes: usize) -> Option<Vec<Vec<NodeTransform>>> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    let _ver = lines.next()?;
    let _fc = lines.next()?;
    let _fr = lines.next()?;
    let actors: usize = lines.next()?.trim().parse().ok()?;
    for _ in 0..actors { lines.next(); }
    let nn: usize = lines.next()?.trim().parse().ok()?;
    let _cks = lines.next()?;
    for _ in 0..nn * 3 { lines.next(); } // name, child, sibling
    let rest: Vec<&str> = lines.collect();
    let mut frames = Vec::new();
    let mut i = 0;
    while i + 2 < rest.len() {
        let t: Vec<f32> = rest[i].split('\t').filter_map(|x| x.trim().parse().ok()).collect();
        let r: Vec<f32> = rest[i + 1].split('\t').filter_map(|x| x.trim().parse().ok()).collect();
        if t.len() < 3 || r.len() < 4 { break; }
        if frames.is_empty() || frames.last().map(|f: &Vec<NodeTransform>| f.len()) == Some(nodes) {
            frames.push(Vec::with_capacity(nodes));
        }
        let last = frames.last_mut().unwrap();
        last.push(NodeTransform {
            translation: blam_tags::math::RealPoint3d { x: t[0], y: t[1], z: t[2] },
            rotation: blam_tags::math::RealQuaternion { i: r[0], j: r[1], k: r[2], w: r[3] },
            scale: rest.get(i + 2).and_then(|s| s.trim().parse().ok()).unwrap_or(1.0),
        });
        i += 3;
    }
    frames.retain(|f| f.len() == nodes);
    Some(frames)
}

fn walk(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() { stack.push(p); } else { out.push(p); }
        }
    }
    out
}
