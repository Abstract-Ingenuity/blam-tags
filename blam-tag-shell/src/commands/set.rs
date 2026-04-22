use anyhow::{Context, Result};
use blam_tags::fields::{
    find_enum_option_index, parse_group_tag as lib_parse_group_tag, StringIdData, TagFieldData,
    TagFieldType, TagReferenceData,
};
use blam_tags::file::TagFile;
use blam_tags::path::lookup_mut;

pub fn run(
    file: &str,
    path: &str,
    value: &str,
    output: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    let mut tag =
        TagFile::read(file).map_err(|e| anyhow::anyhow!("failed to load tag file: {e}"))?;

    {
        let tag_stream = &mut tag.tag_stream;
        let layout = &tag_stream.layout.layout;

        let mut cursor = lookup_mut(layout, &mut tag_stream.data, path)
            .with_context(|| format!("field '{}' not found", path))?;

        let field = &layout.fields[cursor.field_index];
        let parsed = parse_user_value(layout, field, value)?;
        cursor.set(layout, parsed);
    }

    if dry_run {
        println!("(dry run) would set {path} = {value}");
        return Ok(());
    }

    let out_path = output.unwrap_or(file);
    tag.write(out_path)
        .map_err(|e| anyhow::anyhow!("failed to save tag file: {e}"))?;
    println!("set {path} = {value}");
    if out_path != file {
        println!("saved to {out_path}");
    }

    Ok(())
}

/// Translate a CLI string into a `TagFieldData` value appropriate for
/// `field`'s type. Enum fields accept either a variant name (matched
/// case-insensitively) or an integer.
fn parse_user_value(
    layout: &blam_tags::layout::TagLayout,
    field: &blam_tags::layout::TagFieldDefinition,
    input: &str,
) -> Result<TagFieldData> {
    match field.field_type {
        TagFieldType::CharInteger => Ok(TagFieldData::CharInteger(
            input.parse().context("expected i8")?,
        )),
        TagFieldType::ShortInteger => Ok(TagFieldData::ShortInteger(
            input.parse().context("expected i16")?,
        )),
        TagFieldType::LongInteger => Ok(TagFieldData::LongInteger(
            input.parse().context("expected i32")?,
        )),
        TagFieldType::Int64Integer => Ok(TagFieldData::Int64Integer(
            input.parse().context("expected i64")?,
        )),
        TagFieldType::Tag => Ok(TagFieldData::Tag(parse_group_tag(input)?)),

        TagFieldType::Angle => Ok(TagFieldData::Angle(input.parse().context("expected f32")?)),
        TagFieldType::Real => Ok(TagFieldData::Real(input.parse().context("expected f32")?)),
        TagFieldType::RealSlider => Ok(TagFieldData::RealSlider(input.parse().context("expected f32")?)),
        TagFieldType::RealFraction => Ok(TagFieldData::RealFraction(
            input.parse().context("expected f32")?,
        )),

        TagFieldType::CharEnum => Ok(TagFieldData::CharEnum {
            value: parse_enum_value(layout, field, input)? as i8,
            name: None,
        }),
        TagFieldType::ShortEnum => Ok(TagFieldData::ShortEnum {
            value: parse_enum_value(layout, field, input)? as i16,
            name: None,
        }),
        TagFieldType::LongEnum => Ok(TagFieldData::LongEnum {
            value: parse_enum_value(layout, field, input)?,
            name: None,
        }),

        TagFieldType::ByteFlags => Ok(TagFieldData::ByteFlags {
            value: parse_int(input)? as u8,
            names: Vec::new(),
        }),
        TagFieldType::WordFlags => Ok(TagFieldData::WordFlags {
            value: parse_int(input)? as u16,
            names: Vec::new(),
        }),
        TagFieldType::LongFlags => Ok(TagFieldData::LongFlags {
            value: parse_int(input)? as i32,
            names: Vec::new(),
        }),

        TagFieldType::ByteBlockFlags => Ok(TagFieldData::ByteBlockFlags(parse_int(input)? as u8)),
        TagFieldType::WordBlockFlags => Ok(TagFieldData::WordBlockFlags(parse_int(input)? as u16)),
        TagFieldType::LongBlockFlags => Ok(TagFieldData::LongBlockFlags(parse_int(input)? as i32)),

        TagFieldType::CharBlockIndex => Ok(TagFieldData::CharBlockIndex(parse_block_index(input)? as i8)),
        TagFieldType::CustomCharBlockIndex => Ok(TagFieldData::CustomCharBlockIndex(
            parse_block_index(input)? as i8,
        )),
        TagFieldType::ShortBlockIndex => Ok(TagFieldData::ShortBlockIndex(
            parse_block_index(input)? as i16,
        )),
        TagFieldType::CustomShortBlockIndex => Ok(TagFieldData::CustomShortBlockIndex(
            parse_block_index(input)? as i16,
        )),
        TagFieldType::LongBlockIndex => Ok(TagFieldData::LongBlockIndex(parse_block_index(input)?)),
        TagFieldType::CustomLongBlockIndex => Ok(TagFieldData::CustomLongBlockIndex(
            parse_block_index(input)?,
        )),

        TagFieldType::String => Ok(TagFieldData::String(input.to_string())),
        TagFieldType::LongString => Ok(TagFieldData::LongString(input.to_string())),

        TagFieldType::StringId => Ok(TagFieldData::StringId(StringIdData {
            string: input.to_string(),
        })),
        TagFieldType::OldStringId => Ok(TagFieldData::OldStringId(StringIdData {
            string: input.to_string(),
        })),

        TagFieldType::TagReference => Ok(TagFieldData::TagReference(parse_tag_reference(input)?)),

        TagFieldType::Data => {
            anyhow::bail!("setting 'data' fields from the CLI is not yet supported")
        }

        TagFieldType::Struct
        | TagFieldType::Block
        | TagFieldType::Array
        | TagFieldType::PageableResource => {
            anyhow::bail!("cannot set container field types directly")
        }

        TagFieldType::ApiInterop | TagFieldType::VertexBuffer => {
            anyhow::bail!("setting api_interop / vertex_buffer fields is not yet supported")
        }

        _ => anyhow::bail!(
            "setting field type '{:?}' from the CLI is not supported",
            field.field_type
        ),
    }
}

