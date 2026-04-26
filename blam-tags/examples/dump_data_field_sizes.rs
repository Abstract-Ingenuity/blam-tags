//! Dump cumulative byte sizes of all `tag_data` fields in a tag.
use blam_tags::{TagFile, TagFieldData};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).unwrap();
    let tag = TagFile::read(&path)?;
    let mut total = 0usize;
    let mut count = 0usize;
    let mut max = (0usize, String::new());
    fn walk(s: blam_tags::TagStruct<'_>, path: &mut String, total: &mut usize, count: &mut usize, max: &mut (usize, String)) {
        for f in s.fields() {
            let saved = path.len();
            if !path.is_empty() { path.push('/'); }
            path.push_str(f.name());
            if let Some(bytes) = f.as_data() {
                *total += bytes.len();
                *count += 1;
                if bytes.len() > max.0 {
                    *max = (bytes.len(), path.clone());
                }
            } else if let Some(nested) = f.as_struct() {
                walk(nested, path, total, count, max);
            } else if let Some(b) = f.as_block() {
                for (i, e) in b.iter().enumerate() {
                    use std::fmt::Write;
                    let bs = path.len();
                    let _ = write!(path, "[{i}]");
                    walk(e, path, total, count, max);
                    path.truncate(bs);
                }
            } else if let Some(a) = f.as_array() {
                for (i, e) in a.iter().enumerate() {
                    use std::fmt::Write;
                    let bs = path.len();
                    let _ = write!(path, "[{i}]");
                    walk(e, path, total, count, max);
                    path.truncate(bs);
                }
            }
            path.truncate(saved);
        }
        // Suppress unused-variant warning
        let _ = TagFieldData::Real(0.0);
    }
    let mut path = String::new();
    walk(tag.root(), &mut path, &mut total, &mut count, &mut max);
    println!("total tag_data bytes: {} ({} fields)", total, count);
    println!("largest: {} bytes at {}", max.0, max.1);
    Ok(())
}
