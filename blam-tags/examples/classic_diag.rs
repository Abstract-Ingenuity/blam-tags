//! Diagnose a single classic tag's decode/roundtrip failure.
//! Usage: classic_diag <defs-dir> <tag-file>
//! Prints the decode error, or the first byte-mismatch offset on roundtrip.

use blam_tags::classic::{classic_roundtrip, ClassicHeader};
use blam_tags::layout::TagLayout;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let defs_dir = PathBuf::from(&args[1]);
    let tag_path = &args[2];

    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(defs_dir.join("_meta.json")).unwrap()).unwrap();
    let mut name_for: BTreeMap<[u8; 4], String> = BTreeMap::new();
    for (k, v) in meta["tag_index"].as_object().unwrap() {
        let mut key = [b' '; 4];
        for (i, b) in k.bytes().take(4).enumerate() {
            key[i] = b;
        }
        name_for.insert(key, v.as_str().unwrap().to_owned());
    }

    let bytes = std::fs::read(tag_path).unwrap();
    let (header, engine) = ClassicHeader::parse(&bytes).expect("not a classic header");
    let name = name_for.get(&header.group_tag).expect("group not in meta");
    println!("group={} engine={:?}", name, engine);
    let layout = TagLayout::from_json(defs_dir.join(format!("{name}.json"))).expect("load layout");
    let body = &bytes[64..];

    match classic_roundtrip(body, &layout, engine) {
        Err(e) => println!("DECODE/ROUNDTRIP-ERR: {e}"),
        Ok(re) if re == body => println!("OK byte-exact ({} bytes)", body.len()),
        Ok(re) => {
            let n = body.len().min(re.len());
            let first = (0..n).find(|&i| body[i] != re[i]).unwrap_or(n);
            println!("MISMATCH at byte {first} (body {} / re {} bytes)", body.len(), re.len());
            let lo = first.saturating_sub(8);
            let hi = (first + 8).min(n);
            println!("  body[{lo}..{hi}] = {:02x?}", &body[lo..hi]);
            println!("  re  [{lo}..{hi}] = {:02x?}", &re[lo..hi]);
        }
    }
}
