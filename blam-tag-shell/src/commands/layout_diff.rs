use anyhow::Result;
use blam_tags::{format_group_tag, TagFieldDefinition, TagFile, TagStructDefinition};

pub fn run(file_a: &str, file_b: &str) -> Result<()> {
    let tag_a = TagFile::read(file_a).map_err(|e| anyhow::anyhow!("failed to parse first file: {e}"))?;
    let tag_b = TagFile::read(file_b).map_err(|e| anyhow::anyhow!("failed to parse second file: {e}"))?;

    let name_a = std::path::Path::new(file_a)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(file_a);
    let name_b = std::path::Path::new(file_b)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(file_b);

    println!("Layout diff: {name_a} vs {name_b}");
    println!();

    let group_a = tag_a.group();
    let group_b = tag_b.group();
    if group_a.tag != group_b.tag {
        println!("  group_tag: {} -> {}", format_group_tag(group_a.tag), format_group_tag(group_b.tag));
    }
    if group_a.version != group_b.version {
        println!("  group_version: {} -> {}", group_a.version, group_b.version);
    }

    diff_struct(tag_a.definitions().root_struct(), tag_b.definitions().root_struct(), 1);

    Ok(())
}

fn diff_struct(a: TagStructDefinition<'_>, b: TagStructDefinition<'_>, indent: usize) {
    let pad = "  ".repeat(indent);
    let name_changed = a.name() != b.name();
    let size_changed = a.size() != b.size();
    let guid_changed = a.guid() != b.guid();

    let fields_a: Vec<TagFieldDefinition<'_>> = a.fields().collect();
    let fields_b: Vec<TagFieldDefinition<'_>> = b.fields().collect();

    if !name_changed && !size_changed && !guid_changed && fields_equal(&fields_a, &fields_b) {
        return;
    }

    if name_changed {
        println!("{pad}struct {} -> {}:", a.name(), b.name());
    } else {
        println!("{pad}struct {}:", a.name());
    }

    if guid_changed {
        println!("{pad}  guid: {:02x?} -> {:02x?}", &a.guid()[..4], &b.guid()[..4]);
    }
    if size_changed {
        let delta = b.size() as isize - a.size() as isize;
        println!("{pad}  size: {} -> {} ({:+})", a.size(), b.size(), delta);
    }

    // Removed fields.
    for field in &fields_a {
        let name = field.name();
        if name.is_empty() || !fields_b.iter().any(|f| f.name() == name) {
            println!("{pad}  - {name} : {} @ {}", field.type_name(), field.offset());
        }
    }

    // Added fields.
    for field in &fields_b {
        let name = field.name();
        if name.is_empty() || !fields_a.iter().any(|f| f.name() == name) {
            println!("{pad}  + {name} : {} @ {}", field.type_name(), field.offset());
        }
    }

    // Changed / recursed fields.
    for field_a in &fields_a {
        let name = field_a.name();
        if name.is_empty() {
            continue;
        }
        let Some(field_b) = fields_b.iter().find(|f| f.name() == name) else {
            continue;
        };

        let mut changes = Vec::new();
        if field_a.field_type() != field_b.field_type() {
            changes.push(format!("type: {} -> {}", field_a.type_name(), field_b.type_name()));
        }
        if field_a.offset() != field_b.offset() {
            changes.push(format!("offset: {} -> {}", field_a.offset(), field_b.offset()));
        }
        if !changes.is_empty() {
            println!("{pad}  ~ {name} : {}", changes.join(", "));
        }

        // Recurse into nested struct / block / array.
        if let (Some(sa), Some(sb)) = (field_a.as_struct(), field_b.as_struct()) {
            diff_struct(sa, sb, indent + 2);
        } else if let (Some(ba), Some(bb)) = (field_a.as_block(), field_b.as_block()) {
            if ba.max_count() != bb.max_count() {
                println!("{pad}    block max_count: {} -> {}", ba.max_count(), bb.max_count());
            }
            diff_struct(ba.struct_definition(), bb.struct_definition(), indent + 2);
        } else if let (Some(aa), Some(ab)) = (field_a.as_array(), field_b.as_array()) {
            if aa.count() != ab.count() {
                println!("{pad}    array count: {} -> {}", aa.count(), ab.count());
            }
            diff_struct(aa.struct_definition(), ab.struct_definition(), indent + 2);
        }
    }
}

fn fields_equal(a: &[TagFieldDefinition<'_>], b: &[TagFieldDefinition<'_>]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(fa, fb)| {
        fa.name() == fb.name()
            && fa.field_type() == fb.field_type()
            && fa.offset() == fb.offset()
    })
}