fn parse_enum_value(
    layout: &blam_tags::layout::TagLayout,
    field: &blam_tags::layout::TagFieldDefinition,
    input: &str,
) -> Result<i32> {
    // Try numeric first, then case-insensitive name lookup.
    if let Ok(n) = input.parse::<i32>() {
        return Ok(n);
    }
    if let Some(index) = find_enum_option_index(layout, field, input) {
        return Ok(index as i32);
    }
    anyhow::bail!("enum option '{}' not found", input)
}

fn parse_int(s: &str) -> Result<i64> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Ok(i64::from_str_radix(hex, 16).context("expected hex integer")?)
    } else {
        Ok(s.parse::<i64>().context("expected integer")?)
    }
}

fn parse_block_index(s: &str) -> Result<i32> {
    if s.eq_ignore_ascii_case("none") {
        return Ok(-1);
    }
    Ok(s.parse().context("expected integer or 'none'")?)
}

/// Parse a group-tag string (e.g. `"unit"` or `"mo  "`) into a
/// BE-packed `u32`, matching the on-disk representation. Thin
/// wrapper over [`lib_parse_group_tag`] that converts `None` into
/// an `anyhow` error.
fn parse_group_tag(s: &str) -> Result<u32> {
    lib_parse_group_tag(s).context("group tag must be 1..=4 ASCII chars")
}

fn parse_tag_reference(s: &str) -> Result<TagReferenceData> {
    if s.eq_ignore_ascii_case("none") || s.is_empty() {
        return Ok(TagReferenceData { group_tag_and_name: None });
    }
    let (group_str, path) = s.split_once(':').context(
        "tag reference format: GROUP:path (e.g. hlmt:objects/characters/elite), or 'none'",
    )?;
    let group_tag = parse_group_tag(group_str)?;
    Ok(TagReferenceData {
        group_tag_and_name: Some((group_tag, path.to_string())),
    })
}
