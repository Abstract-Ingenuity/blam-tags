//! Tag layout: the schema attached to every tag file. A layout describes
//! the structs, blocks, fields, and field types used to interpret the
//! tag's payload bytes. It lives in the `blay` chunk (wrapping the `tgly`
//! chunk for v2/v3; flat for v1).
//!
//! Everything here is *schema*, not instance data. Per-tag values live in
//! [`crate::data`], which dispatches on the [`TagFieldType`] resolved on
//! each [`TagFieldDefinition`] during layout read.

use std::error::Error;
use std::io::{Read, Seek, Write};

use crate::fields::TagFieldType;
use crate::io::*;

/// An `sz[]` entry: a named list of strings, represented as a slice
/// into [`TagLayout::string_offsets`]. Used for enum/flags value names.
#[derive(Debug)]
pub struct TagStringList {
    /// `name_offset`-style index into [`TagLayout::string_data`] — the
    /// display name of this list (e.g. an enum type name).
    pub offset: u32,
    /// Number of entries in this list.
    pub count: u32,
    /// Index into [`TagLayout::string_offsets`] of the first entry.
    /// Entries `[first .. first + count]` are this list's strings.
    pub first: u32,
}

/// An `arr!` entry: a fixed-count inline array of a struct. Array
/// elements have no wrapping `tgst` — their raw bytes live inline in
/// the parent struct's `raw_data`, and their sub-chunks flow inline
/// into the parent's `tgst` content.
#[derive(Debug)]
pub struct TagArrayDefinition {
    /// Offset into [`TagLayout::string_data`] of the array's name.
    pub name_offset: u32,
    /// Number of elements.
    pub count: u32,
    /// Index into [`TagLayout::struct_definitions`] of the element struct.
    pub struct_index: u32,
}

/// A `tgft` entry: a field-type registry record. Indexed by
/// [`TagFieldDefinition::type_index`].
#[derive(Debug)]
pub struct TagFieldTypeDefinition {
    /// Offset into [`TagLayout::string_data`] of the canonical type name
    /// (e.g. `"real point 3d"`), resolved at read time via
    /// [`TagFieldType::from_name`].
    pub name_offset: u32,
    /// Raw-data byte footprint of a field of this type. Used to advance
    /// the struct offset for primitives that don't need a sub-chunk.
    pub size: u32,
    /// Nonzero if fields of this type emit a sub-chunk inside their
    /// containing `tgst`. Used by [`TagLayout::get_struct_expected_children`]
    /// and as the bug-catching assertion in `read_sub_chunks`.
    pub needs_sub_chunk: u32,
}

/// A `gras` entry: one field within a struct definition. Serialized
/// form is 12 bytes (`name_offset` + `type_index` + `definition`); the
/// derived `field_type` and `offset` are computed at read time and are
/// not on the wire.
#[derive(Debug)]
pub struct TagFieldDefinition {
    /// Offset into [`TagLayout::string_data`] of the field name.
    pub name_offset: u32,
    /// Index into [`TagLayout::field_types`].
    pub type_index: u32,
    /// Type-specific "definition" payload. For struct/block/array/
    /// pageable_resource/api_interop fields, indexes the matching
    /// definition table; for pad/skip, the byte count; for enum/flags,
    /// the `string_lists` index.
    pub definition: u32,
    /// Dispatch-ready resolved type. Set once during layout read; not
    /// serialized.
    pub field_type: TagFieldType,
    /// Byte offset of this field's raw data within its containing struct. Set
    /// after the layout is fully parsed (after struct sizes are known).
    pub offset: u32,
}

/// A `blv2` entry (v2/v3) or half of a v1 `agro` record: names a block
/// whose elements are instances of a struct.
#[derive(Debug)]
pub struct TagBlockDefinition {
    /// Position in [`TagLayout::block_definitions`]. Tracked so
    /// [`crate::data::TagBlockData`] can remember which block it came from.
    pub index: u32,
    /// Offset into [`TagLayout::string_data`] of the block name.
    pub name_offset: u32,
    /// Element-count cap from the schema. Not enforced here — preserved
    /// for roundtrip.
    pub max_count: u32,
    /// Index into [`TagLayout::struct_definitions`] of the element struct.
    pub struct_index: u32,
}

/// An `rcv2` entry: declares a pageable-resource field's shape.
#[derive(Debug)]
pub struct TagResourceDefinition {
    /// Offset into [`TagLayout::string_data`] of the resource name.
    pub name_offset: u32,
    /// Unknown purpose; preserved verbatim.
    pub unknown: u32,
    /// Index into [`TagLayout::struct_definitions`] of the struct that
    /// wraps the resource payload when it's in
    /// [`crate::data::TagResourceChunk::Exploded`] form.
    pub struct_index: u32,
}

/// A `]==[` entry (v3 only): declares an api-interop field — an opaque
/// runtime-only pointer slot. Not parsed.
#[derive(Debug)]
pub struct TagInteropDefinition {
    pub name_offset: u32,
    pub struct_index: u32,
    /// Stable identifier for the interop type across versions.
    pub guid: [u8; 16],
}

/// An `stv2` entry (v2/v3) or half of a v1 `agro` record: names a
/// struct and points at its first field. Size is derived at read time
/// by [`TagLayout::compute_struct_layout`] walking fields until the
/// terminator.
#[derive(Debug)]
pub struct TagStructDefinition {
    /// Position in [`TagLayout::struct_definitions`]. Tracked so
    /// [`crate::data::TagStruct`] can remember which struct it came from.
    pub index: u32,
    /// Stable identifier for the struct type across layout versions.
    pub guid: [u8;16],
    /// Offset into [`TagLayout::string_data`] of the struct name.
    pub name_offset: u32,
    /// Index into [`TagLayout::fields`] of the first field in this
    /// struct; fields continue until a `Terminator`-typed field.
    pub first_field_index: u32,
    /// Derived field-packed size in bytes. Zero until
    /// [`TagLayout::compute_struct_layout`] runs. Not serialized.
    pub size: usize,
    /// Schema-version tag for the struct. Only present in V4 layouts
    /// (serialized as the trailing `u32` of an `stv4` record). Zero
    /// for V1/V2/V3 layouts.
    pub version: u32,
}

