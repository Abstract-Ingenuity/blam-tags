//! Diagnostic — dump the three byte regions of every `tag_resource`
//! field in a tag: the 8 inline bytes (in the parent struct's raw
//! region), the resource header struct, and the `tgdt` payload.
//!
//! Usage: inspect_resource <TAG_FILE>

use std::error::Error;
use std::path::PathBuf;

use blam_tags::{TagField, TagFile, TagResourceKind, TagStruct};

fn main() -> Result<(), Box<dyn Error>> {
    let path = PathBuf::from(
        std::env::args().nth(1).ok_or("usage: inspect_resource <TAG_FILE>")?,
    );
    let tag = TagFile::read(&path)?;
    println!("tag: {}", path.display());

    let mut path_buf = String::new();
    visit_struct(tag.root(), &mut path_buf, 0);
    Ok(())
}

fn visit_struct(s: TagStruct<'_>, path: &mut String, depth: usize) {
    for field in s.fields() {
        let saved = path.len();
        if !path.is_empty() {
            path.push('/');
        }
        path.push_str(field.name());

        if let Some(resource) = field.as_resource() {
            print_resource(&field, resource, path, depth);
        } else if let Some(nested) = field.as_struct() {
            visit_struct(nested, path, depth + 1);
        } else if let Some(block) = field.as_block() {
            for (i, elem) in block.iter().enumerate() {
                let elem_saved = path.len();
                use std::fmt::Write;
                let _ = write!(path, "[{i}]");
                visit_struct(elem, path, depth + 1);
                path.truncate(elem_saved);
            }
        } else if let Some(array) = field.as_array() {
            for (i, elem) in array.iter().enumerate() {
                let elem_saved = path.len();
                use std::fmt::Write;
                let _ = write!(path, "[{i}]");
                visit_struct(elem, path, depth + 1);
                path.truncate(elem_saved);
            }
        }

        path.truncate(saved);
    }
}

fn print_resource(
    field: &TagField<'_>,
    resource: blam_tags::TagResource<'_>,
    path: &str,
    _depth: usize,
) {
    let def = resource.definition();
    let header_def = def.struct_definition();

    println!();
    println!("== {path} ==");
    println!("  field: {} ({})", field.name(), field.type_name());
    println!("  resource definition: {}", def.name());
    println!(
        "  header struct: {} (declared size {} bytes)",
        header_def.name(),
        header_def.size(),
    );
    println!("  kind: {:?}", resource.kind());

    let inline = resource.inline_bytes();
    println!("  inline 8 bytes (in parent struct): {}", hex(inline));

    match resource.kind() {
        TagResourceKind::Null => {
            println!("  (null — no header bytes / payload)");
        }
        TagResourceKind::Xsync => {
            if let Some(p) = resource.xsync_payload() {
                println!("  xsync payload: {} bytes", p.len());
            }
        }
        TagResourceKind::Exploded => {
            if let Some(payload) = resource.exploded_payload() {
                println!("  tgdt payload: {} bytes", payload.len());
                let preview = &payload[..payload.len().min(32)];
                println!("    first {} bytes: {}", preview.len(), hex(preview));
            }
            match resource.as_struct() {
                Some(header) => {
                    println!(
                        "  header struct fields ({} visible):",
                        header.fields().count(),
                    );
                    for f in header.fields() {
                        let summary = if let Some(b) = f.as_block() {
                            format!("block [{} elements]", b.len())
                        } else if let Some(a) = f.as_array() {
                            format!("array [{} elements]", a.len())
                        } else if let Some(v) = f.value() {
                            format!("{v:?}")
                        } else if f.as_struct().is_some() {
                            "struct".to_owned()
                        } else {
                            "(no value)".to_owned()
                        };
                        println!("    {}: {} = {}", f.name(), f.type_name(), summary);
                    }
                }
                None => {
                    println!("  header struct: as_struct() returned None");
                }
            }
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}
