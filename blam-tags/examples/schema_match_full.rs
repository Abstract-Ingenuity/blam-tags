//! Full-corpus schema validator.
//!
//! Like `schema_match`, but scans **every** tag of each group rather
//! than the first one it finds. A group passes if *at least one* tag in
//! the corpus has a root-struct layout (size + field count) matching
//! the schema. Groups with zero corpus tags are reported as SKIP.
//!
//! Rationale: Bungie/343 reshaped struct definitions throughout
//! development, and any individual tag carries the layout that was
//! current at *its* save time. A schema can be "correct" against
//! current code but disagree with an older tag. Requiring at least
//! one match against the corpus catches schemas that disagree with
//! *every* tag (a real bug) without rejecting schemas that simply
//! drifted past some older tags.
//!
//! Usage:
//!
//! ```text
//! cargo run --release -p blam-tags --example schema_match_full -- \
//!     definitions/haloreach_mcc /Users/camden/Halo/haloreach_mcc/tags
//! ```

use std::error::Error;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use blam_tags::{TagFile, TagFieldDefinition, TagFieldType, TagStructDefinition};

fn collect_tags_with_ext(root: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(root) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_tags_with_ext(&path, ext, out);
        } else if path.extension() == Some(OsStr::new(ext)) {
            out.push(path);
        }
    }
}