/// Counts at the top of the `blay` payload; each field here is the
/// length of the correspondingly named `Vec` on [`TagLayout`]. Which
/// fields are present depends on the block-layout version: v1 flattens
/// structs + blocks + fields into `aggregate_definition_count` records;
/// v2/v3 split them into the modern separate tables, and v3 adds
/// interop definitions.
#[derive(Debug)]
pub struct TagLayoutHeader {
    /// Index into [`TagLayout::block_definitions`] of the root (tag
    /// group) block. The tag's root `bdat` interprets its elements
    /// through this block's struct. v2/v3 only.
    pub tag_group_block_index: u32,
    pub string_data_size: u32,
    pub string_offset_count: u32,
    pub string_list_count: u32,
    pub custom_block_index_search_names_count: u32,
    pub data_definition_name_count: u32,
    pub array_definition_count: u32,
    pub field_type_count: u32,
    pub field_count: u32,
    /// v1 only. Each aggregate record is a flat (guid, name, max_count,
    /// first_field_index) tuple that the reader splits into a paired
    /// [`TagStructDefinition`] and [`TagBlockDefinition`].
    pub aggregate_definition_count: u32,
    /// v2/v3 only.
    pub struct_definition_count: u32,
    /// v2/v3 only.
    pub block_definition_count: u32,
    /// v2/v3 only.
    pub resource_definition_count: u32,
    /// v3 only.
    pub interop_definition_count: u32,
}

/// The full schema that interprets a tag's payload bytes. Lives in the
/// `blay` chunk's `tgly` body (v2/v3) or flat after the header (v1).
/// All name-bearing records reference [`Self::string_data`] by byte
/// offset; use [`Self::get_string`] to resolve.
#[derive(Debug)]
pub struct TagLayout {
    pub header: TagLayoutHeader,
    /// `str*` chunk — concatenated null-terminated UTF-8 strings. All
    /// `name_offset` fields elsewhere index into here.
    pub string_data: Vec<u8>,
    /// `sz+x` chunk — u32 offsets into `string_data`. Referenced by
    /// [`TagStringList::first`] to form enum/flags value lists.
    pub string_offsets: Vec<u32>,
    /// `sz[]` chunk — named string-list records.
    pub string_lists: Vec<TagStringList>,
    /// `csbn` chunk — offsets into `string_data` for custom
    /// block-index search names. Referenced by custom-block-index
    /// field `definition` values.
    pub custom_block_index_search_names_offsets: Vec<u32>,
    /// `dtnm` chunk — offsets into `string_data` for data-field
    /// type names (used to distinguish different `data` field flavors).
    pub data_definition_name_offsets: Vec<u32>,
    /// `arr!` chunk.
    pub array_definitions: Vec<TagArrayDefinition>,
    /// `tgft` chunk.
    pub field_types: Vec<TagFieldTypeDefinition>,
    /// `gras` chunk. Field definitions are stored flat; each struct's
    /// fields are the range `[first_field_index .. first_field_index +
    /// n]` where `n` is the count of fields up to and including the
    /// `Terminator`.
    pub fields: Vec<TagFieldDefinition>,
    /// `blv2` chunk (v2/v3) or reconstructed from v1 aggregate records.
    pub block_definitions: Vec<TagBlockDefinition>,
    /// `rcv2` chunk. Empty in v1.
    pub resource_definitions: Vec<TagResourceDefinition>,
    /// `]==[` chunk. Empty in v1/v2.
    pub interop_definitions: Vec<TagInteropDefinition>,
    /// `stv2` chunk (v2/v3) or reconstructed from v1 aggregate records.
    pub struct_definitions: Vec<TagStructDefinition>,
}

impl TagLayout {
    /// Resolve a `name_offset` into the UTF-8 string at that position
    /// in [`Self::string_data`] (the stored data is null-terminated).
    /// Returns `None` for an out-of-range offset.
    pub fn get_string(&self, offset: u32) -> Option<&str> {
        let start_offset = offset as usize;
        let mut end_offset = start_offset;

        if start_offset >= self.string_data.len() {
            return None;
        }

        while end_offset < self.string_data.len() {
            if self.string_data[end_offset] == 0 {
                break;
            }

            end_offset += 1;
        }

        Some(str::from_utf8(&self.string_data[start_offset..end_offset]).unwrap())
    }

    /// Number of direct child chunks a struct produces when serialized — i.e. how many
    /// of its fields have `needs_sub_chunk` (struct/block/tag_reference/string_id/data/
    /// pageable/api_interop). Matches `_calculate_structure_expected_children_by_entry`
    /// in Blam-Creation-Suite (tag_file_reader.cpp).
    pub fn get_struct_expected_children(&self, struct_index: usize) -> u32 {
        let mut count = 0;
        let mut field_index = self.struct_definitions[struct_index].first_field_index as usize;

        loop {
            let field = &self.fields[field_index];

            if field.field_type == TagFieldType::Terminator {
                return count;
            }

            let field_type = &self.field_types[field.type_index as usize];

            if field_type.needs_sub_chunk != 0 {
                count += 1;
            }

            field_index += 1;
        }
    }

    /// Compute [`TagStructDefinition::size`] and each
    /// [`TagFieldDefinition::offset`] for `struct_index` by walking its
    /// fields, recursing into nested struct/array fields first so their
    /// sizes are known before we accumulate. Idempotent — re-running on
    /// an already-laid-out struct is a no-op.
    pub fn compute_struct_layout(&mut self, struct_index: usize) {
        if self.struct_definitions[struct_index].size != 0 {
            return;
        }

        let mut size = 0;
        let mut field_index = self.struct_definitions[struct_index].first_field_index as usize;

        let mut done = false;

        while !done {
            // Record this field's START offset before accumulating its size.
            self.fields[field_index].offset = size as u32;

            let field = &self.fields[field_index];

            if field.field_type == TagFieldType::Terminator {
                done = true;
            } else if field.field_type == TagFieldType::Struct {
                let child_index = field.definition as usize;
                self.compute_struct_layout(child_index);
                size += self.struct_definitions[child_index].size;
            } else if field.field_type == TagFieldType::Array {
                let (array_struct_index, count) = {
                    let array_definition = &self.array_definitions[field.definition as usize];
                    (array_definition.struct_index as usize, array_definition.count)
                };
                self.compute_struct_layout(array_struct_index);
                size += self.struct_definitions[array_struct_index].size * count as usize;
            } else if field.field_type == TagFieldType::Pad || field.field_type == TagFieldType::Skip {
                size += field.definition as usize;
            } else {
                size += self.field_types[field.type_index as usize].size as usize;
            }

            field_index += 1;
        }

        self.struct_definitions[struct_index].size = size;
    }

