//! Tag data tree: the per-tag instance values shaped by a layout.
//!
//! Byte ownership is **per block**. Each [`TagBlockData`] owns a single
//! `raw_data` buffer holding all of its elements' bytes laid out
//! contiguously. Nested structs, inline arrays, and exploded
//! pageable-resource payloads are *offset regions* inside their
//! enclosing block's `raw_data` — they don't own bytes of their own.
//! Navigating into a sub-block starts a fresh byte region (the
//! sub-block's own `raw_data`).
//!
//! This matches the on-disk `tgbl` chunk layout 1:1: `count + flags +
//! concatenated element bytes + per-element tgst sub-chunks`.

use std::error::Error;
use std::io::{Read, Seek, SeekFrom, Write};

use crate::fields::{deserialize_field, serialize_field, TagFieldData, TagFieldType};
use crate::io::*;
use crate::layout::{TagBlockDefinition, TagBlockLayout, TagLayout, TagStructDefinition};

/// A struct within a tag's data tree. Owns its `sub_chunks` (nested
/// structures + leaf sub-chunks); its *bytes* live in the enclosing
/// [`TagBlockData::raw_data`] at an offset determined by path descent.
#[derive(Debug, Clone)]
pub struct TagStruct {
    /// Index into [`TagLayout::struct_definitions`].
    pub struct_index: u32,
    /// Sub-chunks emitted inside this struct's `tgst` chunk, in
    /// emission order. Only populated for fields whose type needs a
    /// sub-chunk. The tgst chunk itself has no raw bytes of its
    /// own — the parent block's `raw_data` carries them.
    pub sub_chunks: Vec<TagSubChunkEntry>,
}

#[derive(Debug, Clone)]
pub struct TagSubChunkEntry {
    /// Index into [`TagLayout::fields`] for the owning field, or
    /// `None` for empty placeholder `tgst` chunks that don't
    /// correspond to any layout field. See
    /// [`TagSubChunkContent::EmptyPlaceholder`].
    pub field_index: Option<u32>,
    pub content: TagSubChunkContent,
}

#[derive(Debug, Clone)]
pub enum TagSubChunkContent {
    /// Nested struct field. Its raw bytes live in the enclosing
    /// block's `raw_data` at the field's offset within the containing
    /// struct.
    Struct(TagStruct),
    /// Nested block field. Starts a new byte region — the block
    /// carries its own `raw_data`.
    Block(TagBlockData),
    /// Inline fixed-count array. Each element's raw bytes live in the
    /// enclosing block's `raw_data` at `field.offset + i *
    /// element_size`. The vector length equals the schema-declared
    /// array count.
    Array(Vec<TagStruct>),
    /// `tgrf` chunk payload (4-byte group_tag + null-terminated path).
    /// Header is implicit — signature and size are reconstructible on
    /// write.
    TagReference(Vec<u8>),
    /// `tgsi` chunk payload (utf-8 bytes, empty = string_id::NONE).
    StringId(Vec<u8>),
    /// `tgsi` chunk payload for old-style string ids.
    OldStringId(Vec<u8>),
    /// `tgda` chunk payload.
    Data(Vec<u8>),
    /// Pageable resource. Signature distinguishes between concrete
    /// resource chunk shapes. Only the two observed in Halo 3 / Reach
    /// tags are modeled.
    Resource(TagResourceChunk),
    /// An empty `tgst` chunk (size=0) that doesn't correspond to any
    /// layout field. MCC's writer emits these as a placeholder before
    /// the real tgst for a struct sub-chunk field, and as trailing
    /// filler at the end of some struct contents. Preserved verbatim
    /// (as the entry's position within the parent's `sub_chunks`) so
    /// write-side can re-emit them at the correct byte offset.
    EmptyPlaceholder,
}

#[derive(Debug, Clone)]
pub enum TagResourceChunk {
    /// `tg\0c` — empty null resource.
    Null,
    /// `tgrc` — exploded/control resource. Wraps a nested `tgdt`
    /// payload blob and the resource's own struct tree. The resource
    /// struct's raw bytes (typically 8 inline bytes) live in the
    /// enclosing block's `raw_data` at the resource field's offset.
    Exploded {
        /// `tgdt` payload (content bytes only; header reconstructible
        /// on write).
        exploded: Vec<u8>,
        /// Nested resource struct tree (sub_chunks only).
        struct_data: TagStruct,
    },
    /// `tgxc` — XSync resource. Opaque payload. Not seen in the
    /// Halo 3 / Reach MCC corpus; kept here so future tags that use
    /// it don't panic.
    Xsync(Vec<u8>),
}

