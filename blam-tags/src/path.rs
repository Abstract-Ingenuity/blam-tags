//! Path-based navigation into a tag's data tree.
//!
//! A path is a `/`-separated sequence of segments. Each segment names
//! a field; sub-chunk-bearing fields (structs / blocks / arrays) are
//! transparently stepped into by intermediate segments, and blocks /
//! arrays accept an optional `[N]` suffix to select a specific
//! element. If a descent hits a block/array without an explicit
//! index, element 0 is chosen — matching the CLI's behavior.
//!
//! Segment grammar: `[Type:]name[\[index\]]`
//! - `Type:` — optional case-insensitive filter on the field-type
//!   name (e.g. `Block:regions`), used to disambiguate fields that
//!   share a name across types.
//! - `name` — case-sensitive field name, looked up against the
//!   containing struct's fields via `layout.get_string`.
//! - `[index]` — block / array element index. Ignored on the final
//!   segment (the caller decides what to do with it).
//!
//! Lookups return a [`FieldCursor`] / [`FieldCursorMut`] bundling the
//! enclosing block's `raw_data` slice, the containing
//! [`crate::data::TagStruct`], and the final field's index. Read
//! primitives via [`FieldCursor::parse`]; write via
//! [`FieldCursorMut::set`].

use crate::data::{TagBlockData, TagStruct, TagSubChunkContent};
use crate::fields::{deserialize_field, serialize_field, TagFieldData, TagFieldType};
use crate::layout::TagLayout;

/// Immutable cursor at a resolved field. `struct_raw` is the slice
/// from the enclosing block's `raw_data` covering exactly the
/// containing struct's bytes; `field_index` indexes into
/// `layout.fields`.
pub struct FieldCursor<'a> {
    pub struct_raw: &'a [u8],
    pub struct_data: &'a TagStruct,
    pub field_index: usize,
}

impl<'a> FieldCursor<'a> {
    /// Parse the field's value. See [`crate::data::TagStruct::parse_field`].
    pub fn parse(&self, layout: &TagLayout) -> Option<TagFieldData> {
        let field = &layout.fields[self.field_index];
        let sub_chunk = self
            .struct_data
            .sub_chunks
            .iter()
            .find(|entry| entry.field_index == Some(self.field_index as u32))
            .map(|entry| &entry.content);
        deserialize_field(layout, field, self.struct_raw, sub_chunk)
    }
}

/// Mutable cursor. Same layout as [`FieldCursor`] but with mutable
/// references, so [`FieldCursorMut::set`] can write to `struct_raw`
/// and swap sub-chunk contents in `struct_data`.
pub struct FieldCursorMut<'a> {
    pub struct_raw: &'a mut [u8],
    pub struct_data: &'a mut TagStruct,
    pub field_index: usize,
}

impl<'a> FieldCursorMut<'a> {
    /// Parse the field's current value. Same as
    /// [`FieldCursor::parse`] — provided on the mutable cursor so
    /// read-modify-write flows (e.g. flag toggling) don't need to
    /// re-resolve the path.
    pub fn parse(&self, layout: &TagLayout) -> Option<TagFieldData> {
        let field = &layout.fields[self.field_index];
        let sub_chunk = self
            .struct_data
            .sub_chunks
            .iter()
            .find(|entry| entry.field_index == Some(self.field_index as u32))
            .map(|entry| &entry.content);
        deserialize_field(layout, field, self.struct_raw, sub_chunk)
    }

    /// Write `value` back to this field. Primitive values mutate
    /// `struct_raw`; sub-chunk leaves swap the matching entry's
    /// content. See [`crate::data::TagStruct::set_field`].
    pub fn set(&mut self, layout: &TagLayout, value: TagFieldData) {
        let field = &layout.fields[self.field_index];
        if let Some(new_content) = serialize_field(field, &value, self.struct_raw) {
            let entry = self
                .struct_data
                .sub_chunks
                .iter_mut()
                .find(|entry| entry.field_index == Some(self.field_index as u32))
                .expect("FieldCursorMut::set: sub-chunk entry missing");
            entry.content = new_content;
        }
    }
}