    /// Parse a `tgly`-shaped (v2/v3) or flat (v1) layout body.
    ///
    /// `block_layout_version` is the payload version carried on the
    /// enclosing `blay` chunk (1, 2, or 3) and controls which header
    /// fields are present and whether the body is wrapped in a `tgly`
    /// chunk with per-section sub-chunk headers.
    ///
    /// After parsing all records, the reader resolves each field's
    /// [`TagFieldType`] and computes the size/offset of every struct
    /// so the data-layer parsing can dispatch cheaply.
    pub fn read<R: Seek + Read>(
        block_layout_version: u32,
        reader: &mut std::io::BufReader<R>,
    ) -> Result<Self, Box<dyn Error>> {
        let mut string_data;
        let mut string_offsets;
        let mut string_lists;
        let mut custom_block_index_search_names_offsets;
        let mut data_definition_name_offsets;
        let mut array_definitions;
        let mut field_types;
        let mut field_definitions;
        let mut block_definitions;
        let mut resource_definitions;
        let mut interop_definitions;
        let mut struct_definitions;

        //================================================================================
        // Read the tag layout header
        //================================================================================

        let header = TagLayoutHeader {
            tag_group_block_index: if matches!(block_layout_version, 2 | 3 | 4) { read_u32_le(reader)? } else { 0 },
            string_data_size: read_u32_le(reader)?,
            string_offset_count: read_u32_le(reader)?,
            string_list_count: read_u32_le(reader)?,
            custom_block_index_search_names_count: read_u32_le(reader)?,
            data_definition_name_count: read_u32_le(reader)?,
            array_definition_count: read_u32_le(reader)?,
            field_type_count: read_u32_le(reader)?,
            field_count: read_u32_le(reader)?,
            aggregate_definition_count: if block_layout_version == 1 { read_u32_le(reader)? } else { 0 },
            struct_definition_count: if matches!(block_layout_version, 2 | 3 | 4) { read_u32_le(reader)? } else { 0 },
            block_definition_count: if matches!(block_layout_version, 2 | 3 | 4) { read_u32_le(reader)? } else { 0 },
            resource_definition_count: if matches!(block_layout_version, 2 | 3 | 4) { read_u32_le(reader)? } else { 0 },
            interop_definition_count: if matches!(block_layout_version, 3 | 4) { read_u32_le(reader)? } else { 0 },
        };

        //================================================================================
        // Read the tag layout chunk header (if present)
        //================================================================================

        let tag_layout_header_and_offset = if block_layout_version > 1 {
            let tag_layout_header = read_tag_chunk_header(reader)?;
            assert!(tag_layout_header.signature == u32::from_be_bytes(*b"tgly"));
            // HYPOTHESIS: tgly.version equals the blay layout version (2, 3, or 4).
            assert_eq!(
                tag_layout_header.version, block_layout_version,
                "tgly version ({}) != block_layout_version ({})",
                tag_layout_header.version, block_layout_version,
            );
            Some((tag_layout_header, reader.stream_position()?))
        } else {
            None
        };

        //================================================================================
        // Read the string data
        //================================================================================

        if block_layout_version > 1 {
            let string_data_header = read_tag_chunk_header(reader)?;
            assert!(string_data_header.signature == u32::from_be_bytes(*b"str*"));
            // HYPOTHESIS: str* version is always 0.
            assert_eq!(string_data_header.version, 0, "str* version ({}) != 0", string_data_header.version);
            assert!(header.string_data_size == string_data_header.size);
        }

        string_data = vec![0u8; header.string_data_size as _];
        reader.read_exact(string_data.as_mut_slice())?;

        //================================================================================
        // Read the string offsets
        //================================================================================

        if block_layout_version > 1 {
            let string_offsets_header = read_tag_chunk_header(reader)?;
            assert!(string_offsets_header.signature == u32::from_be_bytes(*b"sz+x"));
            // HYPOTHESIS: sz+x version is always 0.
            assert_eq!(string_offsets_header.version, 0, "sz+x version ({}) != 0", string_offsets_header.version);
            assert!(header.string_offset_count as usize == string_offsets_header.size as usize / std::mem::size_of::<u32>());
        }

        string_offsets = vec![0; header.string_offset_count as usize];

        for i in 0..header.string_offset_count as usize {
            string_offsets[i] = read_u32_le(reader)?;
        }

        //================================================================================
        // Read the string lists
        //================================================================================

        if block_layout_version > 1 {
            let string_lists_header = read_tag_chunk_header(reader)?;
            assert!(string_lists_header.signature == u32::from_be_bytes(*b"sz[]"));
            // HYPOTHESIS: sz[] version is always 0.
            assert_eq!(string_lists_header.version, 0, "sz[] version ({}) != 0", string_lists_header.version);
            assert!(header.string_list_count as usize == string_lists_header.size as usize / 12);
        }

        string_lists = Vec::with_capacity(header.string_list_count as usize);

        for _ in 0..header.string_list_count {
            string_lists.push(TagStringList {
                offset: read_u32_le(reader)?,
                count: read_u32_le(reader)?,
                first: read_u32_le(reader)?,
            });
        }

        //================================================================================
        // Read the custom block index search names
        //================================================================================

        if block_layout_version > 1 {
            let custom_block_search_index_names_header = read_tag_chunk_header(reader)?;
            assert!(custom_block_search_index_names_header.signature == u32::from_be_bytes(*b"csbn"));
            // HYPOTHESIS: csbn version is always 0.
            assert_eq!(custom_block_search_index_names_header.version, 0, "csbn version ({}) != 0", custom_block_search_index_names_header.version);
            assert!(header.custom_block_index_search_names_count as usize == custom_block_search_index_names_header.size as usize / std::mem::size_of::<u32>());
        }

        custom_block_index_search_names_offsets = Vec::with_capacity(header.custom_block_index_search_names_count as usize);

        for _ in 0..header.custom_block_index_search_names_count {
            custom_block_index_search_names_offsets.push(read_u32_le(reader)?);
        }

        //================================================================================
        // Read the data definition names
        //================================================================================

        if block_layout_version > 1 {
            let data_definition_names_header = read_tag_chunk_header(reader)?;
            assert!(data_definition_names_header.signature == u32::from_be_bytes(*b"dtnm"));
            // HYPOTHESIS: dtnm version is always 0.
            assert_eq!(data_definition_names_header.version, 0, "dtnm version ({}) != 0", data_definition_names_header.version);
            assert!(header.data_definition_name_count as usize == data_definition_names_header.size as usize / std::mem::size_of::<u32>());
        }

        data_definition_name_offsets = vec![0; header.data_definition_name_count as usize];

        for i in 0..header.data_definition_name_count as usize {
            data_definition_name_offsets[i] = read_u32_le(reader)?;
        }

        //================================================================================
        // Read the array definitions
        //================================================================================

        if block_layout_version > 1 {
            let array_definitions_header = read_tag_chunk_header(reader)?;
            assert!(array_definitions_header.signature == u32::from_be_bytes(*b"arr!"));
            // HYPOTHESIS: arr! version is always 0.
            assert_eq!(array_definitions_header.version, 0, "arr! version ({}) != 0", array_definitions_header.version);
            assert!(header.array_definition_count as usize == array_definitions_header.size as usize / 12);
        }

        array_definitions = Vec::with_capacity(header.array_definition_count as usize);

        for _ in 0..header.array_definition_count {
            array_definitions.push(TagArrayDefinition {
                name_offset: read_u32_le(reader)?,
                count: read_u32_le(reader)?,
                struct_index: read_u32_le(reader)?,
            });
        }

        //================================================================================
        // Read the field types
        //================================================================================

        if block_layout_version > 1 {
            let field_types_header = read_tag_chunk_header(reader)?;
            assert!(field_types_header.signature == u32::from_be_bytes(*b"tgft"));
            // HYPOTHESIS: tgft version is always 0.
            assert_eq!(field_types_header.version, 0, "tgft version ({}) != 0", field_types_header.version);
            assert!(header.field_type_count as usize == field_types_header.size as usize / 12);
        }

        field_types = Vec::with_capacity(header.field_type_count as usize);

        for _ in 0..header.field_type_count {
            field_types.push(TagFieldTypeDefinition {
                name_offset: read_u32_le(reader)?,
                size: read_u32_le(reader)?,
                needs_sub_chunk: read_u32_le(reader)?,
            });
        }

        //================================================================================
        // Read the fields
        //================================================================================

        if block_layout_version > 1 {
            let fields_header = read_tag_chunk_header(reader)?;
            assert!(fields_header.signature == u32::from_be_bytes(*b"gras"));
            // HYPOTHESIS: gras version is always 0.
            assert_eq!(fields_header.version, 0, "gras version ({}) != 0", fields_header.version);
            assert!(header.field_count as usize == fields_header.size as usize / 12);
        }

        field_definitions = Vec::with_capacity(header.field_count as usize);

        for _ in 0..header.field_count {
            field_definitions.push(TagFieldDefinition {
                name_offset: read_u32_le(reader)?,
                type_index: read_u32_le(reader)?,
                definition: read_u32_le(reader)?,
                field_type: TagFieldType::Unknown,
                offset: 0,
            });
        }

        if block_layout_version == 1 {
            //================================================================================
            // Read the aggregate definitions
            //================================================================================

            block_definitions = Vec::with_capacity(header.aggregate_definition_count as usize);
            struct_definitions = Vec::with_capacity(header.aggregate_definition_count as usize);

            for i in 0..header.aggregate_definition_count {
                // Convert v1 agro records (28 bytes: guid[16] + name_offset + max_count + first_field_index)
                // into stv2 (24 bytes: guid[16] + name_offset + first_field_index)
                // and blv2 (12 bytes: name_offset + max_count + struct_index) format.
                let guid = read_u8_array(reader)?;
                let name_offset = read_u32_le(reader)?;
                let max_count = read_u32_le(reader)?;
                let first_field_index = read_u32_le(reader)?;

                struct_definitions.push(TagStructDefinition {
                    index: i,
                    guid,
                    name_offset,
                    first_field_index,
                    size: 0,
                    version: 0,
                });

                block_definitions.push(TagBlockDefinition {
                    index: i,
                    name_offset,
                    max_count,
                    struct_index: i as _,
                });
            }

            // Not present in V1:
            resource_definitions = vec![];
            interop_definitions = vec![];
        } else {
            assert!(matches!(block_layout_version, 2 | 3 | 4));

            //================================================================================
            // Read the block definitions
            //================================================================================

            let block_definition_header = read_tag_chunk_header(reader)?;

            assert!(block_definition_header.signature == u32::from_be_bytes(*b"blv2"));
            // HYPOTHESIS: blv2 version is always 0.
            assert_eq!(block_definition_header.version, 0, "blv2 version ({}) != 0", block_definition_header.version);
            assert!(header.block_definition_count as usize == block_definition_header.size as usize / 12);

            block_definitions = Vec::with_capacity(header.block_definition_count as usize);

            for i in 0..header.block_definition_count {
                block_definitions.push(TagBlockDefinition {
                    index: i,
                    name_offset: read_u32_le(reader)?,
                    max_count: read_u32_le(reader)?,
                    struct_index: read_u32_le(reader)?,
                });
            }

            //================================================================================
            // Read the resource definitions
            //================================================================================

            let resource_definitions_header = read_tag_chunk_header(reader)?;

            assert!(resource_definitions_header.signature == u32::from_be_bytes(*b"rcv2"));
            // HYPOTHESIS: rcv2 version is always 0.
            assert_eq!(resource_definitions_header.version, 0, "rcv2 version ({}) != 0", resource_definitions_header.version);
            assert!(header.resource_definition_count as usize == resource_definitions_header.size as usize / 12);

            resource_definitions = Vec::with_capacity(header.resource_definition_count as usize);

            for _ in 0..header.resource_definition_count {
                resource_definitions.push(TagResourceDefinition {
                    name_offset: read_u32_le(reader)?,
                    unknown: read_u32_le(reader)?,
                    struct_index: read_u32_le(reader)?,
                });
            }

            //================================================================================
            // Read the interop definitions (not present in V2; present in V3 and V4)
            //================================================================================

            interop_definitions = vec![];

            if matches!(block_layout_version, 3 | 4) {
                let interop_definitions_header = read_tag_chunk_header(reader)?;

                assert!(interop_definitions_header.signature == u32::from_be_bytes(*b"]==["));
                // HYPOTHESIS: ]==[ version is always 0.
                assert_eq!(interop_definitions_header.version, 0, "]==[ version ({}) != 0", interop_definitions_header.version);
                assert!(header.interop_definition_count as usize == interop_definitions_header.size as usize / 24);

                interop_definitions.reserve(header.interop_definition_count as usize);

                for _ in 0..header.interop_definition_count {
                    interop_definitions.push(TagInteropDefinition {
                        name_offset: read_u32_le(reader)?,
                        struct_index: read_u32_le(reader)?,
                        guid: read_u8_array(reader)?,
                    });
                }
            }

            //================================================================================
            // Read the struct definitions (stv2 for V2/V3, stv4 for V4).
            // stv4 adds a trailing `version: u32` per record → 28 bytes;
            // stv2 is 24 bytes.
            //================================================================================

            let struct_definitions_header = read_tag_chunk_header(reader)?;
            let (expected_struct_sig, struct_record_size) = if block_layout_version == 4 {
                (u32::from_be_bytes(*b"stv4"), 28usize)
            } else {
                (u32::from_be_bytes(*b"stv2"), 24usize)
            };

            assert!(struct_definitions_header.signature == expected_struct_sig);
            // HYPOTHESIS: stv2/stv4 version is always 0.
            assert_eq!(struct_definitions_header.version, 0, "stv2/stv4 version ({}) != 0", struct_definitions_header.version);
            assert!(header.struct_definition_count as usize == struct_definitions_header.size as usize / struct_record_size);

            struct_definitions = Vec::with_capacity(header.struct_definition_count as usize);

            for i in 0..header.struct_definition_count {
                let guid = read_u8_array(reader)?;
                let name_offset = read_u32_le(reader)?;
                let first_field_index = read_u32_le(reader)?;
                let version = if block_layout_version == 4 { read_u32_le(reader)? } else { 0 };
                struct_definitions.push(TagStructDefinition {
                    index: i,
                    guid,
                    name_offset,
                    first_field_index,
                    size: 0,
                    version,
                });
            }
        }

        //================================================================================
        // Finished reading the tag layout chunk
        //================================================================================

        if let Some((tag_layout_header, tag_layout_offset)) = tag_layout_header_and_offset {
            assert!(reader.stream_position()? == tag_layout_offset + tag_layout_header.size as u64);
        }

        let mut result = Self {
            header,
            string_data,
            string_offsets,
            string_lists,
            custom_block_index_search_names_offsets,
            data_definition_name_offsets,
            array_definitions,
            field_types,
            fields: field_definitions,
            block_definitions,
            struct_definitions,
            resource_definitions,
            interop_definitions,
        };

        //================================================================================
        // Resolve each field's type-name string into a TagFieldType enum once,
        // so the hot parse loops can dispatch on the enum instead of re-walking
        // string_data and comparing strings for every field read.
        //================================================================================

        for i in 0..result.fields.len() {
            let type_name_offset = result.field_types[result.fields[i].type_index as usize].name_offset;
            let name = result.get_string(type_name_offset).unwrap();
            result.fields[i].field_type = TagFieldType::from_name(name);
        }

        for i in 0..result.struct_definitions.len() {
            result.compute_struct_layout(i);
        }

        Ok(result)
    }