/// Lightweight description of a container-shaped field's current
/// state, produced by [`TagStruct::container_kind`]. Lets callers
/// summarize (e.g. "block [7 elements]") without walking sub_chunks
/// themselves.
#[derive(Debug, Clone, Copy)]
pub enum ContainerKind {
    /// An inline sub-struct. No count — a struct contains a fixed
    /// set of fields defined by its layout.
    Struct,
    /// A variable-count block. `count` is the current number of
    /// elements.
    Block { count: usize },
    /// A fixed-count inline array. `count` is the schema-declared
    /// element count (never changes at runtime).
    Array { count: usize },
    /// A pageable resource. No introspectable count — the shape is
    /// one of [`TagResourceChunk::Null`] / [`TagResourceChunk::Exploded`]
    /// / [`TagResourceChunk::Xsync`].
    PageableResource,
}

impl TagStruct {
    /// Parse a `tgst` chunk.
    ///
    /// `raw_data` is the enclosing slice that owns this struct's
    /// bytes — passed in by the caller (the containing block). This
    /// method parses only the `tgst` header and its sub-chunks from
    /// `reader`; the raw bytes themselves stay in the enclosing
    /// block's `raw_data`.
    pub fn read<R: Seek + Read>(
        block_layout: &TagBlockLayout,
        definition: &TagStructDefinition,
        raw_data: &[u8],
        reader: &mut std::io::BufReader<R>,
    ) -> Result<Self, Box<dyn Error>> {
        let tag_struct_header = read_tag_chunk_header(reader)?;
        let tag_struct_offset = reader.stream_position()?;
        assert!(
            tag_struct_header.signature == u32::from_be_bytes(*b"tgst"),
            "Invalid tag struct header at 0x{:X}",
            tag_struct_offset - 12,
        );
        // HYPOTHESIS: tgst.version always equals tgst.size.
        assert_eq!(
            tag_struct_header.version, tag_struct_header.size,
            "tgst version ({}) != size ({}) at 0x{:X}",
            tag_struct_header.version, tag_struct_header.size, tag_struct_offset - 12,
        );

        // tgst with size=0 is a null struct: no sub-chunks follow.
        let sub_chunks = if tag_struct_header.size != 0 {
            let mut sub_chunks = read_sub_chunks(block_layout, definition, raw_data, reader)?;

            // Trailing empty-tgst absorb: MCC's writer occasionally
            // emits size=0 tgst chunks at the end of a struct's
            // content that don't correspond to any layout field.
            // Preserve them as EmptyPlaceholder entries so write-side
            // re-emits them at the same position.
            let mut end_offset = reader.stream_position()?;
            let expected_offset = tag_struct_offset + tag_struct_header.size as u64;

            if end_offset != expected_offset {
                let mut non_empty_trailing_chunks = false;

                loop {
                    end_offset = reader.stream_position()?;

                    if end_offset == expected_offset {
                        break;
                    }

                    let trailer = read_tag_chunk_header(reader)?;

                    if trailer.signature != u32::from_be_bytes(*b"tgst") || trailer.size != 0 {
                        non_empty_trailing_chunks = true;
                        break;
                    }

                    assert_eq!(
                        trailer.version, 0,
                        "trailing empty tgst version ({}) != 0", trailer.version,
                    );
                    sub_chunks.push(TagSubChunkEntry {
                        field_index: None,
                        content: TagSubChunkContent::EmptyPlaceholder,
                    });
                }

                if non_empty_trailing_chunks {
                    let tag_struct_name = block_layout.layout.get_string(definition.name_offset).unwrap();

                    panic!(
                        "failed to read 'tgst' \"{tag_struct_name}\": started at 0x{tag_struct_offset:X}, \
                         ended at 0x{end_offset:X}, expected 0x{expected_offset:X}"
                    );
                }
            }

            sub_chunks
        } else {
            Vec::new()
        };

        Ok(Self {
            struct_index: definition.index,
            sub_chunks,
        })
    }

    /// Write this struct as a `tgst` chunk. Emits only the sub_chunks
    /// content; the struct's raw bytes flow out through the enclosing
    /// block's `raw_data` concatenation.
    pub fn write<W: Write>(
        &self,
        block_layout: &TagBlockLayout,
        writer: &mut W,
    ) -> std::io::Result<()> {
        let mut content = Vec::new();
        write_sub_chunks(&self.sub_chunks, block_layout, &mut content)?;
        let size = content.len() as u32;
        write_tag_chunk_header(writer, u32::from_be_bytes(*b"tgst"), size, size)?;
        writer.write_all(&content)?;
        Ok(())
    }

    /// Parse a single field's value.
    ///
    /// `struct_raw` is the slice of the enclosing block's `raw_data`
    /// that covers exactly this struct's bytes — typically obtained
    /// via [`crate::path::lookup`] or a caller-computed offset. For
    /// sub-chunk leaf fields (string_id / tag_reference / data),
    /// walks `self.sub_chunks` to find the matching payload.
    pub fn parse_field(
        &self,
        layout: &TagLayout,
        struct_raw: &[u8],
        field_index: usize,
    ) -> Option<TagFieldData> {
        let field = &layout.fields[field_index];
        let sub_chunk = self
            .sub_chunks
            .iter()
            .find(|entry| entry.field_index == Some(field_index as u32))
            .map(|entry| &entry.content);
        deserialize_field(layout, field, struct_raw, sub_chunk)
    }

