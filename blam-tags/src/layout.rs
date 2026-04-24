//! Tag layout: the schema attached to every tag file. A layout describes
//! the structs, blocks, fields, and field types used to interpret the
//! tag's payload bytes. It lives in the `blay` chunk (wrapping the `tgly`
//! chunk for v2/v3; flat for v1).
//!
//! Everything here is *schema*, not instance data. Per-tag values live in
//! [`crate::data`], which dispatches on the [`TagFieldType`] resolved on
//! each [`TagFieldLayout`] during layout read.

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
pub struct TagArrayLayout {
    /// Offset into [`TagLayout::string_data`] of the array's name.
    pub name_offset: u32,
    /// Number of elements.
    pub count: u32,
    /// Index into [`TagLayout::struct_layouts`] of the element struct.
    pub struct_index: u32,
}

/// A `tgft` entry: a field-type registry record. Indexed by
/// [`TagFieldLayout::type_index`].
#[derive(Debug)]
pub struct TagFieldTypeLayout {
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
pub struct TagFieldLayout {
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
pub struct TagBlockLayout {
    /// Position in [`TagLayout::block_layouts`]. Tracked so
    /// [`crate::data::TagBlockData`] can remember which block it came from.
    pub index: u32,
    /// Offset into [`TagLayout::string_data`] of the block name.
    pub name_offset: u32,
    /// Element-count cap from the schema. Not enforced here — preserved
    /// for roundtrip.
    pub max_count: u32,
    /// Index into [`TagLayout::struct_layouts`] of the element struct.
    pub struct_index: u32,
}

/// An `rcv2` entry: declares a pageable-resource field's shape.
#[derive(Debug)]
pub struct TagResourceLayout {
    /// Offset into [`TagLayout::string_data`] of the resource name.
    pub name_offset: u32,
    /// Unknown purpose; preserved verbatim.
    pub unknown: u32,
    /// Index into [`TagLayout::struct_layouts`] of the struct that
    /// wraps the resource payload when it's in
    /// [`crate::data::TagResourceChunk::Exploded`] form.
    pub struct_index: u32,
}

/// A `]==[` entry (v3 only): declares an api-interop field — an opaque
/// runtime-only pointer slot. Not parsed.
#[derive(Debug)]
pub struct TagInteropLayout {
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
pub struct TagStructLayout {
    /// Position in [`TagLayout::struct_layouts`]. Tracked so
    /// [`crate::data::TagStructData`] can remember which struct it came from.
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
/// structs + blocks + fields into `aggregate_layout_count` records;
/// v2/v3 split them into the modern separate tables, and v3 adds
/// interop definitions.
#[derive(Debug)]
pub struct TagLayoutHeader {
    /// Index into [`TagLayout::block_layouts`] of the root (tag
    /// group) block. The tag's root `bdat` interprets its elements
    /// through this block's struct. v2/v3 only.
    pub tag_group_block_index: u32,
    pub string_data_size: u32,
    pub string_offset_count: u32,
    pub string_list_count: u32,
    pub custom_block_index_search_names_count: u32,
    pub data_definition_name_count: u32,
    pub array_layout_count: u32,
    pub field_type_count: u32,
    pub field_count: u32,
    /// v1 only. Each aggregate record is a flat (guid, name, max_count,
    /// first_field_index) tuple that the reader splits into a paired
    /// [`TagStructLayout`] and [`TagBlockLayout`].
    pub aggregate_layout_count: u32,
    /// v2/v3 only.
    pub struct_layout_count: u32,
    /// v2/v3 only.
    pub block_layout_count: u32,
    /// v2/v3 only.
    pub resource_layout_count: u32,
    /// v3 only.
    pub interop_layout_count: u32,
}

/// The full `blay` chunk: a tag's schema plus its 24-byte payload
/// header (`root_data_size`, `guid`, `version`). The outer `blay`
/// chunk header is always version 2, even when the inner layout
/// payload `version` (1/2/3/4) differs.
///
/// All name-bearing records reference [`Self::string_data`] by byte
/// offset; use [`Self::get_string`] to resolve.
#[derive(Debug)]
pub struct TagLayout {
    /// Raw-data size of the root struct (one element of the root
    /// block). Schema-level sanity check; preserved for roundtrip.
    pub root_data_size: u32,
    /// Stable identifier for the tag group / root block type.
    pub guid: [u8; 16],
    /// Layout payload version (1, 2, 3, or 4). Distinct from the
    /// outer `blay` chunk-header version (always 2).
    pub version: u32,

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
    pub array_layouts: Vec<TagArrayLayout>,
    /// `tgft` chunk.
    pub field_types: Vec<TagFieldTypeLayout>,
    /// `gras` chunk. Field definitions are stored flat; each struct's
    /// fields are the range `[first_field_index .. first_field_index +
    /// n]` where `n` is the count of fields up to and including the
    /// `Terminator`.
    pub fields: Vec<TagFieldLayout>,
    /// `blv2` chunk (v2/v3) or reconstructed from v1 aggregate records.
    pub block_layouts: Vec<TagBlockLayout>,
    /// `rcv2` chunk. Empty in v1.
    pub resource_layouts: Vec<TagResourceLayout>,
    /// `]==[` chunk. Empty in v1/v2.
    pub interop_layouts: Vec<TagInteropLayout>,
    /// `stv2` chunk (v2/v3) or reconstructed from v1 aggregate records.
    pub struct_layouts: Vec<TagStructLayout>,
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
        let mut field_index = self.struct_layouts[struct_index].first_field_index as usize;

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

    /// Compute [`TagStructLayout::size`] and each
    /// [`TagFieldLayout::offset`] for `struct_index` by walking its
    /// fields, recursing into nested struct/array fields first so their
    /// sizes are known before we accumulate. Idempotent — re-running on
    /// an already-laid-out struct is a no-op.
    pub fn compute_struct_layout(&mut self, struct_index: usize) {
        if self.struct_layouts[struct_index].size != 0 {
            return;
        }

        let mut size = 0;
        let mut field_index = self.struct_layouts[struct_index].first_field_index as usize;

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
                size += self.struct_layouts[child_index].size;
            } else if field.field_type == TagFieldType::Array {
                let (array_struct_index, count) = {
                    let array_layout = &self.array_layouts[field.definition as usize];
                    (array_layout.struct_index as usize, array_layout.count)
                };
                self.compute_struct_layout(array_struct_index);
                size += self.struct_layouts[array_struct_index].size * count as usize;
            } else if field.field_type == TagFieldType::Pad || field.field_type == TagFieldType::Skip {
                size += field.definition as usize;
            } else if field.field_type == TagFieldType::Custom {
                // For `tmpl` customs the JSON importer stashes the
                // template's parent-chain expansion size into the
                // `definition` slot (non-zero means bytes to inline
                // here). Zero for all other custom subtypes (UI-only
                // hints like `hide`/`edih`/`vert`/etc.).
                size += field.definition as usize;
            } else {
                size += self.field_types[field.type_index as usize].size as usize;
            }

            field_index += 1;
        }

        self.struct_layouts[struct_index].size = size;
    }

