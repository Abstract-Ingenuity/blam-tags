//! Per-game `blay` structural-variance check.
//!
//! Walks a tag-directory tree, groups files by `group_tag`, parses each
//! file's `blay` chunk into a [`TagLayout`], then builds a canonical
//! text fingerprint by walking the block/struct/field tree starting
//! from the root (tag-group) block. Two layouts with the same tree —
//! same block names + max counts, same struct names + sizes, same
//! field names + type names + offsets — produce identical fingerprints
//! even when their on-disk `blay` bytes differ (different string-table
//! ordering, reordered definition tables, etc.).
//!
//! Goal: if every group has one canonical fingerprint, the schema is
//! stable per group and we can pick any tag's blay as the representative
//! (variance is purely byte-packing). If a group has >1 fingerprints,
//! the schema genuinely diverges per tag and we need a different
//! storage strategy (DLL dump or superset synthesis).
//!
//! Usage:
//!
//! ```text
//! cargo run --release -p blam-tags --example blay_variance -- <DIR> [<DIR>...]
//! ```

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::Write as _;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use blam_tags::layout::TagLayout;
use blam_tags::TagFieldType;

fn collect_tag_paths<P: AsRef<Path>>(dir: P) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut paths = vec![];

    if dir.as_ref().is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                paths.extend(collect_tag_paths(&path)?);
            } else {
                if let Some(file_name) = path.file_name() && file_name == ".DS_Store" {
                    continue;
                }
                if let Some(extension) = path.extension() && extension == "txt" {
                    continue;
                }
                paths.push(path);
            }
        }
    }

    Ok(paths)
}

fn read_u32_le<R: Read>(r: &mut R) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

struct TagInfo {
    group_tag: u32,
    layout: TagLayout,
}

/// Seek to the start of the file's `blay` chunk and parse it.
/// Skips the 64-byte file header and the 12-byte `tag!` chunk header
/// (which brackets the `blay` + `bdat` body) but leaves the `blay`
/// chunk header in-stream for [`TagLayout::read`] to consume.
fn read_tag_layout<P: AsRef<Path>>(path: P) -> Result<TagInfo, Box<dyn Error>> {
    let file = std::fs::File::open(&path)?;
    let mut r = BufReader::with_capacity(64 * 1024, file);

    // group_tag lives at offset 48 in the 64-byte file header.
    r.seek(SeekFrom::Start(48))?;
    let group_tag = read_u32_le(&mut r)?;

    // Past the file header.
    r.seek(SeekFrom::Start(64))?;

    // tag! chunk header: validate and skip.
    let tag_sig = read_u32_le(&mut r)?;
    if tag_sig != u32::from_be_bytes(*b"tag!") {
        return Err(format!(
            "{}: expected 'tag!' chunk, got 0x{:08X}",
            path.as_ref().display(),
            tag_sig,
        )
        .into());
    }
    let _tag_ver = read_u32_le(&mut r)?;
    let _tag_size = read_u32_le(&mut r)?;

    // Now positioned at the start of the blay chunk header — exactly
    // what TagLayout::read expects.
    let layout = TagLayout::read(&mut r)?;

    Ok(TagInfo { group_tag, layout })
}

//
// Structural fingerprint: walk block -> struct -> fields, recursing
// into nested struct / block / array / resource / interop fields.
// Emits a canonical indented text form with block name + max_count,
// struct name + size, field name + type name + offset.
//

fn indent(depth: usize, out: &mut String) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn fingerprint_block(layout: &TagLayout, block_index: usize, depth: usize, out: &mut String) {
    let block = &layout.block_layouts[block_index];
    let name = layout.get_string(block.name_offset).unwrap_or("");
    indent(depth, out);
    let _ = writeln!(out, "block:{} max={}", name, block.max_count);
    fingerprint_struct(layout, block.struct_index as usize, depth + 1, out);
}