    /// Write `value` back to this struct.
    ///
    /// Primitive, enum/flag, and math values mutate `struct_raw` at
    /// the field's offset. Sub-chunk leaf values swap the matching
    /// `TagSubChunkEntry.content`; that entry is expected to exist
    /// already (set on read or via `new_default`).
    pub fn set_field(
        &mut self,
        layout: &TagLayout,
        struct_raw: &mut [u8],
        field_index: usize,
        value: TagFieldData,
    ) {
        let field = &layout.fields[field_index];
        if let Some(new_content) = serialize_field(field, &value, struct_raw) {
            let entry = self
                .sub_chunks
                .iter_mut()
                .find(|entry| entry.field_index == Some(field_index as u32))
                .expect("set_field: sub-chunk entry missing for sub-chunk-bearing field");
            entry.content = new_content;
        }
    }

    /// Build a struct tree with default sub_chunks for every
    /// sub-chunk-bearing field. Used by [`TagBlockData::add_element`]
    /// and friends to initialize a new element's struct tree. Does
    /// not allocate any raw bytes — the caller (the block) provides
    /// them by growing its own `raw_data`.
    pub fn new_default(layout: &TagLayout, struct_index: usize) -> Self {
        let struct_definition = &layout.struct_definitions[struct_index];
        let mut sub_chunks = Vec::new();
        let mut field_index = struct_definition.first_field_index as usize;

        loop {
            let field = &layout.fields[field_index];
            if field.field_type == TagFieldType::Terminator {
                break;
            }

            let content: Option<TagSubChunkContent> = match field.field_type {
                TagFieldType::Struct => Some(TagSubChunkContent::Struct(
                    TagStruct::new_default(layout, field.definition as usize),
                )),
                TagFieldType::Block => {
                    let block_definition = &layout.block_definitions[field.definition as usize];
                    Some(TagSubChunkContent::Block(TagBlockData {
                        block_index: block_definition.index,
                        flags: 0,
                        raw_data: Vec::new(),
                        elements: Vec::new(),
                    }))
                }
                TagFieldType::Array => {
                    let array_definition = &layout.array_definitions[field.definition as usize];
                    let mut elements = Vec::with_capacity(array_definition.count as usize);
                    for _ in 0..array_definition.count {
                        elements.push(TagStruct::new_default(
                            layout,
                            array_definition.struct_index as usize,
                        ));
                    }
                    Some(TagSubChunkContent::Array(elements))
                }
                TagFieldType::TagReference => Some(TagSubChunkContent::TagReference(Vec::new())),
                TagFieldType::StringId => Some(TagSubChunkContent::StringId(Vec::new())),
                TagFieldType::OldStringId => Some(TagSubChunkContent::OldStringId(Vec::new())),
                TagFieldType::Data => Some(TagSubChunkContent::Data(Vec::new())),
                TagFieldType::PageableResource => {
                    Some(TagSubChunkContent::Resource(TagResourceChunk::Null))
                }
                _ => None,
            };

            if let Some(content) = content {
                sub_chunks.push(TagSubChunkEntry {
                    field_index: Some(field_index as u32),
                    content,
                });
            }

            field_index += 1;
        }

        Self {
            struct_index: struct_definition.index,
            sub_chunks,
        }
    }

    /// Find the index (into `layout.fields`) of a field in this
    /// struct by name. Case-sensitive. Walks fields starting at
    /// `first_field_index` up to the terminator and returns the
    /// first match. Returns `None` if no such field exists.
    pub fn find_field_by_name(&self, layout: &TagLayout, name: &str) -> Option<usize> {
        let struct_definition = &layout.struct_definitions[self.struct_index as usize];
        let mut field_index = struct_definition.first_field_index as usize;
        loop {
            let field = &layout.fields[field_index];
            if field.field_type == TagFieldType::Terminator {
                return None;
            }
            if layout.get_string(field.name_offset) == Some(name) {
                return Some(field_index);
            }
            field_index += 1;
        }
    }