    /// Parse a `blay` chunk: outer chunk header, 24-byte payload
    /// header (`root_data_size` + `guid[16]` + payload `version`),
    /// then the layout body. The payload `version` (1/2/3/4) controls
    /// which layout-header fields are present and whether the body is
    /// wrapped in a `tgly` chunk with per-section sub-chunk headers.
    ///
    /// After parsing all records, resolves each field's
    /// [`TagFieldType`] and computes the size/offset of every struct
    /// so the data-layer parsing can dispatch cheaply.
    pub fn read<R: Seek + Read>(
        reader: &mut std::io::BufReader<R>,
    ) -> Result<Self, Box<dyn Error>> {
        //================================================================================
        // Outer blay chunk header + 24-byte payload header
        //================================================================================

        let blay_header = read_tag_chunk_header(reader)?;
        assert!(blay_header.signature == u32::from_be_bytes(*b"blay"));

        let blay_offset = reader.stream_position()?;

        let root_data_size = read_u32_le(reader)?;
        let guid = read_u8_array(reader)?;
        let version = read_u32_le(reader)?;
        let block_layout_version = version;

        // HYPOTHESIS: the outer blay chunk-header version is always 2 (even
        // when the payload version — 1/2/3/4 — differs).
        assert_eq!(
            blay_header.version, 2,
            "blay chunk-header version ({}) != 2 (payload version = {})",
            blay_header.version, version,
        );

        let mut string_data;
        let mut string_offsets;
        let mut string_lists;
        let mut custom_block_index_search_names_offsets;
        let mut data_definition_name_offsets;
        let mut array_layouts;
        let mut field_types;
        let mut field_layouts;
        let mut block_layouts;
        let mut resource_layouts;
        let mut interop_layouts;
        let mut struct_layouts;

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
            array_layout_count: read_u32_le(reader)?,
            field_type_count: read_u32_le(reader)?,
            field_count: read_u32_le(reader)?,
            aggregate_layout_count: if block_layout_version == 1 { read_u32_le(reader)? } else { 0 },
            struct_layout_count: if matches!(block_layout_version, 2 | 3 | 4) { read_u32_le(reader)? } else { 0 },
            block_layout_count: if matches!(block_layout_version, 2 | 3 | 4) { read_u32_le(reader)? } else { 0 },
            resource_layout_count: if matches!(block_layout_version, 2 | 3 | 4) { read_u32_le(reader)? } else { 0 },
            interop_layout_count: if matches!(block_layout_version, 3 | 4) { read_u32_le(reader)? } else { 0 },
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
            let array_layouts_header = read_tag_chunk_header(reader)?;
            assert!(array_layouts_header.signature == u32::from_be_bytes(*b"arr!"));
            // HYPOTHESIS: arr! version is always 0.
            assert_eq!(array_layouts_header.version, 0, "arr! version ({}) != 0", array_layouts_header.version);
            assert!(header.array_layout_count as usize == array_layouts_header.size as usize / 12);
        }

        array_layouts = Vec::with_capacity(header.array_layout_count as usize);