    /// Debug/pretty-print a block definition and its element struct,
    /// recursively. Writes to stdout with two-space indent per depth
    /// level. Intended for investigation, not production output.
    pub fn display_block(&self, block_index: usize, depth: usize) {
        let block = &self.block_definitions[block_index];

        let block_name = self.get_string(block.name_offset).unwrap();
        println!("block: {} (index {})", block_name, block_index);

        (0..depth + 1).for_each(|_| print!("  "));
        self.display_struct(block.struct_index as usize, depth + 1);
    }

    /// Debug/pretty-print a struct definition and all of its fields,
    /// recursing into nested struct/block/array/resource/interop fields.
    /// Writes to stdout; for investigation only.
    pub fn display_struct(&self, struct_index: usize, depth: usize) {
        let struct_definition = &self.struct_definitions[struct_index];
        let struct_name = self.get_string(struct_definition.name_offset).unwrap();
        println!("struct: {} (index {})", struct_name, struct_index);

        let mut field_offset = 0;

        for field in &self.fields[struct_definition.first_field_index as usize ..] {
            let field_type = &self.field_types[field.type_index as usize];
            let field_type_name = self.get_string(field_type.name_offset).unwrap();
            let field_name = self.get_string(field.name_offset).unwrap();

            match field.field_type {
                TagFieldType::Terminator => break,

                TagFieldType::Pad | TagFieldType::Skip => {
                    (0..depth + 1).for_each(|_| print!("  "));
                    println!("field: \"{}\" - \"{}\" - \"{}\" - offset 0x{:X}", field_name, field_type_name, field.definition, field_offset);
                    field_offset += field.definition;
                    continue;
                }

                TagFieldType::CharInteger | TagFieldType::ShortInteger | TagFieldType::LongInteger | TagFieldType::Int64Integer => {}

                TagFieldType::CharEnum | TagFieldType::ShortEnum | TagFieldType::LongEnum
                | TagFieldType::ByteFlags | TagFieldType::WordFlags | TagFieldType::LongFlags => {
                    let string_list = &self.string_lists[field.definition as usize];

                    (0..depth + 1).for_each(|_| print!("  "));
                    println!("field: \"{}\" - \"{}\" - \"{}\" - offset 0x{:X}:", field_name, field_type_name, self.get_string(string_list.offset).unwrap(), field_offset);

                    for &string_offset in &self.string_offsets[string_list.first as usize .. (string_list.first + string_list.count) as usize] {
                        let item = self.get_string(string_offset).unwrap();
                        (0..depth + 2).for_each(|_| print!("  "));
                        println!("\"{}\"", item);
                    }

                    field_offset += field_type.size;
                    continue;
                }

                TagFieldType::CharBlockIndex | TagFieldType::ShortBlockIndex | TagFieldType::LongBlockIndex => {}
                TagFieldType::CustomCharBlockIndex | TagFieldType::CustomShortBlockIndex | TagFieldType::CustomLongBlockIndex => {}

                TagFieldType::ByteBlockFlags | TagFieldType::WordBlockFlags | TagFieldType::LongBlockFlags => {}

                TagFieldType::String => {}
                TagFieldType::LongString => {}

                TagFieldType::Tag => {}

                TagFieldType::RgbColor => {}
                TagFieldType::ArgbColor => {}
                TagFieldType::Point2d => {}
                TagFieldType::Rectangle2d => {}

                TagFieldType::Angle => {}
                TagFieldType::AngleBounds => {}

                TagFieldType::Real => {}
                TagFieldType::RealBounds => {}

                TagFieldType::RealFraction => {}
                TagFieldType::FractionBounds => {}

                TagFieldType::ShortIntegerBounds => {}

                TagFieldType::RealPoint2d => {}
                TagFieldType::RealPoint3d => {}
                TagFieldType::RealVector2d => {}
                TagFieldType::RealVector3d => {}
                TagFieldType::RealEulerAngles2d => {}
                TagFieldType::RealEulerAngles3d => {}
                TagFieldType::RealPlane2d => {}
                TagFieldType::RealPlane3d => {}
                TagFieldType::RealQuaternion => {}
                TagFieldType::RealRgbColor => {}
                TagFieldType::RealArgbColor => {}
                TagFieldType::RealHsvColor => {}
                TagFieldType::RealAhsvColor => {}
                TagFieldType::RealSlider => {}

                TagFieldType::Custom => {}
                TagFieldType::TagReference => {}
                TagFieldType::StringId => {}
                TagFieldType::OldStringId => {}
                TagFieldType::Explanation => {}
                TagFieldType::UselessPad => {}

                TagFieldType::Struct => {
                    (0..depth + 1).for_each(|_| print!("  "));
                    print!("field: \"{}\" - offset 0x{:X} - ", field_name, field_offset);
                    self.display_struct(field.definition as usize, depth + 1);
                    let struct_definition = &self.struct_definitions[field.definition as usize];
                    field_offset += struct_definition.size as u32;
                    continue;
                }

                TagFieldType::Block => {
                    (0..depth + 1).for_each(|_| print!("  "));
                    print!("field: \"{}\" - offset 0x{:X} - ", field_name, field_offset);
                    self.display_block(field.definition as usize, depth + 1);
                    field_offset += field_type.size;
                    continue;
                }

                TagFieldType::Data => {}

                TagFieldType::VertexBuffer => {}

                TagFieldType::Array => {
                    let array_definition = &self.array_definitions[field.definition as usize];
                    (0..depth + 1).for_each(|_| print!("  "));
                    print!("field: \"{}\" - \"{}\" - \"{}\" - offset 0x{:X} - ", field_name, field_type_name, array_definition.count, field_offset);
                    self.display_struct(array_definition.struct_index as usize, depth + 1);
                    let struct_definition = &self.struct_definitions[array_definition.struct_index as usize];
                    field_offset += struct_definition.size as u32 * array_definition.count;
                    continue;
                }

                TagFieldType::PageableResource => {
                    let resource_definition = &self.resource_definitions[field.definition as usize];
                    (0..depth + 1).for_each(|_| print!("  "));
                    print!("field: \"{}\" - \"{}\" - offset 0x{:X} - ", field_name, field_type_name, field_offset);
                    self.display_struct(resource_definition.struct_index as usize, depth + 1);
                    field_offset += field_type.size;
                    continue;
                }

                TagFieldType::ApiInterop => {
                    let interop_definition = &self.interop_definitions[field.definition as usize];
                    (0..depth + 1).for_each(|_| print!("  "));
                    print!("field: \"{}\" - \"{}\" - offset 0x{:X} - ", field_name, field_type_name, field_offset);
                    self.display_struct(interop_definition.struct_index as usize, depth + 1);
                    field_offset += field_type.size;
                    continue;
                }

                _ => panic!("unhandled field type: \"{}\"", field_type_name),
            }

            (0..depth + 1).for_each(|_| print!("  "));
            println!("field: \"{}\" - \"{}\" - \"{}\" - offset 0x{:X}", field_name, field_type_name, field.definition, field_offset);
            field_offset += field_type.size;
        }
    }