    /// Iterate the user-addressable field names of this struct:
    /// everything except terminator / pad / useless_pad / skip /
    /// explanation / unknown. Empty names are skipped too.
    pub fn field_names<'a>(
        &'a self,
        layout: &'a TagLayout,
    ) -> impl Iterator<Item = &'a str> + 'a {
        let struct_definition = &layout.struct_definitions[self.struct_index as usize];
        let start = struct_definition.first_field_index as usize;
        layout.fields[start..]
            .iter()
            .take_while(|f| f.field_type != TagFieldType::Terminator)
            .filter(|f| {
                !matches!(
                    f.field_type,
                    TagFieldType::Pad
                        | TagFieldType::UselessPad
                        | TagFieldType::Skip
                        | TagFieldType::Explanation
                        | TagFieldType::Unknown,
                )
            })
            .filter_map(|f| layout.get_string(f.name_offset))
            .filter(|name| !name.is_empty())
    }

    /// Describe a container field's current state, or return `None`
    /// if `field_index` isn't a container field (or its sub-chunk
    /// entry is missing). Used by callers that want a one-line
    /// summary without walking `sub_chunks` themselves.
    pub fn container_kind(
        &self,
        layout: &TagLayout,
        field_index: usize,
    ) -> Option<ContainerKind> {
        let field_type = &layout.fields[field_index].field_type;
        let entry = self
            .sub_chunks
            .iter()
            .find(|e| e.field_index == Some(field_index as u32));
        match (field_type, entry.map(|e| &e.content)) {
            (TagFieldType::Struct, _) => Some(ContainerKind::Struct),
            (TagFieldType::Block, Some(TagSubChunkContent::Block(b))) => {
                Some(ContainerKind::Block { count: b.elements.len() })
            }
            (TagFieldType::Array, Some(TagSubChunkContent::Array(a))) => {
                Some(ContainerKind::Array { count: a.len() })
            }
            (TagFieldType::PageableResource, Some(TagSubChunkContent::Resource(_))) => {
                Some(ContainerKind::PageableResource)
            }
            _ => None,
        }
    }

    /// Step into a nested struct field. Returns `(nested_struct,
    /// nested_raw)` where `nested_raw` is the slice of `element_raw`
    /// covering the nested struct's bytes. Returns `None` if
    /// `field_index` isn't a Struct field or the sub-chunk is
    /// missing.
    pub fn nested_struct<'a>(
        &'a self,
        layout: &TagLayout,
        element_raw: &'a [u8],
        field_index: usize,
    ) -> Option<(&'a TagStruct, &'a [u8])> {
        let field = &layout.fields[field_index];
        if field.field_type != TagFieldType::Struct {
            return None;
        }
        let entry = self
            .sub_chunks
            .iter()
            .find(|e| e.field_index == Some(field_index as u32))?;
        let nested = match &entry.content {
            TagSubChunkContent::Struct(s) => s,
            _ => return None,
        };
        let nested_size = layout.struct_definitions[nested.struct_index as usize].size;
        let offset = field.offset as usize;
        Some((nested, &element_raw[offset..offset + nested_size]))
    }

    /// Step into an array-element field. Returns `(element_struct,
    /// element_raw)`. Returns `None` if `field_index` isn't an Array
    /// field, the sub-chunk is missing, or `i` is out of range.
    pub fn array_element<'a>(
        &'a self,
        layout: &TagLayout,
        element_raw: &'a [u8],
        field_index: usize,
        i: usize,
    ) -> Option<(&'a TagStruct, &'a [u8])> {
        let field = &layout.fields[field_index];
        if field.field_type != TagFieldType::Array {
            return None;
        }
        let entry = self
            .sub_chunks
            .iter()
            .find(|e| e.field_index == Some(field_index as u32))?;
        let elements = match &entry.content {
            TagSubChunkContent::Array(a) => a,
            _ => return None,
        };
        let element = elements.get(i)?;
        let array_def = &layout.array_definitions[field.definition as usize];
        let element_size = layout.struct_definitions[array_def.struct_index as usize].size;
        let start = field.offset as usize + i * element_size;
        Some((element, &element_raw[start..start + element_size]))
    }

    /// Mutable counterpart to [`Self::nested_struct`].
    pub fn nested_struct_mut<'a>(
        &'a mut self,
        layout: &TagLayout,
        element_raw: &'a mut [u8],
        field_index: usize,
    ) -> Option<(&'a mut TagStruct, &'a mut [u8])> {
        let field = &layout.fields[field_index];
        if field.field_type != TagFieldType::Struct {
            return None;
        }
        // Pre-compute sizing before borrowing sub_chunks mutably.
        let offset = field.offset as usize;

        let entry = self
            .sub_chunks
            .iter_mut()
            .find(|e| e.field_index == Some(field_index as u32))?;
        let nested = match &mut entry.content {
            TagSubChunkContent::Struct(s) => s,
            _ => return None,
        };
        let nested_size = layout.struct_definitions[nested.struct_index as usize].size;
        Some((nested, &mut element_raw[offset..offset + nested_size]))
    }

    /// Mutable counterpart to [`Self::array_element`].
    pub fn array_element_mut<'a>(
        &'a mut self,
        layout: &TagLayout,
        element_raw: &'a mut [u8],
        field_index: usize,
        i: usize,
    ) -> Option<(&'a mut TagStruct, &'a mut [u8])> {
        let field = &layout.fields[field_index];
        if field.field_type != TagFieldType::Array {
            return None;
        }
        let array_def = &layout.array_definitions[field.definition as usize];
        let element_size = layout.struct_definitions[array_def.struct_index as usize].size;
        let start = field.offset as usize + i * element_size;

        let entry = self
            .sub_chunks
            .iter_mut()
            .find(|e| e.field_index == Some(field_index as u32))?;
        let elements = match &mut entry.content {
            TagSubChunkContent::Array(a) => a,
            _ => return None,
        };
        let element = elements.get_mut(i)?;
        Some((element, &mut element_raw[start..start + element_size]))
    }
}