        for _ in 0..header.array_layout_count {
            array_layouts.push(TagArrayLayout {
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
            field_types.push(TagFieldTypeLayout {
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

        field_layouts = Vec::with_capacity(header.field_count as usize);

        for _ in 0..header.field_count {
            field_layouts.push(TagFieldLayout {
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

            block_layouts = Vec::with_capacity(header.aggregate_layout_count as usize);
            struct_layouts = Vec::with_capacity(header.aggregate_layout_count as usize);

            for i in 0..header.aggregate_layout_count {
                // Convert v1 agro records (28 bytes: guid[16] + name_offset + max_count + first_field_index)
                // into stv2 (24 bytes: guid[16] + name_offset + first_field_index)
                // and blv2 (12 bytes: name_offset + max_count + struct_index) format.
                let guid = read_u8_array(reader)?;
                let name_offset = read_u32_le(reader)?;
                let max_count = read_u32_le(reader)?;
                let first_field_index = read_u32_le(reader)?;

                struct_layouts.push(TagStructLayout {
                    index: i,
                    guid,
                    name_offset,
                    first_field_index,
                    size: 0,
                    version: 0,
                });

                block_layouts.push(TagBlockLayout {
                    index: i,
                    name_offset,
                    max_count,
                    struct_index: i as _,
                });
            }

            // Not present in V1:
            resource_layouts = vec![];
            interop_layouts = vec![];
        } else {
            assert!(matches!(block_layout_version, 2 | 3 | 4));

            //================================================================================
            // Read the block definitions
            //================================================================================

            let block_layout_header = read_tag_chunk_header(reader)?;

            assert!(block_layout_header.signature == u32::from_be_bytes(*b"blv2"));
            // HYPOTHESIS: blv2 version is always 0.
            assert_eq!(block_layout_header.version, 0, "blv2 version ({}) != 0", block_layout_header.version);
            assert!(header.block_layout_count as usize == block_layout_header.size as usize / 12);

            block_layouts = Vec::with_capacity(header.block_layout_count as usize);

            for i in 0..header.block_layout_count {
                block_layouts.push(TagBlockLayout {
                    index: i,
                    name_offset: read_u32_le(reader)?,
                    max_count: read_u32_le(reader)?,
                    struct_index: read_u32_le(reader)?,
                });
            }

            //================================================================================
            // Read the resource definitions
            //================================================================================

            let resource_layouts_header = read_tag_chunk_header(reader)?;

            assert!(resource_layouts_header.signature == u32::from_be_bytes(*b"rcv2"));
            // HYPOTHESIS: rcv2 version is always 0.
            assert_eq!(resource_layouts_header.version, 0, "rcv2 version ({}) != 0", resource_layouts_header.version);
            assert!(header.resource_layout_count as usize == resource_layouts_header.size as usize / 12);

            resource_layouts = Vec::with_capacity(header.resource_layout_count as usize);

            for _ in 0..header.resource_layout_count {
                resource_layouts.push(TagResourceLayout {
                    name_offset: read_u32_le(reader)?,
                    unknown: read_u32_le(reader)?,
                    struct_index: read_u32_le(reader)?,
                });
            }

            //================================================================================
            // Read the interop definitions (not present in V2; present in V3 and V4)
            //================================================================================

            interop_layouts = vec![];

            if matches!(block_layout_version, 3 | 4) {
                let interop_layouts_header = read_tag_chunk_header(reader)?;

                assert!(interop_layouts_header.signature == u32::from_be_bytes(*b"]==["));
                // HYPOTHESIS: ]==[ version is always 0.
                assert_eq!(interop_layouts_header.version, 0, "]==[ version ({}) != 0", interop_layouts_header.version);
                assert!(header.interop_layout_count as usize == interop_layouts_header.size as usize / 24);

                interop_layouts.reserve(header.interop_layout_count as usize);

                for _ in 0..header.interop_layout_count {
                    interop_layouts.push(TagInteropLayout {
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

            let struct_layouts_header = read_tag_chunk_header(reader)?;
            let (expected_struct_sig, struct_record_size) = if block_layout_version == 4 {
                (u32::from_be_bytes(*b"stv4"), 28usize)
            } else {
                (u32::from_be_bytes(*b"stv2"), 24usize)
            };

            assert!(struct_layouts_header.signature == expected_struct_sig);
            // HYPOTHESIS: stv2/stv4 version is always 0.
            assert_eq!(struct_layouts_header.version, 0, "stv2/stv4 version ({}) != 0", struct_layouts_header.version);
            assert!(header.struct_layout_count as usize == struct_layouts_header.size as usize / struct_record_size);

            struct_layouts = Vec::with_capacity(header.struct_layout_count as usize);

            for i in 0..header.struct_layout_count {
                let guid = read_u8_array(reader)?;
                let name_offset = read_u32_le(reader)?;
                let first_field_index = read_u32_le(reader)?;
                let version = if block_layout_version == 4 { read_u32_le(reader)? } else { 0 };
                struct_layouts.push(TagStructLayout {
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

        // Outer blay chunk's end must match its declared size.
        let blay_end = reader.stream_position()?;
        let blay_expected_end = blay_offset + blay_header.size as u64;
        if blay_end != blay_expected_end {
            panic!(
                "blay chunk: ended at 0x{blay_end:X}, expected 0x{blay_expected_end:X}",
            );
        }

        let mut result = Self {
            root_data_size,
            guid,
            version,
            header,
            string_data,
            string_offsets,
            string_lists,
            custom_block_index_search_names_offsets,
            data_definition_name_offsets,
            array_layouts,
            field_types,
            fields: field_layouts,
            block_layouts,
            struct_layouts,
            resource_layouts,
            interop_layouts,
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

        for i in 0..result.struct_layouts.len() {
            result.compute_struct_layout(i);
        }

        Ok(result)
    }

    /// Debug/pretty-print a block definition and its element struct,
    /// recursively. Writes to stdout with two-space indent per depth
    /// level. Intended for investigation, not production output.
    pub fn display_block(&self, block_index: usize, depth: usize) {
        let block = &self.block_layouts[block_index];

        let block_name = self.get_string(block.name_offset).unwrap();
        println!("block: {} (index {})", block_name, block_index);

        (0..depth + 1).for_each(|_| print!("  "));
        self.display_struct(block.struct_index as usize, depth + 1);
    }

    /// Debug/pretty-print a struct definition and all of its fields,
    /// recursing into nested struct/block/array/resource/interop fields.
    /// Writes to stdout; for investigation only.
    pub fn display_struct(&self, struct_index: usize, depth: usize) {
        let struct_layout = &self.struct_layouts[struct_index];
        let struct_name = self.get_string(struct_layout.name_offset).unwrap();
        println!("struct: {} (index {})", struct_name, struct_index);

        let mut field_offset = 0;

        for field in &self.fields[struct_layout.first_field_index as usize ..] {
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
                    let struct_layout = &self.struct_layouts[field.definition as usize];
                    field_offset += struct_layout.size as u32;
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
                    let array_layout = &self.array_layouts[field.definition as usize];
                    (0..depth + 1).for_each(|_| print!("  "));
                    print!("field: \"{}\" - \"{}\" - \"{}\" - offset 0x{:X} - ", field_name, field_type_name, array_layout.count, field_offset);
                    self.display_struct(array_layout.struct_index as usize, depth + 1);
                    let struct_layout = &self.struct_layouts[array_layout.struct_index as usize];
                    field_offset += struct_layout.size as u32 * array_layout.count;
                    continue;
                }

                TagFieldType::PageableResource => {
                    let resource_layout = &self.resource_layouts[field.definition as usize];
                    (0..depth + 1).for_each(|_| print!("  "));
                    print!("field: \"{}\" - \"{}\" - offset 0x{:X} - ", field_name, field_type_name, field_offset);
                    self.display_struct(resource_layout.struct_index as usize, depth + 1);
                    field_offset += field_type.size;
                    continue;
                }

                TagFieldType::ApiInterop => {
                    let interop_layout = &self.interop_layouts[field.definition as usize];
                    (0..depth + 1).for_each(|_| print!("  "));
                    print!("field: \"{}\" - \"{}\" - offset 0x{:X} - ", field_name, field_type_name, field_offset);
                    self.display_struct(interop_layout.struct_index as usize, depth + 1);
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

    /// Write this layout as a `blay` chunk. Mirrors [`TagLayout::read`]:
    /// outer `blay` chunk header (always version 2), 24-byte payload
    /// header (root data size, guid, layout version), then the layout
    /// body. The body shape depends on the payload version (`self.version`):
    /// v1 emits flat records directly; v2/v3/v4 wrap them in a `tgly`
    /// chunk with per-section sub-chunks (`str*`, `sz+x`, `sz[]`,
    /// `csbn`, `dtnm`, `arr!`, `tgft`, `gras`, `blv2`, `rcv2`, optional
    /// `]==[`, and `stv2` for v2/v3 / `stv4` for v4).
    pub fn write<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        // Buffer the blay body into a Vec so we can emit the outer
        // chunk header with the correct size without pre-computing.
        let mut body = Vec::new();
        body.extend_from_slice(&self.root_data_size.to_le_bytes());
        body.extend_from_slice(&self.guid);
        body.extend_from_slice(&self.version.to_le_bytes());

        self.write_body(&mut body)?;

        write_tag_chunk_content(writer, u32::from_be_bytes(*b"blay"), 2, &body)
    }

    /// Write the layout body (everything after the 24-byte payload
    /// header). Private — [`TagLayout::write`] owns the outer chunk
    /// wrapping. Uses `self.version` to drive the conditional shape.
    fn write_body<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let block_layout_version = self.version;

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
        writer.write_all(&self.header.array_layout_count.to_le_bytes())?;
        writer.write_all(&self.header.field_type_count.to_le_bytes())?;
        writer.write_all(&self.header.field_count.to_le_bytes())?;
        if block_layout_version == 1 {
            writer.write_all(&self.header.aggregate_layout_count.to_le_bytes())?;
        }
        if matches!(block_layout_version, 2 | 3 | 4) {
            writer.write_all(&self.header.struct_layout_count.to_le_bytes())?;
            writer.write_all(&self.header.block_layout_count.to_le_bytes())?;
            writer.write_all(&self.header.resource_layout_count.to_le_bytes())?;
        }
        if matches!(block_layout_version, 3 | 4) {
            writer.write_all(&self.header.interop_layout_count.to_le_bytes())?;
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
        size += section_size(self.array_layouts.len(), 12);
        size += section_size(self.field_types.len(), 12);
        size += section_size(self.fields.len(), 12);
        size += section_size(self.block_layouts.len(), 12);
        size += section_size(self.resource_layouts.len(), 12);
        if matches!(block_layout_version, 3 | 4) {
            size += section_size(self.interop_layouts.len(), 24);
        }
        size += section_size(self.struct_layouts.len(), struct_record_size);
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
                (self.array_layouts.len() * 12) as u32,
            )?;
        }
        for array_layout in &self.array_layouts {
            writer.write_all(&array_layout.name_offset.to_le_bytes())?;
            writer.write_all(&array_layout.count.to_le_bytes())?;
            writer.write_all(&array_layout.struct_index.to_le_bytes())?;
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
        for field_type_layout in &self.field_types {
            writer.write_all(&field_type_layout.name_offset.to_le_bytes())?;
            writer.write_all(&field_type_layout.size.to_le_bytes())?;
            writer.write_all(&field_type_layout.needs_sub_chunk.to_le_bytes())?;
        }

        //------------------------------------------------------------
        // gras  — field definitions (12 bytes each: name_offset,
        // type_index, definition). The in-memory TagFieldLayout also
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
        for field_layout in &self.fields {
            writer.write_all(&field_layout.name_offset.to_le_bytes())?;
            writer.write_all(&field_layout.type_index.to_le_bytes())?;
            writer.write_all(&field_layout.definition.to_le_bytes())?;
        }

        //------------------------------------------------------------
        // v1 aggregate definitions: flat 28-byte records reconstructed
        // from paired struct/block definitions (1:1, by index).
        //------------------------------------------------------------
        if block_layout_version == 1 {
            assert_eq!(self.struct_layouts.len(), self.block_layouts.len());
            for i in 0..self.struct_layouts.len() {
                let struct_layout = &self.struct_layouts[i];
                let block_layout = &self.block_layouts[i];
                writer.write_all(&struct_layout.guid)?;
                writer.write_all(&struct_layout.name_offset.to_le_bytes())?;
                writer.write_all(&block_layout.max_count.to_le_bytes())?;
                writer.write_all(&struct_layout.first_field_index.to_le_bytes())?;
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
            (self.block_layouts.len() * 12) as u32,
        )?;
        for block_layout in &self.block_layouts {
            writer.write_all(&block_layout.name_offset.to_le_bytes())?;
            writer.write_all(&block_layout.max_count.to_le_bytes())?;
            writer.write_all(&block_layout.struct_index.to_le_bytes())?;
        }

        //------------------------------------------------------------
        // rcv2  — resource definitions (12 bytes each)
        //------------------------------------------------------------
        write_tag_chunk_header(
            writer,
            u32::from_be_bytes(*b"rcv2"),
            0,
            (self.resource_layouts.len() * 12) as u32,
        )?;
        for resource_layout in &self.resource_layouts {
            writer.write_all(&resource_layout.name_offset.to_le_bytes())?;
            writer.write_all(&resource_layout.unknown.to_le_bytes())?;
            writer.write_all(&resource_layout.struct_index.to_le_bytes())?;
        }

        //------------------------------------------------------------
        // ]==[  — interop definitions (v3 and v4; 24 bytes each)
        //------------------------------------------------------------
        if matches!(block_layout_version, 3 | 4) {
            write_tag_chunk_header(
                writer,
                u32::from_be_bytes(*b"]==["),
                0,
                (self.interop_layouts.len() * 24) as u32,
            )?;
            for interop_layout in &self.interop_layouts {
                writer.write_all(&interop_layout.name_offset.to_le_bytes())?;
                writer.write_all(&interop_layout.struct_index.to_le_bytes())?;
                writer.write_all(&interop_layout.guid)?;
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
            (self.struct_layouts.len() * struct_record_size) as u32,
        )?;
        for struct_layout in &self.struct_layouts {
            writer.write_all(&struct_layout.guid)?;
            writer.write_all(&struct_layout.name_offset.to_le_bytes())?;
            writer.write_all(&struct_layout.first_field_index.to_le_bytes())?;
            if block_layout_version == 4 {
                writer.write_all(&struct_layout.version.to_le_bytes())?;
            }
        }

        Ok(())
    }
}

//================================================================================
// JSON schema import
//
// Parses the per-group JSON dumped by
// `h3_guerilla_dump_tag_definitions_json.py` into a TagLayout that
// matches what `TagLayout::read` would produce from an equivalent blay
// chunk. The JSON's shape:
//
// - Group metadata (name, tag, parent_tag, version, flags) + a
//   `block` name that points at the root block.
// - Named registries: `blocks`, `structs`, `arrays`, `enums_flags`,
//   `datas`, `resources`, `interops`. Each map key is a definition
//   name; each value is the body (no redundant "name" key).
// - Fields' `definition` is either a name string into one of the
//   registries (for struct/block/array/flags/enum/data/etc.), an
//   integer byte-count (for pad/skip/useless_pad), a text string
//   (for explanation), or an object `{flags, allowed}` (for
//   tag_reference).
//
// The importer walks the registries, assigns stable indices per kind
// (alphabetical via BTreeMap for determinism), builds the string_data
// table dedup'd, resolves name references to indices, populates every
// TagLayout table, and finally runs `compute_struct_layout` so every
// struct has its size + per-field offsets set. As a sanity check,
// each computed struct size is compared against the JSON's dumped
// `size` field — mismatches bubble up as errors rather than silently
// producing a broken layout.
//================================================================================

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug)]
pub enum FromJsonError {
    Io(std::io::Error),
    Json(serde_json::Error),
    UnknownReference { kind: &'static str, name: String },
    BadFieldDefinition { field: String, ty: String },
    UnknownFieldType(String),
    BadGuid(String),
    BadGroupTag(String),
    StructSizeMismatch { name: String, schema: u32, computed: usize },
}

impl std::fmt::Display for FromJsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error reading schema: {e}"),
            Self::Json(e) => write!(f, "JSON parse error: {e}"),
            Self::UnknownReference { kind, name } => {
                write!(f, "schema references unknown {kind} {name:?}")
            }
            Self::BadFieldDefinition { field, ty } => {
                write!(f, "field {field:?} of type {ty:?} has invalid definition value")
            }
            Self::UnknownFieldType(s) => write!(f, "unknown field type {s:?}"),
            Self::BadGuid(s) => write!(f, "invalid guid {s:?} (expected 32 hex chars)"),
            Self::BadGroupTag(s) => write!(f, "invalid group tag {s:?} (expected 4 chars)"),
            Self::StructSizeMismatch { name, schema, computed } => write!(
                f,
                "computed size mismatch for struct {name:?}: schema says {schema}, computed {computed}"
            ),
        }
    }
}

impl std::error::Error for FromJsonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for FromJsonError {
    fn from(e: std::io::Error) -> Self { Self::Io(e) }
}
impl From<serde_json::Error> for FromJsonError {
    fn from(e: serde_json::Error) -> Self { Self::Json(e) }
}

//
// Serde shapes for the JSON schema files the dumper produces.
// Names match the library's Tag* convention + a Schema suffix.
//

#[derive(Debug, Deserialize)]
struct TagGroupSchema {
    tag: String,
    #[serde(default)] parent_tag: Option<String>,
    version: u32,
    flags: u32,
    block: String,
    #[serde(default)] blocks: BTreeMap<String, TagBlockSchema>,
    #[serde(default)] structs: BTreeMap<String, TagStructSchema>,
    #[serde(default)] arrays: BTreeMap<String, TagArraySchema>,
    #[serde(default)] enums_flags: BTreeMap<String, TagEnumSchema>,
    #[serde(default)] datas: BTreeMap<String, TagDataSchema>,
    #[serde(default)] resources: BTreeMap<String, PageableResourceSchema>,
    #[serde(default)] interops: BTreeMap<String, ApiInteropSchema>,
}

#[derive(Debug, Deserialize)]
struct TagBlockSchema {
    max_count: u32,
    #[serde(rename = "struct")] struct_name: String,
}

#[derive(Debug, Deserialize)]
struct TagStructSchema {
    guid: String,
    size: u32,
    fields: Vec<TagFieldSchema>,
}

#[derive(Debug, Deserialize)]
struct TagFieldSchema {
    #[serde(rename = "type")] ty: String,
    #[serde(default)] name: Option<String>,
    #[serde(default)] definition: serde_json::Value,
    #[serde(default)] group_tag: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TagArraySchema {
    count: u32,
    #[serde(rename = "struct")] struct_name: String,
}

#[derive(Debug, Deserialize)]
struct TagEnumSchema {
    options: Vec<Option<String>>,
}

#[derive(Debug, Deserialize)]
struct TagDataSchema {}

#[derive(Debug, Deserialize)]
struct PageableResourceSchema {
    flags: u64,
    #[serde(rename = "struct")] struct_name: String,
}

#[derive(Debug, Deserialize)]
struct ApiInteropSchema {
    guid: String,
    #[serde(rename = "struct")] struct_name: String,
}

//
// Field-type metadata: canonical on-wire name, byte size, whether the
// type emits a sub-chunk. Each JSON field's `type` string (snake_case)
// maps to one of these rows; the (size, needs_sub_chunk) values match
// what the engine packs into each blay's `tgft` registry.
//
// A per-layout `field_types` table is then built incrementally — only
// types actually referenced by the schema get an entry, mirroring how
// real tags only carry the types they use.
//

struct FieldTypeInfo {
    ty: TagFieldType,
    canonical: &'static str,
    size: u32,
    needs_sub_chunk: u32,
}

/// JSON `"type": "..."` string → metadata. Snake-case names match what
/// the dumper emits; `canonical` is the space-separated form that goes
/// into the blay's string table (matches what `TagFieldType::from_name`
/// parses).
fn field_type_info(ty: &str) -> Option<FieldTypeInfo> {
    use TagFieldType::*;
    Some(match ty {
        "string"                   => FieldTypeInfo { ty: String,              canonical: "string",                   size: 32,  needs_sub_chunk: 0 },
        "long_string"              => FieldTypeInfo { ty: LongString,          canonical: "long string",              size: 256, needs_sub_chunk: 0 },
        "string_id"                => FieldTypeInfo { ty: StringId,            canonical: "string id",                size: 4,   needs_sub_chunk: 1 },
        "old_string_id"            => FieldTypeInfo { ty: OldStringId,         canonical: "old string id",            size: 4,   needs_sub_chunk: 1 },
        "char_integer"             => FieldTypeInfo { ty: CharInteger,         canonical: "char integer",             size: 1,   needs_sub_chunk: 0 },
        "short_integer"            => FieldTypeInfo { ty: ShortInteger,        canonical: "short integer",            size: 2,   needs_sub_chunk: 0 },
        "long_integer"             => FieldTypeInfo { ty: LongInteger,         canonical: "long integer",             size: 4,   needs_sub_chunk: 0 },
        "int64_integer"            => FieldTypeInfo { ty: Int64Integer,        canonical: "int64 integer",            size: 8,   needs_sub_chunk: 0 },
        "angle"                    => FieldTypeInfo { ty: Angle,               canonical: "angle",                    size: 4,   needs_sub_chunk: 0 },
        "tag"                      => FieldTypeInfo { ty: Tag,                 canonical: "tag",                      size: 4,   needs_sub_chunk: 0 },
        "char_enum"                => FieldTypeInfo { ty: CharEnum,            canonical: "char enum",                size: 1,   needs_sub_chunk: 0 },
        "short_enum"               => FieldTypeInfo { ty: ShortEnum,           canonical: "short enum",               size: 2,   needs_sub_chunk: 0 },
        "long_enum"                => FieldTypeInfo { ty: LongEnum,            canonical: "long enum",                size: 4,   needs_sub_chunk: 0 },
        "long_flags"               => FieldTypeInfo { ty: LongFlags,           canonical: "long flags",               size: 4,   needs_sub_chunk: 0 },
        "word_flags"               => FieldTypeInfo { ty: WordFlags,           canonical: "word flags",               size: 2,   needs_sub_chunk: 0 },
        "byte_flags"               => FieldTypeInfo { ty: ByteFlags,           canonical: "byte flags",               size: 1,   needs_sub_chunk: 0 },
        "point_2d"                 => FieldTypeInfo { ty: Point2d,             canonical: "point 2d",                 size: 4,   needs_sub_chunk: 0 },
        "rectangle_2d"             => FieldTypeInfo { ty: Rectangle2d,         canonical: "rectangle 2d",             size: 8,   needs_sub_chunk: 0 },
        "rgb_color"                => FieldTypeInfo { ty: RgbColor,            canonical: "rgb color",                size: 4,   needs_sub_chunk: 0 },
        "argb_color"               => FieldTypeInfo { ty: ArgbColor,           canonical: "argb color",               size: 4,   needs_sub_chunk: 0 },
        "real"                     => FieldTypeInfo { ty: Real,                canonical: "real",                     size: 4,   needs_sub_chunk: 0 },
        "real_slider"              => FieldTypeInfo { ty: RealSlider,          canonical: "real slider",              size: 4,   needs_sub_chunk: 0 },
        "real_fraction"            => FieldTypeInfo { ty: RealFraction,        canonical: "real fraction",            size: 4,   needs_sub_chunk: 0 },
        "real_point_2d"            => FieldTypeInfo { ty: RealPoint2d,         canonical: "real point 2d",            size: 8,   needs_sub_chunk: 0 },
        "real_point_3d"            => FieldTypeInfo { ty: RealPoint3d,         canonical: "real point 3d",            size: 12,  needs_sub_chunk: 0 },
        "real_vector_2d"           => FieldTypeInfo { ty: RealVector2d,        canonical: "real vector 2d",           size: 8,   needs_sub_chunk: 0 },
        "real_vector_3d"           => FieldTypeInfo { ty: RealVector3d,        canonical: "real vector 3d",           size: 12,  needs_sub_chunk: 0 },
        "real_quaternion"          => FieldTypeInfo { ty: RealQuaternion,      canonical: "real quaternion",          size: 16,  needs_sub_chunk: 0 },
        "real_euler_angles_2d"     => FieldTypeInfo { ty: RealEulerAngles2d,   canonical: "real euler angles 2d",     size: 8,   needs_sub_chunk: 0 },
        "real_euler_angles_3d"     => FieldTypeInfo { ty: RealEulerAngles3d,   canonical: "real euler angles 3d",     size: 12,  needs_sub_chunk: 0 },
        "real_plane_2d"            => FieldTypeInfo { ty: RealPlane2d,         canonical: "real plane 2d",            size: 12,  needs_sub_chunk: 0 },
        "real_plane_3d"            => FieldTypeInfo { ty: RealPlane3d,         canonical: "real plane 3d",            size: 16,  needs_sub_chunk: 0 },
        "real_rgb_color"           => FieldTypeInfo { ty: RealRgbColor,        canonical: "real rgb color",           size: 12,  needs_sub_chunk: 0 },
        "real_argb_color"          => FieldTypeInfo { ty: RealArgbColor,       canonical: "real argb color",          size: 16,  needs_sub_chunk: 0 },
        "real_hsv_color"           => FieldTypeInfo { ty: RealHsvColor,        canonical: "real hsv color",           size: 12,  needs_sub_chunk: 0 },
        "real_ahsv_color"          => FieldTypeInfo { ty: RealAhsvColor,       canonical: "real ahsv color",          size: 16,  needs_sub_chunk: 0 },
        "short_bounds"             => FieldTypeInfo { ty: ShortIntegerBounds,  canonical: "short integer bounds",     size: 4,   needs_sub_chunk: 0 },
        "angle_bounds"             => FieldTypeInfo { ty: AngleBounds,         canonical: "angle bounds",             size: 8,   needs_sub_chunk: 0 },
        "real_bounds"              => FieldTypeInfo { ty: RealBounds,          canonical: "real bounds",              size: 8,   needs_sub_chunk: 0 },
        "fraction_bounds"          => FieldTypeInfo { ty: FractionBounds,      canonical: "fraction bounds",          size: 8,   needs_sub_chunk: 0 },
        "tag_reference"            => FieldTypeInfo { ty: TagReference,        canonical: "tag reference",            size: 16,  needs_sub_chunk: 1 },
        "block"                    => FieldTypeInfo { ty: Block,               canonical: "block",                    size: 12,  needs_sub_chunk: 1 },
        "long_block_flags"         => FieldTypeInfo { ty: LongBlockFlags,      canonical: "long block flags",         size: 4,   needs_sub_chunk: 0 },
        "word_block_flags"         => FieldTypeInfo { ty: WordBlockFlags,      canonical: "word block flags",         size: 2,   needs_sub_chunk: 0 },
        "byte_block_flags"         => FieldTypeInfo { ty: ByteBlockFlags,      canonical: "byte block flags",         size: 1,   needs_sub_chunk: 0 },
        "char_block_index"         => FieldTypeInfo { ty: CharBlockIndex,      canonical: "char block index",         size: 1,   needs_sub_chunk: 0 },
        "custom_char_block_index"  => FieldTypeInfo { ty: CustomCharBlockIndex,  canonical: "custom char block index",  size: 1, needs_sub_chunk: 0 },
        "short_block_index"        => FieldTypeInfo { ty: ShortBlockIndex,     canonical: "short block index",        size: 2,   needs_sub_chunk: 0 },
        "custom_short_block_index" => FieldTypeInfo { ty: CustomShortBlockIndex, canonical: "custom short block index", size: 2, needs_sub_chunk: 0 },
        "long_block_index"         => FieldTypeInfo { ty: LongBlockIndex,      canonical: "long block index",         size: 4,   needs_sub_chunk: 0 },
        "custom_long_block_index"  => FieldTypeInfo { ty: CustomLongBlockIndex,  canonical: "custom long block index",  size: 4, needs_sub_chunk: 0 },
        "data"                     => FieldTypeInfo { ty: Data,                canonical: "data",                     size: 20,  needs_sub_chunk: 1 },
        "vertex_buffer"            => FieldTypeInfo { ty: VertexBuffer,        canonical: "vertex buffer",            size: 32,  needs_sub_chunk: 0 },
        "pad"                      => FieldTypeInfo { ty: Pad,                 canonical: "pad",                      size: 0,   needs_sub_chunk: 0 },
        "useless_pad"              => FieldTypeInfo { ty: UselessPad,          canonical: "useless pad",              size: 0,   needs_sub_chunk: 0 },
        "skip"                     => FieldTypeInfo { ty: Skip,                canonical: "skip",                     size: 0,   needs_sub_chunk: 0 },
        "explanation"              => FieldTypeInfo { ty: Explanation,         canonical: "explanation",              size: 0,   needs_sub_chunk: 0 },
        "custom"                   => FieldTypeInfo { ty: Custom,              canonical: "custom",                   size: 0,   needs_sub_chunk: 0 },
        "struct"                   => FieldTypeInfo { ty: Struct,              canonical: "struct",                   size: 0,   needs_sub_chunk: 1 },
        "array"                    => FieldTypeInfo { ty: Array,               canonical: "array",                    size: 0,   needs_sub_chunk: 0 },
        "tag_resource"             => FieldTypeInfo { ty: PageableResource,    canonical: "pageable resource",        size: 8,   needs_sub_chunk: 1 },
        "tag_interop"              => FieldTypeInfo { ty: ApiInterop,          canonical: "api interop",              size: 12,  needs_sub_chunk: 1 },
        "terminator"               => FieldTypeInfo { ty: Terminator,          canonical: "terminator X",             size: 0,   needs_sub_chunk: 0 },
        _ => return None,
    })
}

//
// String table builder — dedups identical strings so `name_offset`
// values in the layout point at shared bytes.
//

#[derive(Default)]
struct StringTable {
    bytes: Vec<u8>,
    offsets: std::collections::HashMap<String, u32>,
}

impl StringTable {
    fn new() -> Self {
        // An empty string at offset 0 is free and gives a canonical
        // "nameless" target for fields without a name.
        let mut me = Self::default();
        me.offsets.insert(String::new(), 0);
        me.bytes.push(0);
        me
    }
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(&off) = self.offsets.get(s) {
            return off;
        }
        let off = self.bytes.len() as u32;
        self.bytes.extend_from_slice(s.as_bytes());
        self.bytes.push(0);
        self.offsets.insert(s.to_owned(), off);
        off
    }
}

fn parse_group_tag(s: &str) -> Result<u32, FromJsonError> {
    let bytes = s.as_bytes();
    if bytes.len() != 4 {
        return Err(FromJsonError::BadGroupTag(s.to_owned()));
    }
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn parse_guid(s: &str) -> Result<[u8; 16], FromJsonError> {
    if s.len() != 32 {
        return Err(FromJsonError::BadGuid(s.to_owned()));
    }
    let mut out = [0u8; 16];
    for i in 0..16 {
        out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
            .map_err(|_| FromJsonError::BadGuid(s.to_owned()))?;
    }
    Ok(out)
}

/// Group-level metadata extracted from a schema JSON file. Not part
/// of `TagLayout` (blay doesn't carry it) but needed by `TagFile`
/// to populate its header.
#[derive(Debug, Clone)]
pub struct TagGroupMeta {
    pub tag: u32,
    pub version: u32,
    pub flags: u32,
    pub parent_tag: Option<u32>,
}

impl TagLayout {
    /// Build a TagLayout from a JSON schema file (per-group output of
    /// `h3_guerilla_dump_tag_definitions_json.py`). The result matches
    /// what `TagLayout::read` would produce from an equivalent blay
    /// chunk: same string_data/string_offsets/string_lists,
    /// struct_layouts/block_layouts/etc. with consistent indices, and
    /// every struct's size + field offsets computed.
    ///
    /// Returns `FromJsonError::StructSizeMismatch` if the computed
    /// size of any struct disagrees with what the JSON's `size` field
    /// claims — that's our cross-check against `field_type_info`'s
    /// size column being wrong.
    pub fn from_json(path: impl AsRef<Path>) -> Result<Self, FromJsonError> {
        Self::from_json_with_meta(path).map(|(l, _)| l)
    }

    /// Like [`TagLayout::from_json`] but also returns the group-level
    /// metadata (group tag, version, flags, parent_tag) that the JSON
    /// carries but blay doesn't. Needed when creating a new tag file
    /// from scratch — the file header needs `group_tag` /
    /// `group_version`.
    pub fn from_json_with_meta(
        path: impl AsRef<Path>,
    ) -> Result<(Self, TagGroupMeta), FromJsonError> {
        let path = path.as_ref();
        let file = std::fs::File::open(path)?;
        let schema: TagGroupSchema = serde_json::from_reader(std::io::BufReader::new(file))?;
        let meta = TagGroupMeta {
            tag: parse_group_tag(&schema.tag)?,
            version: schema.version,
            flags: schema.flags,
            parent_tag: schema.parent_tag.as_deref().map(parse_group_tag).transpose()?,
        };
        // `tmpl` custom expansion sizes are resolved by loading the
        // sibling group JSONs from the same directory on demand.
        let defs_dir = path.parent().unwrap_or(Path::new("."));
        let layout = build_layout_from_schema(schema, defs_dir)?;
        Ok((layout, meta))
    }
}

/// Walk a `tmpl` target's parent chain and return the cumulative
/// root-struct size. The target itself is *excluded* — its own fields
/// are serialized via the sibling `struct` field that follows the
/// tmpl custom. Returns 0 if the target can't be resolved (dead
/// templates like `ssfx` with no `_meta.json` entry).
///
/// Loads `_meta.json` to map 4cc → filename, then walks up the chain
/// reading each ancestor's JSON on demand.
fn tmpl_expansion_size(defs_dir: &Path, target_tag: &str) -> u32 {
    let Ok(meta_bytes) = std::fs::read(defs_dir.join("_meta.json")) else { return 0 };
    let Ok(meta): Result<serde_json::Value, _> = serde_json::from_slice(&meta_bytes) else {
        return 0;
    };
    let Some(tag_index) = meta.get("tag_index").and_then(|v| v.as_object()) else { return 0 };

    let mut sum: u32 = 0;
    let mut cur = target_tag.to_owned();
    for _ in 0..32 {
        let Some(name) = tag_index.get(&cur).and_then(|v| v.as_str()) else { break };
        let Ok(bytes) = std::fs::read(defs_dir.join(format!("{name}.json"))) else { break };
        let Ok(schema): Result<TagGroupSchema, _> = serde_json::from_slice(&bytes) else { break };
        // Skip the target itself — we only add parent chain sizes.
        if cur != target_tag {
            let Some(block) = schema.blocks.get(&schema.block) else { break };
            let Some(rs) = schema.structs.get(&block.struct_name) else { break };
            sum = sum.saturating_add(rs.size);
        }
        let Some(parent) = schema.parent_tag else { break };
        cur = parent;
    }
    sum
}

fn build_layout_from_schema(
    schema: TagGroupSchema,
    defs_dir: &Path,
) -> Result<TagLayout, FromJsonError> {
    let _ = parse_group_tag(&schema.tag)?; // validate early

    let mut strings = StringTable::new();

    // Index assignment (stable, alphabetical via BTreeMap iteration).
    let struct_index: BTreeMap<&str, u32> = schema
        .structs
        .keys()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i as u32))
        .collect();
    let block_index: BTreeMap<&str, u32> = schema
        .blocks
        .keys()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i as u32))
        .collect();
    let array_index: BTreeMap<&str, u32> = schema
        .arrays
        .keys()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i as u32))
        .collect();
    let enum_index: BTreeMap<&str, u32> = schema
        .enums_flags
        .keys()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i as u32))
        .collect();
    let data_index: BTreeMap<&str, u32> = schema
        .datas
        .keys()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i as u32))
        .collect();
    let resource_index: BTreeMap<&str, u32> = schema
        .resources
        .keys()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i as u32))
        .collect();
    let interop_index: BTreeMap<&str, u32> = schema
        .interops
        .keys()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i as u32))
        .collect();