    /// Write this layout. Mirrors `TagLayout::read`: the layout header fields
    /// are conditional on `block_layout_version`; v1 emits flat records
    /// directly (no `tgly` wrapper, no section chunk headers); v2/v3/v4
    /// emit a `tgly` chunk wrapping per-section chunks (`str*`, `sz+x`,
    /// `sz[]`, `csbn`, `dtnm`, `arr!`, `tgft`, `gras`, `blv2`, `rcv2`,
    /// optional `]==[`, and `stv2` for v2/v3 / `stv4` for v4). v1
    /// aggregate records are reconstructed 1:1 from the paired
    /// struct/block definitions.
    pub fn write<W: Write>(
        &self,
        block_layout_version: u32,
        writer: &mut W,
    ) -> std::io::Result<()> {
        //============================================================
        // Layout header
        //============================================================

        if matches!(block_layout_version, 2 | 3 | 4) {
            writer.write_all(&self.header.tag_group_block_index.to_le_bytes())?;
        }
        writer.write_all(&self.header.string_data_size.to_le_bytes())?;
        writer.write_all(&self.header.string_offset_count.to_le_bytes())?;
        writer.write_all(&self.header.string_list_count.to_le_bytes())?;
        writer.write_all(&self.header.custom_block_index_search_names_count.to_le_bytes())?;
        writer.write_all(&self.header.data_definition_name_count.to_le_bytes())?;
        writer.write_all(&self.header.array_definition_count.to_le_bytes())?;
        writer.write_all(&self.header.field_type_count.to_le_bytes())?;
        writer.write_all(&self.header.field_count.to_le_bytes())?;
        if block_layout_version == 1 {
            writer.write_all(&self.header.aggregate_definition_count.to_le_bytes())?;
        }
        if matches!(block_layout_version, 2 | 3 | 4) {
            writer.write_all(&self.header.struct_definition_count.to_le_bytes())?;
            writer.write_all(&self.header.block_definition_count.to_le_bytes())?;
            writer.write_all(&self.header.resource_definition_count.to_le_bytes())?;
        }
        if matches!(block_layout_version, 3 | 4) {
            writer.write_all(&self.header.interop_definition_count.to_le_bytes())?;
        }

        //============================================================
        // v1: flat records, no tgly wrapper, no section headers
        //============================================================

        if block_layout_version == 1 {
            self.write_layout_chunks(block_layout_version, writer)?;
            return Ok(());
        }

        //============================================================
        // v2/v3/v4: tgly chunk wrapping per-section chunks. Size is
        // precomputed so the body streams directly to `writer` without
        // buffering into a Vec. tgly.version == block_layout_version
        // (hypothesis-verified on read).
        //============================================================

        assert!(matches!(block_layout_version, 2 | 3 | 4));

        write_tag_chunk_header(
            writer,
            u32::from_be_bytes(*b"tgly"),
            block_layout_version,
            self.compute_layout_size(block_layout_version),
        )?;
        self.write_layout_chunks(block_layout_version, writer)?;

        Ok(())
    }