/// Walk a struct definition's fields, reading each sub-chunk-producing
/// field's chunk from the stream. Primitive / pad / skip / custom /
/// explanation / terminator fields contribute nothing here — their
/// values live in `raw_data` at the precomputed `field.offset`.
fn read_sub_chunks<R: Seek + Read>(
    block_layout: &TagBlockLayout,
    definition: &TagStructDefinition,
    raw_data: &[u8],
    reader: &mut std::io::BufReader<R>,
) -> Result<Vec<TagSubChunkEntry>, Box<dyn Error>> {
    let mut sub_chunks = Vec::new();
    let mut field_index = definition.first_field_index as usize;

    loop {
        let field = &block_layout.layout.fields[field_index];

        match field.field_type {
            TagFieldType::Terminator => break,

            TagFieldType::Struct => {
                let nested_definition = &block_layout.layout.struct_definitions[field.definition as usize];
                let nested_offset = field.offset as usize;
                let nested_raw = &raw_data[nested_offset..nested_offset + nested_definition.size];

                // Placeholder-skip: MCC may emit size=0 tgst placeholder(s) before
                // the real tgst when the nested struct expects sub-chunks.
                let expected_children = block_layout.layout.get_struct_expected_children(field.definition as usize);

                if expected_children > 0 {
                    loop {
                        let header_offset = reader.stream_position()?;
                        let header = read_tag_chunk_header(reader)?;

                        assert!(
                            header.signature == u32::from_be_bytes(*b"tgst"),
                            "Invalid tag struct header at 0x{:X}",
                            header_offset,
                        );

                        if header.size == 0 {
                            assert_eq!(
                                header.version, 0,
                                "empty placeholder tgst version ({}) != 0 at 0x{:X}",
                                header.version, header_offset,
                            );
                            sub_chunks.push(TagSubChunkEntry {
                                field_index: None,
                                content: TagSubChunkContent::EmptyPlaceholder,
                            });
                            continue;
                        }

                        reader.seek(SeekFrom::Start(header_offset))?;
                        break;
                    }
                }

                let nested = TagStruct::read(block_layout, nested_definition, nested_raw, reader)?;

                sub_chunks.push(TagSubChunkEntry {
                    field_index: Some(field_index as u32),
                    content: TagSubChunkContent::Struct(nested),
                });
            }

            TagFieldType::Array => {
                let array_definition = &block_layout.layout.array_definitions[field.definition as usize];
                let element_definition = &block_layout.layout.struct_definitions[array_definition.struct_index as usize];
                let array_offset = field.offset as usize;

                let mut elements = Vec::with_capacity(array_definition.count as usize);

                for i in 0..array_definition.count as usize {
                    let element_offset = array_offset + i * element_definition.size;
                    let element_raw_data = &raw_data[element_offset..element_offset + element_definition.size];
                    let element_sub_chunks =
                        read_sub_chunks(block_layout, element_definition, element_raw_data, reader)?;

                    elements.push(TagStruct {
                        struct_index: element_definition.index,
                        sub_chunks: element_sub_chunks,
                    });
                }

                sub_chunks.push(TagSubChunkEntry {
                    field_index: Some(field_index as u32),
                    content: TagSubChunkContent::Array(elements),
                });
            }

            TagFieldType::Block => {
                let block_definition = &block_layout.layout.block_definitions[field.definition as usize];
                let block_data = TagBlockData::read(block_layout, block_definition, reader)?;

                sub_chunks.push(TagSubChunkEntry {
                    field_index: Some(field_index as u32),
                    content: TagSubChunkContent::Block(block_data),
                });
            }

            TagFieldType::TagReference => {
                let (version, content) = read_tag_chunk_content(reader, u32::from_be_bytes(*b"tgrf"))?;
                assert_eq!(version, 0, "tgrf version ({}) != 0", version);
                sub_chunks.push(TagSubChunkEntry {
                    field_index: Some(field_index as u32),
                    content: TagSubChunkContent::TagReference(content),
                });
            }

            TagFieldType::StringId => {
                let (version, content) = read_tag_chunk_content(reader, u32::from_be_bytes(*b"tgsi"))?;
                assert_eq!(version, 0, "tgsi (string_id) version ({}) != 0", version);
                sub_chunks.push(TagSubChunkEntry {
                    field_index: Some(field_index as u32),
                    content: TagSubChunkContent::StringId(content),
                });
            }

            TagFieldType::OldStringId => {
                let (version, content) = read_tag_chunk_content(reader, u32::from_be_bytes(*b"tgsi"))?;
                assert_eq!(version, 0, "tgsi (old_string_id) version ({}) != 0", version);
                sub_chunks.push(TagSubChunkEntry {
                    field_index: Some(field_index as u32),
                    content: TagSubChunkContent::OldStringId(content),
                });
            }

            TagFieldType::Data => {
                let (version, content) = read_tag_chunk_content(reader, u32::from_be_bytes(*b"tgda"))?;
                assert_eq!(version, 0, "tgda version ({}) != 0", version);
                sub_chunks.push(TagSubChunkEntry {
                    field_index: Some(field_index as u32),
                    content: TagSubChunkContent::Data(content),
                });
            }

            TagFieldType::PageableResource => {
                let resource_definition = &block_layout.layout.resource_definitions[field.definition as usize];
                let resource_struct_definition =
                    &block_layout.layout.struct_definitions[resource_definition.struct_index as usize];
                let resource_offset = field.offset as usize;
                let resource_raw = &raw_data[resource_offset..resource_offset + 8];

                let outer_header = read_tag_chunk_header(reader)?;
                let outer_content_offset = reader.stream_position()?;

                let resource = match &outer_header.signature.to_be_bytes() {
                    b"tg\0c" => {
                        assert_eq!(outer_header.version, 0, "tg\\0c version ({}) != 0", outer_header.version);
                        TagResourceChunk::Null
                    }

                    b"tgrc" => {
                        assert_eq!(outer_header.version, 0, "tgrc version ({}) != 0", outer_header.version);

                        let tgdt_header = read_tag_chunk_header(reader)?;
                        assert!(
                            tgdt_header.signature == u32::from_be_bytes(*b"tgdt"),
                            "expected inner 'tgdt' chunk in pageable resource, got 0x{:08X}",
                            tgdt_header.signature,
                        );
                        assert_eq!(tgdt_header.version, 0, "inner tgdt version ({}) != 0", tgdt_header.version);

                        let mut exploded = vec![0u8; tgdt_header.size as usize];
                        reader.read_exact(&mut exploded)?;

                        let struct_data = TagStruct::read(
                            block_layout,
                            resource_struct_definition,
                            resource_raw,
                            reader,
                        )?;

                        TagResourceChunk::Exploded { exploded, struct_data }
                    }

                    b"tgxc" => {
                        // HYPOTHESIS: tgxc.version is always 0. Mirrors
                        // the other resource variants; trips if an MCC
                        // later game has a non-zero xsync version.
                        assert_eq!(outer_header.version, 0, "tgxc version ({}) != 0", outer_header.version);
                        let mut payload = vec![0u8; outer_header.size as usize];
                        reader.read_exact(&mut payload)?;
                        TagResourceChunk::Xsync(payload)
                    }

                    signature => panic!(
                        "unhandled pageable resource signature: \"{}\"",
                        str::from_utf8(signature).unwrap_or("<non-utf8>"),
                    ),
                };

                let end_offset = reader.stream_position()?;
                let expected_offset = outer_content_offset + outer_header.size as u64;

                if end_offset != expected_offset {
                    panic!(
                        "failed to read pageable resource: started at 0x{outer_content_offset:X}, \
                         ended at 0x{end_offset:X}, expected 0x{expected_offset:X}"
                    );
                }

                sub_chunks.push(TagSubChunkEntry {
                    field_index: Some(field_index as u32),
                    content: TagSubChunkContent::Resource(resource),
                });
            }

            // Primitives / pad / skip / custom / explanation / useless_pad.
            _ => {
                let field_type = &block_layout.layout.field_types[field.type_index as usize];

                if field_type.needs_sub_chunk != 0 {
                    let name = block_layout.layout.get_string(field_type.name_offset).unwrap();
                    panic!("unhandled sub-chunk-producing field type: \"{name}\"");
                }
            }
        }

        field_index += 1;
    }

    Ok(sub_chunks)
}