    // field_types registry — grown on-demand as fields are emitted.
    let mut field_types: Vec<TagFieldTypeLayout> = Vec::new();
    let mut field_type_index_by_name: std::collections::HashMap<&'static str, u32> = Default::default();
    let mut intern_field_type = |canonical: &'static str, size: u32, needs_sub: u32,
                                 strings: &mut StringTable|
     -> u32 {
        if let Some(&i) = field_type_index_by_name.get(canonical) {
            return i;
        }
        let name_offset = strings.intern(canonical);
        let i = field_types.len() as u32;
        field_types.push(TagFieldTypeLayout {
            name_offset,
            size,
            needs_sub_chunk: needs_sub,
        });
        field_type_index_by_name.insert(canonical, i);
        i
    };

    // Build custom_block_index_search_names_offsets — one entry per
    // *distinct* search-name string seen on custom_*_block_index
    // fields. Fields' `definition` becomes the index into here.
    // (Our JSON doesn't currently carry search names, so this stays
    // empty unless the dumper starts emitting them.)
    let custom_block_index_search_names_offsets: Vec<u32> = Vec::new();

    // Build data_definition_name_offsets from `datas` keys.
    let data_definition_name_offsets: Vec<u32> = schema
        .datas
        .keys()
        .map(|n| strings.intern(n))
        .collect();