    /// Precompute the exact number of bytes `write_layout_chunks` will emit
    /// for a v2/v3/v4 layout, so the enclosing `tgly` chunk header can be
    /// written with the correct size without buffering the body. Each
    /// section is a 12-byte chunk header plus `count * chunk_size`. The
    /// struct-definitions section is 24 bytes/record in `stv2` (v2/v3)
    /// and 28 bytes/record in `stv4` (v4).
    fn compute_layout_size(&self, block_layout_version: u32) -> u32 {
        assert!(matches!(block_layout_version, 2 | 3 | 4));

        let section_size = |chunk_count: usize, chunk_size: usize| -> u32 {
            (12 + chunk_count * chunk_size) as u32
        };

        let struct_record_size = if block_layout_version == 4 { 28 } else { 24 };

        let mut size = 0u32;
        size += section_size(self.string_data.len(), 1);
        size += section_size(self.string_offsets.len(), size_of::<u32>());
        size += section_size(self.string_lists.len(), 12);
        size += section_size(self.custom_block_index_search_names_offsets.len(), size_of::<u32>());
        size += section_size(self.data_definition_name_offsets.len(), size_of::<u32>());
        size += section_size(self.array_definitions.len(), 12);
        size += section_size(self.field_types.len(), 12);
        size += section_size(self.fields.len(), 12);
        size += section_size(self.block_definitions.len(), 12);
        size += section_size(self.resource_definitions.len(), 12);
        if matches!(block_layout_version, 3 | 4) {
            size += section_size(self.interop_definitions.len(), 24);
        }
        size += section_size(self.struct_definitions.len(), struct_record_size);
        size
    }