fn fingerprint_struct(layout: &TagLayout, struct_index: usize, depth: usize, out: &mut String) {
    let st = &layout.struct_layouts[struct_index];
    let name = layout.get_string(st.name_offset).unwrap_or("");
    indent(depth, out);
    let _ = writeln!(out, "struct:{} size={}", name, st.size);

    let mut field_idx = st.first_field_index as usize;
    loop {
        let field = &layout.fields[field_idx];
        if field.field_type == TagFieldType::Terminator {
            break;
        }

        let fname = layout.get_string(field.name_offset).unwrap_or("");
        let ft = &layout.field_types[field.type_index as usize];
        let tname = layout.get_string(ft.name_offset).unwrap_or("");

        indent(depth + 1, out);
        let _ = writeln!(out, "field:{} type:{} off={}", fname, tname, field.offset);

        match field.field_type {
            TagFieldType::Struct => {
                fingerprint_struct(layout, field.definition as usize, depth + 2, out);
            }
            TagFieldType::Block => {
                fingerprint_block(layout, field.definition as usize, depth + 2, out);
            }
            TagFieldType::Array => {
                let arr = &layout.array_layouts[field.definition as usize];
                let aname = layout.get_string(arr.name_offset).unwrap_or("");
                indent(depth + 2, out);
                let _ = writeln!(out, "array:{} count={}", aname, arr.count);
                fingerprint_struct(layout, arr.struct_index as usize, depth + 3, out);
            }
            TagFieldType::PageableResource => {
                let res = &layout.resource_layouts[field.definition as usize];
                let rname = layout.get_string(res.name_offset).unwrap_or("");
                indent(depth + 2, out);
                let _ = writeln!(out, "resource:{}", rname);
                fingerprint_struct(layout, res.struct_index as usize, depth + 3, out);
            }
            TagFieldType::ApiInterop => {
                let ai = &layout.interop_layouts[field.definition as usize];
                let iname = layout.get_string(ai.name_offset).unwrap_or("");
                indent(depth + 2, out);
                let _ = writeln!(out, "interop:{}", iname);
                fingerprint_struct(layout, ai.struct_index as usize, depth + 3, out);
            }
            _ => {}
        }

        field_idx += 1;
    }
}

fn fingerprint(layout: &TagLayout) -> String {
    let mut buf = String::new();
    let root = layout.header.tag_group_block_index as usize;
    fingerprint_block(layout, root, 0, &mut buf);
    buf
}

//
// CLI plumbing
//

fn parse_args() -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut roots = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                println!(
                    "Usage: blay_variance <DIR> [<DIR>...]\n\
                     \n\
                     Walks each <DIR> recursively, parses every tag's blay chunk,\n\
                     and reports how many distinct structural fingerprints exist\n\
                     per group (block/struct/field tree, ignoring table ordering)."
                );
                std::process::exit(0);
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown flag: {other}").into());
            }
            _ => roots.push(PathBuf::from(arg)),
        }
    }
    if roots.is_empty() {
        return Err("expected at least one tag root directory (run with --help for usage)".into());
    }
    Ok(roots)
}

fn group_tag_display(g: u32) -> String {
    let bytes = g.to_be_bytes();
    let mut s = String::with_capacity(4);
    for &b in &bytes {
        if b == 0 || b == b' ' {
            s.push('_');
        } else if b.is_ascii_graphic() {
            s.push(b as char);
        } else {
            s.push('?');
        }
    }
    s
}