    // Build string_lists (enums/flags). Each enum's options go into
    // string_offsets contiguously; string_lists[i] points at that
    // slice.
    let mut string_offsets: Vec<u32> = Vec::new();
    let mut string_lists: Vec<TagStringList> = Vec::new();
    for (name, enum_schema) in &schema.enums_flags {
        let list_name_offset = strings.intern(name);
        let first = string_offsets.len() as u32;
        for opt in &enum_schema.options {
            let off = match opt {
                Some(s) => strings.intern(s),
                None => 0, // null option slot → empty string at offset 0
            };
            string_offsets.push(off);
        }
        string_lists.push(TagStringList {
            offset: list_name_offset,
            count: enum_schema.options.len() as u32,
            first,
        });
    }

    // Build array_layouts (resolve each array's struct by name).
    let mut array_layouts: Vec<TagArrayLayout> = Vec::with_capacity(schema.arrays.len());
    for (name, array) in &schema.arrays {
        let si = *struct_index.get(array.struct_name.as_str()).ok_or_else(|| {
            FromJsonError::UnknownReference { kind: "struct", name: array.struct_name.clone() }
        })?;
        array_layouts.push(TagArrayLayout {
            name_offset: strings.intern(name),
            count: array.count,
            struct_index: si,
        });
    }

