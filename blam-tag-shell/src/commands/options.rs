use anyhow::{Context, Result};
use blam_tags::fields::{field_option_names, TagFieldData, TagFieldType};
use blam_tags::file::TagFile;
use blam_tags::path::lookup;

pub fn run(file: &str, path: &str) -> Result<()> {
    let tag = TagFile::read(file).map_err(|e| anyhow::anyhow!("failed to load tag file: {e}"))?;
    let layout = &tag.tag_stream.layout.layout;

    let cursor = lookup(layout, &tag.tag_stream.data, path)
        .with_context(|| format!("field '{}' not found", path))?;

    let field = &layout.fields[cursor.field_index];
    let is_enum = matches!(
        field.field_type,
        TagFieldType::CharEnum | TagFieldType::ShortEnum | TagFieldType::LongEnum
    );
    let is_flags = matches!(
        field.field_type,
        TagFieldType::ByteFlags | TagFieldType::WordFlags | TagFieldType::LongFlags
    );

    if !is_enum && !is_flags {
        anyhow::bail!("field '{}' is not an enum or flags field", path);
    }

    let option_names: Vec<&str> = field_option_names(layout, field).collect();

    let parsed = cursor.parse(layout).context("failed to parse field value")?;

    if is_enum {
        let current_value: Option<i64> = match &parsed {
            TagFieldData::CharEnum { value, .. } => Some(*value as i64),
            TagFieldData::ShortEnum { value, .. } => Some(*value as i64),
            TagFieldData::LongEnum { value, .. } => Some(*value as i64),
            _ => None,
        };
        println!("Enum options for '{}':", path);
        for (i, name) in option_names.iter().enumerate() {
            let marker = if current_value == Some(i as i64) { " <-" } else { "" };
            println!("  {i}: {name}{marker}");
        }
    } else {
        println!("Flag options for '{}':", path);
        for (bit, name) in option_names.iter().enumerate() {
            let is_set = parsed.flag_bit(bit as u32).unwrap_or(false);
            let marker = if is_set { "[x]" } else { "[ ]" };
            println!("  {bit}: {marker} {name}");
        }
    }

    Ok(())
}