fn list_group_schemas(defs_dir: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(defs_dir)? {
        let path = entry?.path();
        if path.extension() == Some(OsStr::new("json"))
            && path.file_name() != Some(OsStr::new("_meta.json"))
        {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// One field as a (type, normalized_name) tuple — used for set-style
/// diffing between schema and tag. Names are normalized to strip
/// runtime markers ('!', '*'), unit/description suffixes (':units',
/// '#description'), and leading/trailing whitespace, so cosmetic
/// drift between dumper passes doesn't fire false diffs.
type FieldKey = (String, String);

/// Wire-significant fields only — drops editor sentinels whose names
/// aren't reliably preserved across dumper / serializer pairings:
///   * `custom`       — editor group markers (fgrb/fgre) and function
///                       descriptors. 0 bytes on disk, tag blays often
///                       store empty names.
///   * `explanation`  — 0-byte editor text.
///   * `terminator`   — implicit list end.
/// Kept (and contribute to size deltas): pad / useless_pad / skip and
/// every real wire-data type.
fn collect_fields(s: TagStructDefinition<'_>) -> Vec<FieldKey> {
    s.fields()
        .filter(|f| !matches!(
            f.field_type(),
            TagFieldType::Custom | TagFieldType::Explanation | TagFieldType::Terminator,
        ))
        .map(field_key)
        .collect()
}

fn field_key(f: TagFieldDefinition<'_>) -> FieldKey {
    let name = f.name();
    // Strip in this order: '#description' / ':units' / '{alt-name}' /
    // '&flag-name' / runtime markers ('!', '*'). Each is a known
    // dumper convention for annotating fields beyond the canonical
    // name; tags written from older dumps may omit any of them.
    let mut base = name.split('#').next().unwrap_or("").to_owned();
    base = base.split(':').next().unwrap_or("").to_owned();
    if let Some(brace) = base.find('{') {
        base.truncate(brace);
    }
    if let Some(amp) = base.find('&') {
        base.truncate(amp);
    }
    let base = base.trim().trim_end_matches(['!', '*']).trim().to_owned();
    (f.type_name().to_owned(), base)
}


struct GroupResult {
    group: String,
    schema_size: usize,
    schema_fields: usize,
    total_tags: usize,
    matching_tags: usize,
    closest_miss: Option<ClosestMiss>,
    schema_load_err: Option<String>,
    tag_errors: usize,
}

struct ClosestMiss {
    path: PathBuf,
    tag_size: usize,
    tag_fields_count: usize,
    delta: isize,
    /// LCS-aligned rows: each row is `(schema_field, tag_field)` where
    /// either side is `None` when that side has no field at this point
    /// in the alignment. Unchanged rows have both sides populated and
    /// equal; insertions/deletions have a single side.
    aligned: Vec<(Option<FieldKey>, Option<FieldKey>)>,
}

/// Produce an LCS-aligned merge of two ordered sequences. Result is a
/// sequence of `(left, right)` rows where matched items appear on both
/// sides (Some/Some, equal keys) and unmatched items appear on a single
/// side (Some/None or None/Some). Preserves relative order on each side.
fn align_lcs(a: &[FieldKey], b: &[FieldKey]) -> Vec<(Option<FieldKey>, Option<FieldKey>)> {
    let n = a.len();
    let m = b.len();
    // dp[i][j] = LCS length of a[..i] vs b[..j]
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in 0..n {
        for j in 0..m {
            dp[i + 1][j + 1] = if a[i] == b[j] {
                dp[i][j] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut out = Vec::with_capacity(n + m);
    let mut i = n;
    let mut j = m;
    while i > 0 && j > 0 {
        if a[i - 1] == b[j - 1] {
            out.push((Some(a[i - 1].clone()), Some(b[j - 1].clone())));
            i -= 1;
            j -= 1;
        } else if dp[i - 1][j] >= dp[i][j - 1] {
            out.push((Some(a[i - 1].clone()), None));
            i -= 1;
        } else {
            out.push((None, Some(b[j - 1].clone())));
            j -= 1;
        }
    }
    while i > 0 {
        out.push((Some(a[i - 1].clone()), None));
        i -= 1;
    }
    while j > 0 {
        out.push((None, Some(b[j - 1].clone())));
        j -= 1;
    }
    out.reverse();
    out
}

fn check_group(schema_path: &Path, tags_root: &Path) -> GroupResult {
    let group = schema_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .to_string();

    let schema_tag = match TagFile::new(schema_path) {
        Ok(t) => t,
        Err(e) => {
            return GroupResult {
                group,
                schema_size: 0,
                schema_fields: 0,
                total_tags: 0,
                matching_tags: 0,
                closest_miss: None,
                schema_load_err: Some(format!("{e}")),
                tag_errors: 0,
            };
        }
    };
    let schema_root = schema_tag.definitions().root_struct();
    let schema_size = schema_root.size();
    let schema_fields_count = schema_root.fields().count();
    let schema_keys: Vec<FieldKey> = collect_fields(schema_root);

    let mut tags: Vec<PathBuf> = Vec::new();
    collect_tags_with_ext(tags_root, &group, &mut tags);

    let mut matching_tags = 0;
    let mut closest_miss: Option<ClosestMiss> = None;
    let mut tag_errors = 0;

    for tag_path in &tags {
        let real = match TagFile::read(tag_path) {
            Ok(t) => t,
            Err(_) => {
                tag_errors += 1;
                continue;
            }
        };
        let real_root = real.definitions().root_struct();
        let tag_size = real_root.size();
        let tag_fields_count = real_root.fields().count();

        if tag_size == schema_size && tag_fields_count == schema_fields_count {
            matching_tags += 1;
        } else {
            let delta = schema_size as isize - tag_size as isize;
            let abs = delta.unsigned_abs();
            let take = match &closest_miss {
                None => true,
                Some(prev) => abs < prev.delta.unsigned_abs(),
            };
            if take {
                let tag_keys: Vec<FieldKey> = collect_fields(real_root);
                let aligned = align_lcs(&schema_keys, &tag_keys);
                closest_miss = Some(ClosestMiss {
                    path: tag_path.clone(),
                    tag_size,
                    tag_fields_count,
                    delta,
                    aligned,
                });
            }
        }
    }

    GroupResult {
        group,
        schema_size,
        schema_fields: schema_fields_count,
        total_tags: tags.len(),
        matching_tags,
        closest_miss,
        schema_load_err: None,
        tag_errors,
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let defs_dir = PathBuf::from(
        args.next().ok_or("usage: schema_match_full <DEFS_DIR> <TAGS_ROOT>")?,
    );
    let tags_root = PathBuf::from(
        args.next().ok_or("usage: schema_match_full <DEFS_DIR> <TAGS_ROOT>")?,
    );

    let schemas = list_group_schemas(&defs_dir)?;
    println!(
        "Checking {} schemas under {} against tags in {}\n",
        schemas.len(),
        defs_dir.display(),
        tags_root.display()
    );

    let mut pass = 0usize;
    let mut fail = Vec::<GroupResult>::new();
    let mut skip = Vec::<String>::new();
    let mut schema_err = Vec::<GroupResult>::new();

    for schema_path in &schemas {
        let r = check_group(schema_path, &tags_root);
        if r.schema_load_err.is_some() {
            println!("  ERR   {:40}  {}", r.group, r.schema_load_err.as_deref().unwrap_or(""));
            schema_err.push(r);
            continue;
        }
        if r.total_tags == 0 {
            skip.push(r.group);
            continue;
        }
        if r.matching_tags > 0 {
            pass += 1;
            println!(
                "  PASS  {:40}  {}/{} matches",
                r.group, r.matching_tags, r.total_tags
            );
        } else {
            println!(
                "  FAIL  {:40}  0/{} matches  closest Δ={}",
                r.group,
                r.total_tags,
                r.closest_miss.as_ref().map(|m| m.delta).unwrap_or(0),
            );
            fail.push(r);
        }
    }

    println!();
    println!("Summary ({} schemas):", schemas.len());
    println!("  PASS              : {pass}");
    println!("  FAIL              : {}", fail.len());
    println!("  SKIP (no tags)    : {}", skip.len());
    println!("  schema load error : {}", schema_err.len());

    if !fail.is_empty() {
        println!("\nFailing groups (no matching tag in corpus):\n");
        let col_w = 56usize; // each side's column width
        for r in &fail {
            let Some(miss) = &r.closest_miss else {
                println!("  {:40}  (no readable tag)", r.group);
                continue;
            };
            println!(
                "── {}  schema size={} fields={}  vs tag size={} fields={} (Δ={})",
                r.group,
                r.schema_size,
                r.schema_fields,
                miss.tag_size,
                miss.tag_fields_count,
                miss.delta,
            );
            println!("   e.g. {}", miss.path.display());
            // Header
            println!(
                "   {:<col_w$}  │  {:<col_w$}",
                "SCHEMA", "TAG (closest)",
                col_w = col_w,
            );
            println!("   {0:─<col_w$}──┼──{0:─<col_w$}", "", col_w = col_w);
            let mut any_diff = false;
            for (left, right) in &miss.aligned {
                let l_str = match left {
                    Some((ty, name)) => format!("{ty} '{name}'"),
                    None => String::new(),
                };
                let r_str = match right {
                    Some((ty, name)) => format!("{ty} '{name}'"),
                    None => String::new(),
                };
                let mark = match (left, right) {
                    (Some(_), None) => '<',
                    (None, Some(_)) => '>',
                    _ => ' ',
                };
                if mark != ' ' { any_diff = true; }
                // Truncate long names so columns stay aligned.
                let l_disp: String = l_str.chars().take(col_w).collect();
                let r_disp: String = r_str.chars().take(col_w).collect();
                println!("   {l_disp:<col_w$} {mark}│ {mark}{r_disp:<col_w$}", col_w = col_w);
            }
            if !any_diff {
                println!("   (no field-list drift on wire-significant fields; size still differs by {} — primitive type drift)", miss.delta);
            }
            if r.tag_errors > 0 {
                println!("   ({} tag(s) failed to read)", r.tag_errors);
            }
            println!();
        }
    }

    if !schema_err.is_empty() {
        println!("\nSchema load errors:");
        for r in &schema_err {
            println!("  {:40}  {}", r.group, r.schema_load_err.as_deref().unwrap_or(""));
        }
    }

    Ok(())
}