    // Build resource_layouts.
    let mut resource_layouts: Vec<TagResourceLayout> = Vec::with_capacity(schema.resources.len());
    for (name, resource) in &schema.resources {
        let si = *struct_index.get(resource.struct_name.as_str()).ok_or_else(|| {
            FromJsonError::UnknownReference { kind: "struct", name: resource.struct_name.clone() }
        })?;
        resource_layouts.push(TagResourceLayout {
            name_offset: strings.intern(name),
            unknown: resource.flags as u32,
            struct_index: si,
        });
    }

    // Build interop_layouts.
    let mut interop_layouts: Vec<TagInteropLayout> = Vec::with_capacity(schema.interops.len());
    for (name, interop) in &schema.interops {
        let si = *struct_index.get(interop.struct_name.as_str()).ok_or_else(|| {
            FromJsonError::UnknownReference { kind: "struct", name: interop.struct_name.clone() }
        })?;
        interop_layouts.push(TagInteropLayout {
            name_offset: strings.intern(name),
            struct_index: si,
            guid: parse_guid(&interop.guid)?,
        });
    }

    // Build block_layouts.
    let mut block_layouts: Vec<TagBlockLayout> = Vec::with_capacity(schema.blocks.len());
    for (i, (name, block)) in schema.blocks.iter().enumerate() {
        let si = *struct_index.get(block.struct_name.as_str()).ok_or_else(|| {
            FromJsonError::UnknownReference { kind: "struct", name: block.struct_name.clone() }
        })?;
        block_layouts.push(TagBlockLayout {
            index: i as u32,
            name_offset: strings.intern(name),
            max_count: block.max_count,
            struct_index: si,
        });
    }