/// Serialize a vec of sub-chunk entries in stored order. Mirrors
/// `read_sub_chunks`.
fn write_sub_chunks<W: Write>(
    entries: &[TagSubChunkEntry],
    block_layout: &TagBlockLayout,
    writer: &mut W,
) -> std::io::Result<()> {
    for entry in entries {
        match &entry.content {
            TagSubChunkContent::EmptyPlaceholder => {
                write_tag_chunk_header(writer, u32::from_be_bytes(*b"tgst"), 0, 0)?;
            }

            TagSubChunkContent::Struct(nested_struct_data) => {
                nested_struct_data.write(block_layout, writer)?;
            }

            TagSubChunkContent::Block(nested_block_data) => {
                nested_block_data.write(block_layout, writer)?;
            }

            TagSubChunkContent::Array(elements) => {
                // Array elements have no wrapping tgst; their sub-chunks
                // flow inline into the parent's tgst content.
                for element in elements {
                    write_sub_chunks(&element.sub_chunks, block_layout, writer)?;
                }
            }

            TagSubChunkContent::TagReference(content) => {
                write_tag_chunk_content(writer, u32::from_be_bytes(*b"tgrf"), 0, content)?;
            }

            TagSubChunkContent::StringId(content) => {
                write_tag_chunk_content(writer, u32::from_be_bytes(*b"tgsi"), 0, content)?;
            }

            TagSubChunkContent::OldStringId(content) => {
                write_tag_chunk_content(writer, u32::from_be_bytes(*b"tgsi"), 0, content)?;
            }

            TagSubChunkContent::Data(content) => {
                write_tag_chunk_content(writer, u32::from_be_bytes(*b"tgda"), 0, content)?;
            }

            TagSubChunkContent::Resource(TagResourceChunk::Null) => {
                write_tag_chunk_header(writer, u32::from_be_bytes(*b"tg\0c"), 0, 0)?;
            }

            TagSubChunkContent::Resource(TagResourceChunk::Exploded { exploded, struct_data }) => {
                let mut inner = Vec::new();
                write_tag_chunk_content(&mut inner, u32::from_be_bytes(*b"tgdt"), 0, exploded)?;
                struct_data.write(block_layout, &mut inner)?;
                write_tag_chunk_content(writer, u32::from_be_bytes(*b"tgrc"), 0, &inner)?;
            }

            TagSubChunkContent::Resource(TagResourceChunk::Xsync(payload)) => {
                write_tag_chunk_content(writer, u32::from_be_bytes(*b"tgxc"), 0, payload)?;
            }
        }
    }
    Ok(())
}