    /// Write the body of the `tgly` chunk (v2/v3/v4) or the flat chunk
    /// stream (v1). Both forms emit the same logical chunk sequence;
    /// v2/v3/v4 wraps each chunk group in a chunk header, v1 emits raw.
    fn write_layout_chunks<W: Write>(
        &self,
        block_layout_version: u32,
        writer: &mut W,
    ) -> std::io::Result<()> {
        let wrap_sections = matches!(block_layout_version, 2 | 3 | 4);

        //------------------------------------------------------------
        // str*  — string data
        //------------------------------------------------------------
        if wrap_sections {
            write_tag_chunk_header(
                writer,
                u32::from_be_bytes(*b"str*"),
                0,
                self.string_data.len() as u32,
            )?;
        }
        writer.write_all(&self.string_data)?;

        //------------------------------------------------------------
        // sz+x  — string offsets (u32 each)
        //------------------------------------------------------------
        if wrap_sections {
            write_tag_chunk_header(
                writer,
                u32::from_be_bytes(*b"sz+x"),
                0,
                (self.string_offsets.len() * size_of::<u32>()) as u32,
            )?;
        }
        for string_offset in &self.string_offsets {
            writer.write_all(&string_offset.to_le_bytes())?;
        }

        //------------------------------------------------------------
        // sz[]  — string lists (12 bytes each: offset, count, first)
        //------------------------------------------------------------
        if wrap_sections {
            write_tag_chunk_header(
                writer,
                u32::from_be_bytes(*b"sz[]"),
                0,
                (self.string_lists.len() * 12) as u32,
            )?;
        }
        for string_list in &self.string_lists {
            writer.write_all(&string_list.offset.to_le_bytes())?;
            writer.write_all(&string_list.count.to_le_bytes())?;
            writer.write_all(&string_list.first.to_le_bytes())?;
        }

        //------------------------------------------------------------
        // csbn  — custom block index search name offsets (u32 each)
        //------------------------------------------------------------
        if wrap_sections {
            write_tag_chunk_header(
                writer,
                u32::from_be_bytes(*b"csbn"),
                0,
                (self.custom_block_index_search_names_offsets.len() * size_of::<u32>()) as u32,
            )?;
        }
        for name_offset in &self.custom_block_index_search_names_offsets {
            writer.write_all(&name_offset.to_le_bytes())?;
        }

        //------------------------------------------------------------
        // dtnm  — data definition name offsets (u32 each)
        //------------------------------------------------------------
        if wrap_sections {
            write_tag_chunk_header(
                writer,
                u32::from_be_bytes(*b"dtnm"),
                0,
                (self.data_definition_name_offsets.len() * size_of::<u32>()) as u32,
            )?;
        }
        for name_offset in &self.data_definition_name_offsets {
            writer.write_all(&name_offset.to_le_bytes())?;
        }

        //------------------------------------------------------------
        // arr!  — array definitions (12 bytes each)
        //------------------------------------------------------------
        if wrap_sections {
            write_tag_chunk_header(
                writer,
                u32::from_be_bytes(*b"arr!"),
                0,
                (self.array_definitions.len() * 12) as u32,
            )?;
        }
        for array_definition in &self.array_definitions {
            writer.write_all(&array_definition.name_offset.to_le_bytes())?;
            writer.write_all(&array_definition.count.to_le_bytes())?;
            writer.write_all(&array_definition.struct_index.to_le_bytes())?;
        }

        //------------------------------------------------------------
        // tgft  — field types (12 bytes each)
        //------------------------------------------------------------
        if wrap_sections {
            write_tag_chunk_header(
                writer,
                u32::from_be_bytes(*b"tgft"),
                0,
                (self.field_types.len() * 12) as u32,
            )?;
        }
        for field_type_definition in &self.field_types {
            writer.write_all(&field_type_definition.name_offset.to_le_bytes())?;
            writer.write_all(&field_type_definition.size.to_le_bytes())?;
            writer.write_all(&field_type_definition.needs_sub_chunk.to_le_bytes())?;
        }

        //------------------------------------------------------------
        // gras  — field definitions (12 bytes each: name_offset,
        // type_index, definition). The in-memory TagFieldDefinition also
        // carries derived `field_type` / `offset`, which are not
        // serialized — they're recomputed from the layout at read time.
        //------------------------------------------------------------
        if wrap_sections {
            write_tag_chunk_header(
                writer,
                u32::from_be_bytes(*b"gras"),
                0,
                (self.fields.len() * 12) as u32,
            )?;
        }
        for field_definition in &self.fields {
            writer.write_all(&field_definition.name_offset.to_le_bytes())?;
            writer.write_all(&field_definition.type_index.to_le_bytes())?;
            writer.write_all(&field_definition.definition.to_le_bytes())?;
        }

        //------------------------------------------------------------
        // v1 aggregate definitions: flat 28-byte records reconstructed
        // from paired struct/block definitions (1:1, by index).
        //------------------------------------------------------------
        if block_layout_version == 1 {
            assert_eq!(self.struct_definitions.len(), self.block_definitions.len());
            for i in 0..self.struct_definitions.len() {
                let struct_definition = &self.struct_definitions[i];
                let block_definition = &self.block_definitions[i];
                writer.write_all(&struct_definition.guid)?;
                writer.write_all(&struct_definition.name_offset.to_le_bytes())?;
                writer.write_all(&block_definition.max_count.to_le_bytes())?;
                writer.write_all(&struct_definition.first_field_index.to_le_bytes())?;
            }
            return Ok(());
        }

        //------------------------------------------------------------
        // blv2  — block definitions (12 bytes each)
        //------------------------------------------------------------
        write_tag_chunk_header(
            writer,
            u32::from_be_bytes(*b"blv2"),
            0,
            (self.block_definitions.len() * 12) as u32,
        )?;
        for block_definition in &self.block_definitions {
            writer.write_all(&block_definition.name_offset.to_le_bytes())?;
            writer.write_all(&block_definition.max_count.to_le_bytes())?;
            writer.write_all(&block_definition.struct_index.to_le_bytes())?;
        }

        //------------------------------------------------------------
        // rcv2  — resource definitions (12 bytes each)
        //------------------------------------------------------------
        write_tag_chunk_header(
            writer,
            u32::from_be_bytes(*b"rcv2"),
            0,
            (self.resource_definitions.len() * 12) as u32,
        )?;
        for resource_definition in &self.resource_definitions {
            writer.write_all(&resource_definition.name_offset.to_le_bytes())?;
            writer.write_all(&resource_definition.unknown.to_le_bytes())?;
            writer.write_all(&resource_definition.struct_index.to_le_bytes())?;
        }

        //------------------------------------------------------------
        // ]==[  — interop definitions (v3 and v4; 24 bytes each)
        //------------------------------------------------------------
        if matches!(block_layout_version, 3 | 4) {
            write_tag_chunk_header(
                writer,
                u32::from_be_bytes(*b"]==["),
                0,
                (self.interop_definitions.len() * 24) as u32,
            )?;
            for interop_definition in &self.interop_definitions {
                writer.write_all(&interop_definition.name_offset.to_le_bytes())?;
                writer.write_all(&interop_definition.struct_index.to_le_bytes())?;
                writer.write_all(&interop_definition.guid)?;
            }
        }

        //------------------------------------------------------------
        // stv2 / stv4  — struct definitions.
        // `size` is derived at read time via compute_struct_layout and
        // is not serialized. v4 adds a trailing `version: u32` per
        // record → 28 bytes/record; v2/v3 is 24.
        //------------------------------------------------------------
        let (struct_sig, struct_record_size) = if block_layout_version == 4 {
            (u32::from_be_bytes(*b"stv4"), 28usize)
        } else {
            (u32::from_be_bytes(*b"stv2"), 24usize)
        };
        write_tag_chunk_header(
            writer,
            struct_sig,
            0,
            (self.struct_definitions.len() * struct_record_size) as u32,
        )?;
        for struct_definition in &self.struct_definitions {
            writer.write_all(&struct_definition.guid)?;
            writer.write_all(&struct_definition.name_offset.to_le_bytes())?;
            writer.write_all(&struct_definition.first_field_index.to_le_bytes())?;
            if block_layout_version == 4 {
                writer.write_all(&struct_definition.version.to_le_bytes())?;
            }
        }

        Ok(())
    }
}