/// Resolve `path` against `root_block` (typically
/// `tag.tag_stream.data`) and return a [`FieldCursor`] at the final
/// field. Returns `None` if any segment fails to resolve.
pub fn lookup<'a>(
    layout: &'a TagLayout,
    root_block: &'a TagBlockData,
    path: &str,
) -> Option<FieldCursor<'a>> {
    let segments: Vec<&str> = path.split('/').collect();
    let (final_segment, preceding) = segments.split_last()?;

    let root_element = root_block.elements.first()?;
    let root_struct_index = root_element.struct_index as usize;
    let root_size = layout.struct_definitions[root_struct_index].size;
    let mut current_raw: &[u8] = &root_block.raw_data[0..root_size];
    let mut current_struct: &TagStruct = root_element;

    for segment in preceding {
        let (type_filter, name, index) = parse_segment(segment);
        let field_index = find_field_in_struct(layout, current_struct, name, type_filter)?;
        let field = &layout.fields[field_index];

        match field.field_type {
            TagFieldType::Struct => {
                let nested_def = &layout.struct_definitions[field.definition as usize];
                let offset = field.offset as usize;
                current_raw = &current_raw[offset..offset + nested_def.size];
                current_struct = descend_struct(current_struct, field_index)?;
            }
            TagFieldType::Block => {
                let block = descend_block_data(current_struct, field_index)?;
                let block_def = &layout.block_definitions[field.definition as usize];
                let element_def = &layout.struct_definitions[block_def.struct_index as usize];
                let element_index = index.unwrap_or(0) as usize;
                let start = element_index * element_def.size;
                current_raw = &block.raw_data[start..start + element_def.size];
                current_struct = block.elements.get(element_index)?;
            }
            TagFieldType::Array => {
                let array_def = &layout.array_definitions[field.definition as usize];
                let element_def = &layout.struct_definitions[array_def.struct_index as usize];
                let element_index = index.unwrap_or(0) as usize;
                let start = field.offset as usize + element_index * element_def.size;
                current_raw = &current_raw[start..start + element_def.size];
                let elements = descend_array(current_struct, field_index)?;
                current_struct = elements.get(element_index)?;
            }
            _ => return None,
        }
    }

    let (type_filter, name, _index) = parse_segment(final_segment);
    let field_index = find_field_in_struct(layout, current_struct, name, type_filter)?;
    Some(FieldCursor {
        struct_raw: current_raw,
        struct_data: current_struct,
        field_index,
    })
}

/// Mutable counterpart to [`lookup`]. Descends through disjoint
/// field splits to maintain simultaneous `&mut` access to the
/// enclosing block's `raw_data` slice and the containing `TagStruct`.
pub fn lookup_mut<'a>(
    layout: &'a TagLayout,
    root_block: &'a mut TagBlockData,
    path: &str,
) -> Option<FieldCursorMut<'a>> {
    let segments: Vec<&str> = path.split('/').collect();
    let (final_segment, preceding) = segments.split_last()?;

    let root_struct_index = root_block.elements.first()?.struct_index as usize;
    let root_size = layout.struct_definitions[root_struct_index].size;

    let (mut current_raw, mut current_struct): (&mut [u8], &mut TagStruct) = {
        let raw = &mut root_block.raw_data[0..root_size];
        let s = root_block.elements.get_mut(0)?;
        (raw, s)
    };

    for segment in preceding {
        let (type_filter, name, index) = parse_segment(segment);
        let field_index = find_field_in_struct(layout, current_struct, name, type_filter)?;
        let field = &layout.fields[field_index];

        match field.field_type {
            TagFieldType::Struct => {
                let nested_def = &layout.struct_definitions[field.definition as usize];
                let offset = field.offset as usize;
                let size = nested_def.size;
                let new_raw = &mut current_raw[offset..offset + size];
                let new_struct = descend_struct_mut(current_struct, field_index)?;
                current_raw = new_raw;
                current_struct = new_struct;
            }
            TagFieldType::Block => {
                let block = descend_block_data_mut(current_struct, field_index)?;
                let block_def = &layout.block_definitions[field.definition as usize];
                let element_def = &layout.struct_definitions[block_def.struct_index as usize];
                let element_index = index.unwrap_or(0) as usize;
                let element_size = element_def.size;
                let start = element_index * element_size;
                // Split-borrow block into raw_data slice + element.
                let new_raw = &mut block.raw_data[start..start + element_size];
                let new_struct = block.elements.get_mut(element_index)?;
                current_raw = new_raw;
                current_struct = new_struct;
            }
            TagFieldType::Array => {
                let array_def = &layout.array_definitions[field.definition as usize];
                let element_def = &layout.struct_definitions[array_def.struct_index as usize];
                let element_index = index.unwrap_or(0) as usize;
                let offset = field.offset as usize + element_index * element_def.size;
                let size = element_def.size;
                let new_raw = &mut current_raw[offset..offset + size];
                let elements = descend_array_mut(current_struct, field_index)?;
                let new_struct = elements.get_mut(element_index)?;
                current_raw = new_raw;
                current_struct = new_struct;
            }
            _ => return None,
        }
    }

    let (type_filter, name, _index) = parse_segment(final_segment);
    let field_index = find_field_in_struct(layout, current_struct, name, type_filter)?;
    Some(FieldCursorMut {
        struct_raw: current_raw,
        struct_data: current_struct,
        field_index,
    })
}