fn hex16(bytes: &[u8; 16]) -> String {
    let mut s = String::with_capacity(32);
    for b in bytes {
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Diff-line cap per outlier variant so scenario-sized fingerprints
/// don't blow up the report.
const DIFF_LINE_LIMIT: usize = 80;

/// Line-level diff between two fingerprint texts with `~`-style
/// modification pairing.
///
/// Phase 1 — bag-of-lines set difference: any line present more times
/// in `a` than `b` becomes a candidate `-`; any line present more times
/// in `b` than `a` becomes a candidate `+`.
///
/// Phase 2 — pair `-` with `+` entries whose (indent, identity-key)
/// match (same `field:<name> type:<type>` at the same depth, or same
/// `struct:<name>` / `block:<name>` / `array:<name>`). Paired entries
/// collapse into a single `~` modification line showing only the
/// attribute change (e.g. `off=24 -> off=16`, `size=76 -> size=52`).
/// Unpaired `-` / `+` remain as true removals / additions.
fn diff_fingerprints(a: &str, b: &str, line_limit: usize) -> String {
    use std::collections::HashMap;

    // counts[line] = (count in a) - (count in b).
    let mut counts: HashMap<&str, i32> = HashMap::new();
    for line in a.lines() {
        *counts.entry(line).or_insert(0) += 1;
    }
    for line in b.lines() {
        *counts.entry(line).or_insert(0) -= 1;
    }

    // Phase 1: collect `-` lines in order of `a`, `+` lines in order of `b`.
    let mut minus_lines: Vec<&str> = Vec::new();
    let mut seen_minus: HashMap<&str, i32> = HashMap::new();
    for line in a.lines() {
        let excess = *counts.get(line).unwrap_or(&0);
        if excess <= 0 {
            continue;
        }
        let used = seen_minus.entry(line).or_insert(0);
        if *used >= excess {
            continue;
        }
        *used += 1;
        minus_lines.push(line);
    }

    let mut plus_lines: Vec<&str> = Vec::new();
    let mut seen_plus: HashMap<&str, i32> = HashMap::new();
    for line in b.lines() {
        let excess = *counts.get(line).unwrap_or(&0);
        if excess >= 0 {
            continue;
        }
        let want = -excess;
        let used = seen_plus.entry(line).or_insert(0);
        if *used >= want {
            continue;
        }
        *used += 1;
        plus_lines.push(line);
    }

    // Phase 2: greedy pairing by (indent, identity-key). For each
    // minus line, find the first unconsumed plus with a matching
    // identity; if found, emit `~` with the attribute delta; otherwise
    // emit as `-`. Remaining pluses emit as `+` at the end.
    let mut plus_consumed = vec![false; plus_lines.len()];
    let mut diff_lines: Vec<String> = Vec::new();

    for m in &minus_lines {
        let (mi, mk, ma) = split_identity(m);
        let match_idx = plus_lines
            .iter()
            .enumerate()
            .position(|(i, p)| {
                if plus_consumed[i] {
                    return false;
                }
                let (pi, pk, _) = split_identity(p);
                pi == mi && pk == mk
            });
        if let Some(i) = match_idx {
            let (_, _, pa) = split_identity(plus_lines[i]);
            plus_consumed[i] = true;
            // Suppress field offset cascades. When the only delta is
            // `off=`, this field/struct didn't actually change — an
            // insertion or deletion higher up shifted everything below
            // it. Keep `~` only for intrinsic shape changes (size,
            // max, count).
            let is_offset_only = ma.trim_start().starts_with("off=");
            if !is_offset_only {
                diff_lines.push(format!(
                    "~ {}{} ({} -> {})",
                    mi,
                    mk,
                    if ma.is_empty() { "-" } else { ma.trim_start() },
                    if pa.is_empty() { "-" } else { pa.trim_start() },
                ));
            }
        } else {
            diff_lines.push(format!("- {}", m));
        }
    }
    for (i, p) in plus_lines.iter().enumerate() {
        if !plus_consumed[i] {
            diff_lines.push(format!("+ {}", p));
        }
    }

    let total = diff_lines.len();
    let mut out = String::new();
    for line in diff_lines.iter().take(line_limit) {
        out.push_str(line);
        out.push('\n');
    }
    if total > line_limit {
        let _ = writeln!(
            out,
            "... {} more diff line{} truncated (total {})",
            total - line_limit,
            if total - line_limit == 1 { "" } else { "s" },
            total,
        );
    }
    out
}

/// Split a fingerprint line into (indent, identity-key, attrs-suffix).
///
/// Example: `"      field:velocity j type:struct off=24"` →
/// `("      ", "field:velocity j type:struct", " off=24")`.
///
/// Identity-key is the line's content up to (but not including) the
/// rightmost ` off=` / ` size=` / ` max=` / ` count=` marker. That
/// marker plus the value that follows it is the attrs-suffix. Lines
/// with no marker return an empty attrs-suffix.
fn split_identity(line: &str) -> (&str, &str, &str) {
    let indent_len = line.bytes().take_while(|&b| b == b' ').count();
    let (indent, rest) = line.split_at(indent_len);

    const MARKERS: &[&str] = &[" off=", " size=", " max=", " count="];
    let mut best: Option<usize> = None;
    for m in MARKERS {
        if let Some(pos) = rest.rfind(m)
            && best.map_or(true, |b| pos > b)
        {
            best = Some(pos);
        }
    }
    if let Some(pos) = best {
        (indent, &rest[..pos], &rest[pos..])
    } else {
        (indent, rest, "")
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let roots = parse_args()?;

    let mut paths = Vec::new();
    for root in &roots {
        paths.extend(collect_tag_paths(root)?);
    }

    let total = paths.len();
    eprintln!("Scanning {} files...", total);

    // group_tag -> fingerprint-digest -> (fingerprint text, sample paths).
    // The fingerprint text is retained per digest so the outlier section
    // can diff each non-majority variant against the majority.
    let mut by_group: BTreeMap<u32, BTreeMap<[u8; 16], (String, Vec<PathBuf>)>> = BTreeMap::new();
    let mut failures: Vec<(PathBuf, Box<dyn Error>)> = Vec::new();

    for (i, path) in paths.iter().enumerate() {
        if i > 0 && i % 5_000 == 0 {
            eprintln!("  [{i}/{total}]");
        }
        match read_tag_layout(path) {
            Ok(info) => {
                let fp = fingerprint(&info.layout);
                let digest = *md5::compute(fp.as_bytes());
                let slot = by_group
                    .entry(info.group_tag)
                    .or_default()
                    .entry(digest)
                    .or_insert_with(|| (fp, Vec::new()));
                slot.1.push(path.clone());
            }
            Err(e) => failures.push((path.clone(), e)),
        }
    }

    //
    // Summary table
    //

    println!();
    println!("== Structural blay variance by group ==");
    println!("{:<6} {:>8} {:>10}", "group", "files", "variants");
    println!("{:-<26}", "");

    let mut groups_with_variance: Vec<u32> = Vec::new();
    let mut total_files = 0usize;

    for (group, digests) in &by_group {
        let files: usize = digests.values().map(|(_, v)| v.len()).sum();
        let distinct = digests.len();
        total_files += files;
        println!("{:<6} {:>8} {:>10}", group_tag_display(*group), files, distinct);
        if distinct > 1 {
            groups_with_variance.push(*group);
        }
    }

    println!("{:-<26}", "");
    println!(
        "{:<6} {:>8} {:>10}   ({} group{}, {} with variance)",
        "total",
        total_files,
        by_group.values().map(|d| d.len()).sum::<usize>(),
        by_group.len(),
        if by_group.len() == 1 { "" } else { "s" },
        groups_with_variance.len(),
    );

    //
    // Outliers: for each group with variance, pick the variant with
    // the largest file count as the "majority consensus" and list every
    // file in a non-majority variant. Ties (no strict majority) are
    // flagged inline.
    //

    if !groups_with_variance.is_empty() {
        let mut total_outliers = 0usize;

        println!();
        println!("== Non-majority files (outliers) ==");
        println!(
            "(diff legend: '-' removed, '+' added, '~' shape changed \
             (size/max/count); field offset cascades suppressed)"
        );

        for group in &groups_with_variance {
            let digests = &by_group[group];

            // Sort variants by descending file count, then by digest for
            // deterministic tie-breaking.
            let mut variants: Vec<(&[u8; 16], &(String, Vec<PathBuf>))> = digests.iter().collect();
            variants.sort_by(|a, b| b.1 .1.len().cmp(&a.1 .1.len()).then(a.0.cmp(b.0)));

            let (majority_digest, majority_entry) = variants[0];
            let (majority_fp, majority_files) = majority_entry;
            let majority_count = majority_files.len();
            // A strict majority requires the top variant to be strictly
            // larger than the runner-up.
            let tied = variants.len() > 1 && variants[1].1 .1.len() == majority_count;

            let outlier_count: usize =
                variants[1..].iter().map(|(_, (_, f))| f.len()).sum();
            total_outliers += outlier_count;

            println!(
                "\n{} (majority md5={} {}/{} file{}{}):",
                group_tag_display(*group),
                hex16(majority_digest),
                majority_count,
                digests.values().map(|(_, v)| v.len()).sum::<usize>(),
                if majority_count == 1 { "" } else { "s" },
                if tied { " — TIED, no strict majority" } else { "" },
            );

            for (digest, (variant_fp, files)) in &variants[1..] {
                println!(
                    "  variant md5={} ({} file{}):",
                    hex16(digest),
                    files.len(),
                    if files.len() == 1 { "" } else { "s" },
                );
                let mut sorted = files.to_vec();
                sorted.sort();
                for f in &sorted {
                    println!("    {}", f.display());
                }
                // Diff this variant against the majority. '-' lines are
                // structure present in the majority but missing here; '+'
                // lines are structure present in this variant but missing
                // from the majority.
                let diff = diff_fingerprints(majority_fp, variant_fp, DIFF_LINE_LIMIT);
                if diff.is_empty() {
                    // Digests differ, but every diff line was an
                    // offset-only `~` that the suppression rule hid.
                    // In practice this means cascade-only shifts (our
                    // pairing missed the true source add/remove) OR a
                    // genuine field reorder — both rare. Worth
                    // flagging loudly so we investigate if it ever
                    // shows up.
                    println!(
                        "    diff vs. majority: only offset-cascade / \
                         possible-reorder changes (investigate)"
                    );
                } else {
                    println!("    diff vs. majority:");
                    for line in diff.lines() {
                        println!("      {}", line);
                    }
                }
            }
        }

        println!();
        println!(
            "Total outliers across {} group{}: {} file{}",
            groups_with_variance.len(),
            if groups_with_variance.len() == 1 { "" } else { "s" },
            total_outliers,
            if total_outliers == 1 { "" } else { "s" },
        );
    } else {
        println!();
        println!("All groups have a single canonical structural fingerprint.");
    }

    if !failures.is_empty() {
        println!();
        println!("== Failures ({}) ==", failures.len());
        for (p, e) in failures.iter().take(20) {
            println!("  {}: {}", p.display(), e);
        }
        if failures.len() > 20 {
            println!("  ... {} more", failures.len() - 20);
        }
    }

    Ok(())
}