/// A `tgbl` chunk: a variable-count array of struct elements.
///
/// `raw_data` is a single concatenated byte buffer of length
/// `elements.len() * element_size`; element `i`'s bytes live at
/// `raw_data[i * element_size..(i + 1) * element_size]`. Nested
/// struct/array fields within an element are offset regions inside
/// this same buffer; nested block fields start fresh buffers in their
/// own `TagBlockData`.
///
/// Two shapes, selected by `flags` bit 0:
/// - **Complex** (bit 0 clear): each element has a `tgst` sub-chunk
///   for its sub-chunk-bearing fields.
/// - **Simple** (bit 0 set, `is_simple_data_type=1` in BCS): element
///   bytes only, no per-element `tgst` and no sub-chunks.
#[derive(Debug, Clone)]
pub struct TagBlockData {
    /// Index into [`TagLayout::block_definitions`].
    pub block_index: u32,
    /// Block flags. Bit 0 toggles simple vs complex shape; other bits
    /// are preserved verbatim for roundtrip.
    pub flags: u32,
    /// Concatenated element bytes. Resized atomically by the block
    /// operations (`add_element`, `insert_at`, `duplicate_at`,
    /// `delete_at`, `clear`).
    pub raw_data: Vec<u8>,
    /// Per-element struct trees. Each element's raw bytes live in
    /// `raw_data` at index `i * element_size`. Simple-block elements
    /// have empty `sub_chunks`.
    pub elements: Vec<TagStruct>,
}

impl TagBlockData {
    /// Parse a `tgbl` chunk. Complex vs simple shape is decided by
    /// `flags` bit 0.
    pub fn read<R: Seek + Read>(
        block_layout: &TagBlockLayout,
        definition: &TagBlockDefinition,
        reader: &mut std::io::BufReader<R>,
    ) -> Result<Self, Box<dyn Error>> {
        let tag_block_header = read_tag_chunk_header(reader)?;
        assert!(tag_block_header.signature == u32::from_be_bytes(*b"tgbl"));
        let tag_block_offset = reader.stream_position()?;
        assert_eq!(
            tag_block_header.version, 0,
            "tgbl version ({}) != 0 at 0x{:X}",
            tag_block_header.version, tag_block_offset - 12,
        );

        let block_element_count = read_u32_le(reader)?;
        let block_flags = read_u32_le(reader)?;

        let struct_definition = &block_layout.layout.struct_definitions[definition.struct_index as usize];
        let element_size = struct_definition.size;

        let mut raw_data = vec![0u8; element_size * block_element_count as usize];
        reader.read_exact(&mut raw_data)?;

        let mut elements = Vec::with_capacity(block_element_count as usize);

        if (block_flags & 1) == 0 {
            // Complex block: per-element tgst sub-chunks.
            for i in 0..block_element_count {
                let element_offset = i as usize * element_size;
                let element_raw = &raw_data[element_offset..element_offset + element_size];
                elements.push(TagStruct::read(
                    block_layout,
                    struct_definition,
                    element_raw,
                    reader,
                )?);
            }
        } else {
            // Simple block: raw bytes only, no per-element tgst, no sub-chunks.
            for _ in 0..block_element_count {
                elements.push(TagStruct {
                    struct_index: struct_definition.index,
                    sub_chunks: Vec::new(),
                });
            }
        }

        let end_offset = reader.stream_position()?;
        let expected_offset = tag_block_offset + tag_block_header.size as u64;
        if end_offset != expected_offset {
            panic!(
                "failed to read 'tgbl': ended at offset 0x{end_offset:X}, expected 0x{expected_offset:X}"
            );
        }

        Ok(Self {
            block_index: definition.index,
            flags: block_flags,
            raw_data,
            elements,
        })
    }

