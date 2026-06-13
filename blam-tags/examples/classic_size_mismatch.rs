//! Report every block element whose on-disk size (raw bytes) != its
//! resolved schema struct size, across a classic tag tree.
//! Usage: classic_size_mismatch <defs-dir> <tags-root>
use std::path::PathBuf;
use std::collections::BTreeMap;
use blam_tags::classic::{read_classic_tag_file, ClassicHeader};
use blam_tags::layout::TagLayout;
use blam_tags::api::{TagStruct, TagBlock};

fn walk(s: &TagStruct, hits: &mut BTreeMap<(String,String,usize,usize), usize>) {
    for f in s.fields() {
        if let Some(b) = f.as_block() {
            let bn = f.name().to_string();
            for i in 0..b.len() {
                let e = b.element(i).unwrap();
                let (raw, schema) = (e.raw().len(), e.size());
                if raw != schema {
                    *hits.entry((bn.clone(), e.name().to_string(), raw, schema)).or_insert(0) += 1;
                }
                walk(&e, hits);
            }
        } else if let Some(st) = f.as_struct() {
            walk(&st, hits);
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let defs = PathBuf::from(&a[1]); let root = &a[2];
    let mut hits: BTreeMap<(String,String,usize,usize), usize> = BTreeMap::new();
    let mut layouts: BTreeMap<String, Option<()>> = BTreeMap::new();
    let (mut tags, mut tags_with) = (0usize, 0usize);
    let mut npanic = 0usize;
    let mut panics: Vec<String> = Vec::new();
    std::panic::set_hook(Box::new(|_| {}));
    let mut stack = vec![PathBuf::from(root)];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() { stack.push(p); continue; }
            let ext = match p.extension().and_then(|s| s.to_str()) { Some(x) => x.to_string(), None => continue };
            let group_file = format!("{ext}.json");
            let jp = defs.join(&group_file);
            if !jp.exists() { continue; }
            let Ok(bytes) = std::fs::read(&p) else { continue };
            if ClassicHeader::parse(&bytes).is_none() { continue; }
            // Record which group files exist (TagLayout isn't Clone, so
            // reload per tag — fine for a one-off diagnostic).
            layouts.entry(group_file.clone()).or_insert(Some(()));
            let Ok(layout) = TagLayout::from_json(&jp) else { continue };
            let Ok(tag) = read_classic_tag_file(&bytes, layout) else { continue };
            tags += 1;
            let total_before: usize = hits.values().sum();
            let pn = p.strip_prefix(root).unwrap().display().to_string();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                walk(&tag.root(), &mut hits);
            }));
            if r.is_err() {
                if panics.len() < 20 { panics.push(pn); }
                npanic += 1;
            } else if hits.values().sum::<usize>() != total_before { tags_with += 1; }
        }
    }
    println!("scanned {tags} tags; {tags_with} had >=1 mismatch; {npanic} PANICKED during walk");
    for p in &panics { println!("  PANIC {p}"); }
    println!("distinct (block, struct, raw, schema) mismatches:");
    for ((bn, sn, raw, schema), n) in &hits {
        println!("  {n:>6}x  block={bn} struct={sn} raw={raw} schema={schema}");
    }
}