    // Build struct_layouts + the flat `fields` array. For each struct,
    // remember its `first_field_index` before pushing its fields.
    let mut struct_layouts: Vec<TagStructLayout> = Vec::with_capacity(schema.structs.len());
    let mut fields: Vec<TagFieldLayout> = Vec::new();
    for (i, (name, struct_schema)) in schema.structs.iter().enumerate() {
        let first = fields.len() as u32;

        for field in &struct_schema.fields {
            let info = field_type_info(&field.ty)
                .ok_or_else(|| FromJsonError::UnknownFieldType(field.ty.clone()))?;

            let type_index = intern_field_type(
                info.canonical,
                info.size,
                info.needs_sub_chunk,
                &mut strings,
            );

            let field_name_offset = match &field.name {
                Some(n) => strings.intern(n),
                None => 0,
            };

            let definition = resolve_field_definition(
                field,
                info.ty,
                &struct_index,
                &block_index,
                &array_index,
                &enum_index,
                &data_index,
                &resource_index,
                &interop_index,
            )?;

            fields.push(TagFieldLayout {
                name_offset: field_name_offset,
                type_index,
                definition,
                field_type: info.ty,
                offset: 0, // computed later by compute_struct_layout
            });
        }

        struct_layouts.push(TagStructLayout {
            index: i as u32,
            guid: parse_guid(&struct_schema.guid)?,
            name_offset: strings.intern(name),
            first_field_index: first,
            size: 0, // computed later
            version: 0,
        });
    }

    // Pull root-block index. Its struct's guid/size become the layout-
    // level guid/root_data_size (matching `TagLayout::read`).
    let root_block_index = *block_index.get(schema.block.as_str()).ok_or_else(|| {
        FromJsonError::UnknownReference { kind: "block", name: schema.block.clone() }
    })?;
    let root_struct_index = block_layouts[root_block_index as usize].struct_index as usize;
    let root_struct = &struct_layouts[root_struct_index];
    let layout_guid = root_struct.guid;
    let schema_root_size = schema.structs.iter().nth(root_struct_index).map(|(_, s)| s.size).unwrap_or(0);

    let header = TagLayoutHeader {
        tag_group_block_index: root_block_index,
        string_data_size: 0, // filled in below
        string_offset_count: string_offsets.len() as u32,
        string_list_count: string_lists.len() as u32,
        custom_block_index_search_names_count: custom_block_index_search_names_offsets.len() as u32,
        data_definition_name_count: data_definition_name_offsets.len() as u32,
        array_layout_count: array_layouts.len() as u32,
        field_type_count: field_types.len() as u32,
        field_count: fields.len() as u32,
        aggregate_layout_count: 0,
        struct_layout_count: struct_layouts.len() as u32,
        block_layout_count: block_layouts.len() as u32,
        resource_layout_count: resource_layouts.len() as u32,
        interop_layout_count: interop_layouts.len() as u32,
    };