    /// Write this block as a `tgbl` chunk.
    pub fn write<W: Write>(
        &self,
        block_layout: &TagBlockLayout,
        writer: &mut W,
    ) -> std::io::Result<()> {
        let mut body = Vec::new();
        let element_count = self.elements.len() as u32;
        body.extend_from_slice(&element_count.to_le_bytes());
        body.extend_from_slice(&self.flags.to_le_bytes());
        body.extend_from_slice(&self.raw_data);

        if (self.flags & 1) == 0 {
            for element in &self.elements {
                element.write(block_layout, &mut body)?;
            }
        }

        write_tag_chunk_content(writer, u32::from_be_bytes(*b"tgbl"), 0, &body)?;
        Ok(())
    }

    /// Size of one element's byte region.
    fn element_size(&self, layout: &TagLayout) -> usize {
        let struct_index = layout.block_definitions[self.block_index as usize].struct_index as usize;
        layout.struct_definitions[struct_index].size
    }

    /// Append a fresh zero-initialized element. Grows `raw_data` by
    /// one element_size and pushes a default `TagStruct`. Returns a
    /// mutable reference to the new element.
    pub fn add_element(&mut self, layout: &TagLayout) -> &mut TagStruct {
        let struct_index = layout.block_definitions[self.block_index as usize].struct_index as usize;
        let element_size = layout.struct_definitions[struct_index].size;
        let old_len = self.raw_data.len();
        self.raw_data.resize(old_len + element_size, 0);
        self.elements.push(TagStruct::new_default(layout, struct_index));
        self.elements.last_mut().unwrap()
    }

    /// Insert a fresh zero-initialized element at `index` (shifting
    /// later elements right).
    pub fn insert_at(&mut self, layout: &TagLayout, index: usize) -> &mut TagStruct {
        let struct_index = layout.block_definitions[self.block_index as usize].struct_index as usize;
        let element_size = layout.struct_definitions[struct_index].size;
        let insert_offset = index * element_size;
        self.raw_data.splice(
            insert_offset..insert_offset,
            std::iter::repeat(0).take(element_size),
        );
        self.elements.insert(index, TagStruct::new_default(layout, struct_index));
        &mut self.elements[index]
    }

    /// Deep-copy the element at `index` and insert the copy directly
    /// after it. Returns a mutable reference to the new element.
    pub fn duplicate_at(&mut self, layout: &TagLayout, index: usize) -> &mut TagStruct {
        let element_size = self.element_size(layout);
        let src_offset = index * element_size;
        let copy_bytes: Vec<u8> = self.raw_data[src_offset..src_offset + element_size].to_vec();
        let insert_offset = (index + 1) * element_size;
        self.raw_data.splice(insert_offset..insert_offset, copy_bytes);
        let cloned = self.elements[index].clone();
        self.elements.insert(index + 1, cloned);
        &mut self.elements[index + 1]
    }

    /// Remove the element at `index`. Panics if out of range.
    pub fn delete_at(&mut self, layout: &TagLayout, index: usize) {
        let element_size = self.element_size(layout);
        let start = index * element_size;
        self.raw_data.drain(start..start + element_size);
        self.elements.remove(index);
    }

    /// Remove all elements.
    pub fn clear(&mut self) {
        self.raw_data.clear();
        self.elements.clear();
    }

    /// Slice of `raw_data` covering element `i`'s bytes.
    pub fn element_raw(&self, layout: &TagLayout, i: usize) -> &[u8] {
        let size = self.element_size(layout);
        let start = i * size;
        &self.raw_data[start..start + size]
    }

    /// Mutable counterpart to [`Self::element_raw`].
    pub fn element_raw_mut(&mut self, layout: &TagLayout, i: usize) -> &mut [u8] {
        let size = self.element_size(layout);
        let start = i * size;
        &mut self.raw_data[start..start + size]
    }

    /// Iterate `(raw_slice, struct_ref)` pairs for every element in
    /// order. Each raw slice is the element's region within
    /// `self.raw_data`. Cheap — no allocation, just offset walking.
    pub fn iter_elements<'a>(
        &'a self,
        layout: &'a TagLayout,
    ) -> impl Iterator<Item = (&'a [u8], &'a TagStruct)> + 'a {
        let element_size = self.element_size(layout);
        self.elements.iter().enumerate().map(move |(i, element)| {
            let start = i * element_size;
            (&self.raw_data[start..start + element_size], element)
        })
    }
}