//================================================================================
// Segment parsing
//================================================================================

fn parse_segment(segment: &str) -> (Option<&str>, &str, Option<u32>) {
    let (type_filter, rest) = match segment.find(':') {
        Some(colon) => (Some(&segment[..colon]), &segment[colon + 1..]),
        None => (None, segment),
    };

    let (name, index) = match rest.find('[') {
        Some(open) => {
            let close = rest[open..].find(']').map(|o| open + o).unwrap_or(rest.len());
            let index = rest[open + 1..close].parse::<u32>().ok();
            (&rest[..open], index)
        }
        None => (rest, None),
    };

    (type_filter, name, index)
}

fn find_field_in_struct(
    layout: &TagLayout,
    struct_data: &TagStruct,
    name: &str,
    type_filter: Option<&str>,
) -> Option<usize> {
    // No type filter → delegate to the by-name helper.
    if type_filter.is_none() {
        return struct_data.find_field_by_name(layout, name);
    }

    // Filtered walk: accept the first name match whose field-type
    // name matches the filter case-insensitively.
    let struct_definition = &layout.struct_definitions[struct_data.struct_index as usize];
    let mut field_index = struct_definition.first_field_index as usize;
    let filter = type_filter.unwrap();

    loop {
        let field = &layout.fields[field_index];
        if field.field_type == TagFieldType::Terminator {
            return None;
        }
        if layout.get_string(field.name_offset) == Some(name) {
            let type_name_offset = layout.field_types[field.type_index as usize].name_offset;
            let type_name = layout.get_string(type_name_offset).unwrap_or("");
            if type_name.eq_ignore_ascii_case(filter) {
                return Some(field_index);
            }
        }
        field_index += 1;
    }
}

fn descend_struct(struct_data: &TagStruct, field_index: usize) -> Option<&TagStruct> {
    let entry = struct_data
        .sub_chunks
        .iter()
        .find(|entry| entry.field_index == Some(field_index as u32))?;
    match &entry.content {
        TagSubChunkContent::Struct(nested) => Some(nested),
        _ => None,
    }
}

fn descend_struct_mut(struct_data: &mut TagStruct, field_index: usize) -> Option<&mut TagStruct> {
    let entry = struct_data
        .sub_chunks
        .iter_mut()
        .find(|entry| entry.field_index == Some(field_index as u32))?;
    match &mut entry.content {
        TagSubChunkContent::Struct(nested) => Some(nested),
        _ => None,
    }
}

fn descend_block_data(struct_data: &TagStruct, field_index: usize) -> Option<&TagBlockData> {
    let entry = struct_data
        .sub_chunks
        .iter()
        .find(|entry| entry.field_index == Some(field_index as u32))?;
    match &entry.content {
        TagSubChunkContent::Block(block) => Some(block),
        _ => None,
    }
}

fn descend_block_data_mut(
    struct_data: &mut TagStruct,
    field_index: usize,
) -> Option<&mut TagBlockData> {
    let entry = struct_data
        .sub_chunks
        .iter_mut()
        .find(|entry| entry.field_index == Some(field_index as u32))?;
    match &mut entry.content {
        TagSubChunkContent::Block(block) => Some(block),
        _ => None,
    }
}

fn descend_array<'a>(struct_data: &'a TagStruct, field_index: usize) -> Option<&'a [TagStruct]> {
    let entry = struct_data
        .sub_chunks
        .iter()
        .find(|entry| entry.field_index == Some(field_index as u32))?;
    match &entry.content {
        TagSubChunkContent::Array(elements) => Some(elements),
        _ => None,
    }
}

fn descend_array_mut<'a>(
    struct_data: &'a mut TagStruct,
    field_index: usize,
) -> Option<&'a mut [TagStruct]> {
    let entry = struct_data
        .sub_chunks
        .iter_mut()
        .find(|entry| entry.field_index == Some(field_index as u32))?;
    match &mut entry.content {
        TagSubChunkContent::Array(elements) => Some(elements),
        _ => None,
    }
}