/// The `blay` chunk: a [`TagLayout`] plus its header metadata
/// (root data size, GUID, payload version). The outer `blay` chunk
/// header is always version 2, even when the inner `version` field
/// (1/2/3) differs — see [`TagBlockLayout::read`].
#[derive(Debug)]
pub struct TagBlockLayout {
    /// Raw-data size of the root struct (one element of the root
    /// block). Used as a schema-level sanity check.
    pub root_data_size: u32,
    /// Stable identifier for the tag group / root block type.
    pub guid: [u8; 16],
    /// The layout payload version (1, 2, or 3). Distinct from the
    /// `blay` chunk-header version. See [`TagLayout::read`].
    pub version: u32,
    pub layout: TagLayout,
}

impl TagBlockLayout {
    /// Read a `blay` chunk: its 24-byte payload header (root data
    /// size, guid, layout version) plus the wrapped [`TagLayout`].
    /// Asserts the outer `blay` chunk-header version is 2 and that
    /// the stream reaches exactly the chunk's declared end.
    pub fn read<R: Seek + Read>(reader: &mut std::io::BufReader<R>) -> Result<Self, Box<dyn Error>> {
        // Read the 'blay' chunk header
        let block_layout_header = read_tag_chunk_header(reader)?;
        assert!(block_layout_header.signature == u32::from_be_bytes(*b"blay"));

        let offset = reader.stream_position()?;

        // Read the block layout header data
        let root_data_size = read_u32_le(reader)?;
        let guid = read_u8_array(reader)?;
        let version = read_u32_le(reader)?;

        // HYPOTHESIS: the outer blay chunk-header version is always 2 (even
        // when the inner payload version — 1/2/3 — differs).
        assert_eq!(
            block_layout_header.version, 2,
            "blay chunk-header version ({}) != 2 (payload version = {})",
            block_layout_header.version, version,
        );

        let layout = TagLayout::read(version, reader)?;

        let end_offset = reader.stream_position()?;
        let expected_offset = offset + block_layout_header.size as u64;
        if end_offset != expected_offset {
            panic!("At offset 0x{end_offset:X}, expected 0x{expected_offset:X}");
        }

        Ok(Self {
            root_data_size,
            guid,
            version,
            layout,
        })
    }

    /// Write this block layout as a `blay` chunk. The payload is:
    /// `root_data_size (u32 LE) + guid[16] + version (u32 LE) + TagLayout
    /// body`. The outer `blay` chunk-header version is always 2
    /// (hypothesis-verified on read), even when the inner layout version
    /// is 1/2/3.
    pub fn write<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let mut body = Vec::new();
        body.extend_from_slice(&self.root_data_size.to_le_bytes());
        body.extend_from_slice(&self.guid);
        body.extend_from_slice(&self.version.to_le_bytes());
        self.layout.write(self.version, &mut body)?;

        write_tag_chunk_content(writer, u32::from_be_bytes(*b"blay"), 2, &body)?;
        Ok(())
    }
}