    let mut result = TagLayout {
        root_data_size: schema_root_size,
        guid: layout_guid,
        version: 3, // H3 MCC — layout payload version 3
        header: TagLayoutHeader {
            string_data_size: strings.bytes.len() as u32,
            ..header
        },
        string_data: strings.bytes,
        string_offsets,
        string_lists,
        custom_block_index_search_names_offsets,
        data_definition_name_offsets,
        array_layouts,
        field_types,
        fields,
        block_layouts,
        resource_layouts,
        interop_layouts,
        struct_layouts,
    };

    // Compute struct sizes + field offsets. First pass with tmpl
    // customs stored at 0 (no expansion) — matches how H3 schemas lay
    // out (common shader fields are inlined directly in the struct
    // field that follows the tmpl).
    let tmpl_expansions: Vec<(usize, u32)> = {
        let mut out = Vec::new();
        let mut global_field_idx = 0usize;
        for (_, struct_schema) in schema.structs.iter() {
            for field in &struct_schema.fields {
                if field.ty == "custom"
                    && field.group_tag.as_deref() == Some("tmpl")
                {
                    if let Some(target) = field.definition.as_str() {
                        let exp = tmpl_expansion_size(defs_dir, target);
                        if exp > 0 {
                            out.push((global_field_idx, exp));
                        }
                    }
                }
                global_field_idx += 1;
            }
        }
        out
    };

    for i in 0..result.struct_layouts.len() {
        result.compute_struct_layout(i);
    }

    // Cross-check computed sizes against the schema's stated sizes.
    // If declared > computed and this struct has tmpl customs, apply
    // their expansion (Reach-style: parent-chain inlined here) and
    // recompute. If declared still doesn't match — or we're > declared
    // — it's a genuine mismatch.
    for (i, (name, struct_schema)) in schema.structs.iter().enumerate() {
        let computed = result.struct_layouts[i].size;
        let declared = struct_schema.size as usize;
        if computed == declared {
            continue;
        }
        if computed < declared {
            // Try tmpl expansion for this struct's fields.
            let first = result.struct_layouts[i].first_field_index as usize;
            let mut field_idx = first;
            let mut applied = 0usize;
            while result.fields[field_idx].field_type != TagFieldType::Terminator {
                if let Some(&(_, exp)) = tmpl_expansions.iter().find(|&&(fi, _)| fi == field_idx) {
                    result.fields[field_idx].definition = exp;
                    applied += exp as usize;
                }
                field_idx += 1;
            }
            if applied > 0 {
                // Reset the struct's size so compute_struct_layout runs again.
                result.struct_layouts[i].size = 0;
                result.compute_struct_layout(i);
            }
        }
        let computed = result.struct_layouts[i].size;
        if computed != declared {
            return Err(FromJsonError::StructSizeMismatch {
                name: name.clone(),
                schema: struct_schema.size,
                computed,
            });
        }
    }

    // Update header size-counts that depend on final string_data size.
    result.header.string_data_size = result.string_data.len() as u32;

    Ok(result)
}

/// Translate a field schema's `definition` value into the `u32` that
/// goes into the corresponding `TagFieldLayout`. The interpretation
/// depends on the field type:
///
/// - named-registry types (struct/block/array/flags/enum/data/
///   resource/interop): string → index into the matching table.
/// - `pad`/`useless_pad`/`skip`: integer → byte count (stored in the
///   `definition` slot verbatim).
/// - `tag_reference`: object → would normally store flags+allowed,
///   but blay only stores flags here (just flags slot).
/// - `explanation`: string → stored as a string offset into
///   string_data.
/// - primitives / `terminator`: 0.
fn resolve_field_definition(
    field: &TagFieldSchema,
    ty: TagFieldType,
    struct_index: &BTreeMap<&str, u32>,
    block_index: &BTreeMap<&str, u32>,
    array_index: &BTreeMap<&str, u32>,
    enum_index: &BTreeMap<&str, u32>,
    data_index: &BTreeMap<&str, u32>,
    resource_index: &BTreeMap<&str, u32>,
    interop_index: &BTreeMap<&str, u32>,
) -> Result<u32, FromJsonError> {
    use TagFieldType::*;

    let def = &field.definition;

    // `custom` fields contribute 0 bytes by default. `tmpl`-typed
    // customs inline their target group's parent-chain size only
    // when the containing struct's declared size is larger than the
    // sum of plain field sizes — that post-hoc patch happens in
    // `build_layout_from_schema`, not here.
    if matches!(ty, Custom) {
        return Ok(0);
    }

    // Primitives & no-definition types: return 0.
    if matches!(
        ty,
        Unknown
            | String
            | LongString
            | StringId
            | OldStringId
            | CharInteger
            | ShortInteger
            | LongInteger
            | Int64Integer
            | Angle
            | Tag
            | Point2d
            | Rectangle2d
            | RgbColor
            | ArgbColor
            | Real
            | RealSlider
            | RealFraction
            | RealPoint2d
            | RealPoint3d
            | RealVector2d
            | RealVector3d
            | RealQuaternion
            | RealEulerAngles2d
            | RealEulerAngles3d
            | RealPlane2d
            | RealPlane3d
            | RealRgbColor
            | RealArgbColor
            | RealHsvColor
            | RealAhsvColor
            | ShortIntegerBounds
            | AngleBounds
            | RealBounds
            | FractionBounds
            | VertexBuffer
            | CustomCharBlockIndex
            | CustomShortBlockIndex
            | CustomLongBlockIndex
            | Terminator,
    ) {
        return Ok(0);
    }

    // Pad/skip/useless_pad: definition is a byte count integer.
    if matches!(ty, Pad | UselessPad | Skip) {
        return def
            .as_u64()
            .map(|v| v as u32)
            .ok_or_else(|| FromJsonError::BadFieldDefinition {
                field: field.name.clone().unwrap_or_default(),
                ty: field.ty.clone(),
            });
    }

    // Explanation: store as 0 in the layout (blay's `definition` slot
    // holds the string offset at runtime via a separate mechanism).
    // Preserving the text in string_data is out-of-scope for now.
    if matches!(ty, Explanation) {
        return Ok(0);
    }

    // tag_reference: blay's `definition` holds flags. `allowed` list
    // isn't part of blay's field record.
    if matches!(ty, TagReference) {
        let flags = def
            .as_object()
            .and_then(|m| m.get("flags"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        return Ok(flags as u32);
    }

    // Named-registry types: resolve by name.
    let name = def.as_str().ok_or_else(|| FromJsonError::BadFieldDefinition {
        field: field.name.clone().unwrap_or_default(),
        ty: field.ty.clone(),
    })?;
    let lookup = match ty {
        Struct => struct_index.get(name).copied(),
        Block | LongBlockFlags | WordBlockFlags | ByteBlockFlags | CharBlockIndex
        | ShortBlockIndex | LongBlockIndex => block_index.get(name).copied(),
        Array => array_index.get(name).copied(),
        CharEnum | ShortEnum | LongEnum | LongFlags | WordFlags | ByteFlags => {
            enum_index.get(name).copied()
        }
        Data => data_index.get(name).copied(),
        PageableResource => resource_index.get(name).copied(),
        ApiInterop => interop_index.get(name).copied(),
        _ => None,
    };
    lookup.ok_or_else(|| FromJsonError::UnknownReference {
        kind: match ty {
            Struct => "struct",
            Block | LongBlockFlags | WordBlockFlags | ByteBlockFlags | CharBlockIndex
            | ShortBlockIndex | LongBlockIndex => "block",
            Array => "array",
            CharEnum | ShortEnum | LongEnum | LongFlags | WordFlags | ByteFlags => "enum_or_flags",
            Data => "data",
            PageableResource => "resource",
            ApiInterop => "interop",
            _ => "?",
        },
        name: name.to_owned(),
    })
}
