//! Façade API — concept-oriented access on top of the structural
//! types in [`crate::data`] / [`crate::layout`] / [`crate::path`].
//!
//! **Status: design sketch.** Every body is `todo!()`. Not yet wired
//! into `lib.rs`. The goal here is to pin down the shape (names,
//! lifetimes, which operations belong where) before writing impls.
//!
//! ## CLI before / after
//!
//! ```text
//! // get.rs — read a field value
//! - let tag = TagFile::read(file)?;
//! - let layout = &tag.tag_stream.layout;
//! - let cursor = lookup(layout, &tag.tag_stream.data, path)?;
//! - let value = cursor.parse(layout)?;
//! + let tag   = TagFile::read(file)?;
//! + let field = tag.root().field_path(path)?;
//! + let value = field.value()?;
//!
//! // flag.rs — toggle a flag bit
//! - let mut cursor = lookup_mut(layout, &mut tag_stream.data, path)?;
//! - let bit        = find_flag_bit(layout, field, flag_name)?;
//! - let mut parsed = cursor.parse(layout)?;
//! - let current    = parsed.flag_bit(bit)?;
//! - parsed.set_flag_bit(bit, !current);
//! - cursor.set(layout, parsed);
//! + let mut flag = tag.root_mut().field_path_mut(path)?.flag_mut(flag_name)?;
//! + flag.toggle();
//!
//! // block.rs — delete element N
//! - let entry = cursor.struct_data.sub_chunks.iter_mut()
//! -     .find(|e| e.field_index == Some(cursor.field_index as u32))?;
//! - let TagSubChunkContent::Block(block) = &mut entry.content else { ... };
//! - block.delete_at(layout, idx);
//! + tag.root_mut().field_path_mut(path)?.as_block_mut()?.delete(idx)?;
//! ```

use crate::data::{TagBlockData, TagResourceChunk, TagStructData, TagSubChunkContent};
use crate::fields::{
    field_option_names, find_enum_option_index, find_flag_bit, parse_group_tag,
    StringIdData, TagFieldData, TagFieldType, TagReferenceData,
};
use crate::file::TagFile;
use crate::layout::TagLayout;

//================================================================================
// Tag-level entry points
//================================================================================

impl TagFile {
    /// What kind of tag this is — group tag and group version.
    pub fn group(&self) -> TagGroup {
        TagGroup {
            tag: self.header.group_tag,
            version: self.header.group_version,
        }
    }

    /// The tag's root element — the first (and only) element of the
    /// `tag!` stream's root block.
    pub fn root(&self) -> TagStruct<'_> {
        stream_root(&self.tag_stream).expect("tag has no root element")
    }

    /// Mutable counterpart of [`TagFile::root`].
    pub fn root_mut(&mut self) -> TagStructMut<'_> {
        stream_root_mut(&mut self.tag_stream).expect("tag has no root element")
    }

    /// Root element of the `want` stream — the dependency list — if
    /// this tag has one. Most tags do.
    pub fn dependency_list(&self) -> Option<TagStruct<'_>> {
        stream_root(self.dependency_list_stream.as_ref()?)
    }

    /// Mutable counterpart of [`TagFile::dependency_list`].
    pub fn dependency_list_mut(&mut self) -> Option<TagStructMut<'_>> {
        stream_root_mut(self.dependency_list_stream.as_mut()?)
    }

    /// Root element of the `info` stream — import / source metadata —
    /// if this tag has one.
    pub fn import_info(&self) -> Option<TagStruct<'_>> {
        stream_root(self.import_info_stream.as_ref()?)
    }

    /// Mutable counterpart of [`TagFile::import_info`].
    pub fn import_info_mut(&mut self) -> Option<TagStructMut<'_>> {
        stream_root_mut(self.import_info_stream.as_mut()?)
    }
}

fn stream_root(stream: &crate::stream::TagStream) -> Option<TagStruct<'_>> {
    let layout = &stream.layout;
    let block = &stream.data;
    let struct_data = block.elements.first()?;
    let struct_raw = block.element_raw(layout, 0);
    Some(TagStruct { layout, struct_data, struct_raw })
}

fn stream_root_mut(stream: &mut crate::stream::TagStream) -> Option<TagStructMut<'_>> {
    let layout = &stream.layout;
    let block = &mut stream.data;

    // Inline the element-size math so we can disjoint-split `block`
    // into its `elements` and `raw_data` fields below.
    let struct_index = layout.block_layouts[block.block_index as usize].struct_index as usize;
    let size = layout.struct_layouts[struct_index].size;

    let struct_data = block.elements.first_mut()?;
    let struct_raw = &mut block.raw_data[0..size];
    Some(TagStructMut { layout, struct_data, struct_raw })
}

/// What kind of tag this is: the 4-byte group tag (e.g. `b"scnr"`)
/// plus its group version. For the authoring-toolset build, format
/// version, and checksum, read [`crate::file::TagFileHeader`]
/// directly via `tag.header`.
///
/// `Display` renders the group tag in its ASCII form with trailing
/// NULs and spaces stripped (e.g. `b"scnr"` → `"scnr"`, `b"mo  "` →
/// `"mo"`). Matches [`crate::fields::format_group_tag`].
#[derive(Debug, Clone, Copy)]
pub struct TagGroup {
    /// BE-packed 4-byte group tag — same representation as on disk.
    pub tag: u32,
    pub version: u32,
}

impl std::fmt::Display for TagGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&crate::fields::format_group_tag(self.tag))
    }
}

//================================================================================
// Read-side: TagStruct / TagField and their typed views
//================================================================================

/// A struct instance — the unit that fields hang off of. The root
/// element, a block's element, an array's element, and a nested
/// struct field all map to this same type.
///
/// Cheap to copy (three references); pass by value freely.
#[derive(Clone, Copy)]
pub struct TagStruct<'a> {
    layout: &'a TagLayout,
    struct_data: &'a TagStructData,
    struct_raw: &'a [u8],
}

impl<'a> TagStruct<'a> {
    /// The schema side of this instance — the struct definition it
    /// conforms to. Bridges to the [`crate::definition`] façade.
    pub fn definition(&self) -> crate::TagStructDefinition<'a> {
        crate::TagStructDefinition::new(self.layout, self.struct_data.struct_index as usize)
    }

    /// The struct type's display name (e.g. `"biped"`).
    pub fn name(&self) -> &'a str {
        let definition = &self.layout.struct_layouts[self.struct_data.struct_index as usize];
        self.layout.get_string(definition.name_offset).unwrap_or("")
    }

    /// Size in bytes of one instance of this struct.
    pub fn size(&self) -> usize {
        self.layout.struct_layouts[self.struct_data.struct_index as usize].size
    }

    /// Walk the struct's fields in declaration order. Skips padding,
    /// explanations, terminators, and unknown types.
    pub fn fields(&self) -> impl Iterator<Item = TagField<'a>> + 'a {
        let TagStruct { layout, struct_data, struct_raw } = *self;
        let definition = &layout.struct_layouts[struct_data.struct_index as usize];
        let start = definition.first_field_index as usize;
        (start..)
            .take_while(move |&i| layout.fields[i].field_type != TagFieldType::Terminator)
            .filter(move |&i| !matches!(
                layout.fields[i].field_type,
                TagFieldType::Pad | TagFieldType::UselessPad | TagFieldType::Skip
                    | TagFieldType::Explanation | TagFieldType::Unknown,
            ))
            .map(move |i| TagField { layout, struct_data, struct_raw, field_index: i })
    }

    /// Walk every field, including padding / skip / explanation /
    /// unknown fields. Intended for layout investigation tooling
    /// (e.g. `inspect --all`). Normal consumers should use
    /// [`TagStruct::fields`] which filters these out.
    pub fn fields_all(&self) -> impl Iterator<Item = TagField<'a>> + 'a {
        let TagStruct { layout, struct_data, struct_raw } = *self;
        let definition = &layout.struct_layouts[struct_data.struct_index as usize];
        let start = definition.first_field_index as usize;
        (start..)
            .take_while(move |&i| layout.fields[i].field_type != TagFieldType::Terminator)
            .map(move |i| TagField { layout, struct_data, struct_raw, field_index: i })
    }

    /// User-addressable field names in declaration order. Mirrors
    /// [`TagStructData::field_names`] — used by the CLI's "did you
    /// mean?" path.
    pub fn field_names(&self) -> impl Iterator<Item = &'a str> + 'a {
        self.struct_data.field_names(self.layout)
    }

    /// Resolve a single field by name (case-sensitive, no path
    /// descent). Use [`TagStruct::field_path`] for paths like
    /// `"unit/seats[0]/flags"`.
    pub fn field(&self, name: &str) -> Option<TagField<'a>> {
        let field_index = self.struct_data.find_field_by_name(self.layout, name)?;
        Some(TagField {
            layout: self.layout,
            struct_data: self.struct_data,
            struct_raw: self.struct_raw,
            field_index,
        })
    }

    /// Walk `path` treating every `/`-separated segment as an
    /// intermediate descent into a struct / block element / array
    /// element — like [`TagStruct::field_path`] but with no
    /// "terminal field lookup" step. Returns the struct at the end
    /// of the walk.
    ///
    /// Use this when you want to reach a struct itself (e.g. to walk
    /// everything underneath it), not a specific field. Paths like
    /// `"unit/seats[2]/variants[0]"` land inside the 0th variant of
    /// the 2nd seat.
    pub fn descend(&self, path: &str) -> Option<TagStruct<'a>> {
        let (struct_data, struct_raw) = crate::path::descend_from_struct(
            self.layout, self.struct_data, self.struct_raw, path,
        )?;
        Some(TagStruct { layout: self.layout, struct_data, struct_raw })
    }

    /// Resolve a `/`-separated field path. Accepts optional
    /// `Type:name` filter and `[N]` block/array index per segment —
    /// same grammar as [`crate::path::lookup`].
    pub fn field_path(&self, path: &str) -> Option<TagField<'a>> {
        let cursor = crate::path::lookup_from_struct(
            self.layout, self.struct_data, self.struct_raw, path,
        )?;
        Some(TagField {
            layout: self.layout,
            struct_data: cursor.struct_data,
            struct_raw: cursor.struct_raw,
            field_index: cursor.field_index,
        })
    }

    /// Closest-match suggestion for an unresolved field name, for
    /// "did you mean?" UX. Returns `None` when nothing is close
    /// enough (distance > `typed.len() / 2 + 1`, matching the
    /// existing CLI heuristic).
    pub fn suggest_field_name(&self, typed: &str) -> Option<&'a str> {
        let typed_lower = typed.to_lowercase();
        let mut best: Option<(usize, &'a str)> = None;
        for candidate in self.field_names() {
            let distance = edit_distance(&typed_lower, &candidate.to_lowercase());
            match best {
                Some((d, _)) if distance >= d => {}
                _ => best = Some((distance, candidate)),
            }
        }
        best.filter(|(d, _)| *d <= typed.len() / 2 + 1).map(|(_, s)| s)
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 0..=m { dp[i][0] = i; }
    for j in 0..=n { dp[0][j] = j; }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1).min(dp[i][j - 1] + 1).min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[m][n]
}

/// A resolved field within a [`TagStruct`]. Carries the field's
/// schema, current value (for scalar fields), and — for container
/// fields — a way to step into the sub-tree without ever touching
/// `sub_chunks` directly.
///
/// Cheap to copy; pass by value freely.
#[derive(Clone, Copy)]
pub struct TagField<'a> {
    layout: &'a TagLayout,
    struct_data: &'a TagStructData,
    struct_raw: &'a [u8],
    field_index: usize,
}

impl<'a> TagField<'a> {
    /// The schema side of this field — its definition in the layout.
    /// Bridges to the [`crate::definition`] façade.
    pub fn definition(&self) -> crate::TagFieldDefinition<'a> {
        crate::TagFieldDefinition::new(self.layout, self.field_index)
    }

    pub fn name(&self) -> &'a str {
        let field = &self.layout.fields[self.field_index];
        self.layout.get_string(field.name_offset).unwrap_or("")
    }

    /// The schema type's display name (e.g. `"short_integer"`).
    pub fn type_name(&self) -> &'a str {
        let field = &self.layout.fields[self.field_index];
        let type_name_offset = self.layout.field_types[field.type_index as usize].name_offset;
        self.layout.get_string(type_name_offset).unwrap_or("")
    }

    /// The field's schema type — callers dispatch on this when they
    /// need to know exactly what kind of field this is.
    pub fn field_type(&self) -> TagFieldType {
        self.layout.fields[self.field_index].field_type
    }

    /// The field's current value. `None` for container and padding
    /// fields — use [`TagField::as_struct`] / [`TagField::as_block`]
    /// / [`TagField::as_array`] / [`TagField::as_resource`] to step
    /// into containers.
    pub fn value(&self) -> Option<TagFieldData> {
        self.struct_data.parse_field(self.layout, self.struct_raw, self.field_index)
    }

    /// Typed step-in accessors for container fields. Each returns
    /// `None` either when this field isn't that specific container
    /// shape OR when the schema says it is but the sub-chunk is
    /// missing on this tag instance. Real-world tags ship with
    /// null-sized `tgst` chunks whose array / block / struct fields
    /// have no corresponding entries; callers walking many tags
    /// shouldn't crash on that.
    pub fn as_struct(&self) -> Option<TagStruct<'a>> {
        if self.layout.fields[self.field_index].field_type != TagFieldType::Struct {
            return None;
        }
        let (struct_data, struct_raw) = self
            .struct_data
            .nested_struct(self.layout, self.struct_raw, self.field_index)?;
        Some(TagStruct { layout: self.layout, struct_data, struct_raw })
    }

    pub fn as_block(&self) -> Option<TagBlock<'a>> {
        if self.layout.fields[self.field_index].field_type != TagFieldType::Block {
            return None;
        }
        let block_data = self.sub_chunk().and_then(|c| match c {
            TagSubChunkContent::Block(b) => Some(b),
            _ => None,
        })?;
        Some(TagBlock { layout: self.layout, block_data })
    }

    pub fn as_array(&self) -> Option<TagArray<'a>> {
        let field = &self.layout.fields[self.field_index];
        if field.field_type != TagFieldType::Array {
            return None;
        }
        let elements = self.sub_chunk().and_then(|c| match c {
            TagSubChunkContent::Array(elements) => Some(elements.as_slice()),
            _ => None,
        })?;
        let array_layout_index = field.definition;
        let array_def = &self.layout.array_layouts[array_layout_index as usize];
        let element_size = self.layout.struct_layouts[array_def.struct_index as usize].size;
        let start = field.offset as usize;
        let array_raw = &self.struct_raw[start..start + elements.len() * element_size];
        Some(TagArray {
            layout: self.layout,
            array_layout_index,
            array_raw,
            elements,
        })
    }

    pub fn as_resource(&self) -> Option<TagResource<'a>> {
        if self.layout.fields[self.field_index].field_type != TagFieldType::PageableResource {
            return None;
        }
        let chunk = self
            .sub_chunk()
            .and_then(|c| match c {
                TagSubChunkContent::Resource(r) => Some(r),
                _ => None,
            })?;
        Some(TagResource { chunk })
    }

    /// For enum or flags fields: variant / bit names plus current
    /// state. Returns `None` for other field types.
    pub fn options(&self) -> Option<TagOptions<'a>> {
        let field = &self.layout.fields[self.field_index];
        let is_enum = matches!(
            field.field_type,
            TagFieldType::CharEnum | TagFieldType::ShortEnum | TagFieldType::LongEnum,
        );
        let is_flags = matches!(
            field.field_type,
            TagFieldType::ByteFlags | TagFieldType::WordFlags | TagFieldType::LongFlags,
        );
        if !is_enum && !is_flags {
            return None;
        }

        let names: Vec<&'a str> = field_option_names(self.layout, field).collect();
        let value = self.value();

        if is_enum {
            let current = value.and_then(|v| match v {
                TagFieldData::CharEnum { value, .. } => Some(value as i64),
                TagFieldData::ShortEnum { value, .. } => Some(value as i64),
                TagFieldData::LongEnum { value, .. } => Some(value as i64),
                _ => None,
            });
            Some(TagOptions::Enum { names, current })
        } else {
            let items = names
                .iter()
                .enumerate()
                .map(|(bit, &name)| {
                    let is_set = value.as_ref().and_then(|v| v.flag_bit(bit as u32)).unwrap_or(false);
                    TagFlagOption { name, bit: bit as u32, is_set }
                })
                .collect();
            Some(TagOptions::Flags(items))
        }
    }

    /// Look up a single flag by name on a flags-typed field.
    pub fn flag(&self, name: &str) -> Option<TagFlag<'a>> {
        let field = &self.layout.fields[self.field_index];
        let bit = find_flag_bit(self.layout, field, name)?;
        Some(TagFlag { field: *self, bit })
    }

    /// Parse a CLI-flavored string into a [`TagFieldData`] matching
    /// this field's schema type — without mutating anything. Same
    /// parser as [`TagFieldMut::parse_and_set`]; pull it out
    /// separately when you want validation or preview without
    /// committing (e.g. `set --dry-run`).
    pub fn parse(&self, input: &str) -> Result<TagFieldData, TagSetError> {
        parse_value(self.layout, self.field_index, input)
    }

    /// The sub-chunk content entry (if any) owned by this field —
    /// shared plumbing for [`TagField::as_block`] / `as_array` /
    /// `as_resource` and the string-id / tag-reference / data leaf
    /// variants that also live under this field's sub-chunk.
    fn sub_chunk(&self) -> Option<&'a TagSubChunkContent> {
        self.struct_data
            .sub_chunks
            .iter()
            .find(|e| e.field_index == Some(self.field_index as u32))
            .map(|e| &e.content)
    }
}

/// A variable-count block of same-typed elements. Byte-ownership
/// boundary — a block carries its own `raw_data`.
#[derive(Clone, Copy)]
pub struct TagBlock<'a> {
    layout: &'a TagLayout,
    block_data: &'a TagBlockData,
}

impl<'a> TagBlock<'a> {
    /// The schema side of this block — its block definition.
    /// Bridges to the [`crate::definition`] façade.
    pub fn definition(&self) -> crate::TagBlockDefinition<'a> {
        crate::TagBlockDefinition::new(self.layout, self.block_data.block_index as usize)
    }

    pub fn len(&self) -> usize { self.block_data.elements.len() }
    pub fn is_empty(&self) -> bool { self.block_data.elements.is_empty() }

    pub fn element(&self, index: usize) -> Option<TagStruct<'a>> {
        let struct_data = self.block_data.elements.get(index)?;
        let size = block_element_size(self.layout, self.block_data);
        let start = index * size;
        let struct_raw = &self.block_data.raw_data[start..start + size];
        Some(TagStruct { layout: self.layout, struct_data, struct_raw })
    }

    pub fn iter(&self) -> impl Iterator<Item = TagStruct<'a>> + 'a {
        let TagBlock { layout, block_data } = *self;
        block_data.iter_elements(layout).map(move |(struct_raw, struct_data)| {
            TagStruct { layout, struct_data, struct_raw }
        })
    }
}

fn block_element_size(layout: &TagLayout, block_data: &TagBlockData) -> usize {
    let struct_index = layout.block_layouts[block_data.block_index as usize].struct_index as usize;
    layout.struct_layouts[struct_index].size
}

/// A fixed-count inline array. Count is schema-declared; elements'
/// bytes live contiguously in `array_raw` (a slice of the enclosing
/// struct's raw region starting at the array field's offset).
#[derive(Clone, Copy)]
pub struct TagArray<'a> {
    layout: &'a TagLayout,
    array_layout_index: u32,
    array_raw: &'a [u8],
    elements: &'a [TagStructData],
}

impl<'a> TagArray<'a> {
    /// The schema side of this array — its array definition.
    /// Bridges to the [`crate::definition`] façade.
    pub fn definition(&self) -> crate::TagArrayDefinition<'a> {
        crate::TagArrayDefinition::new(self.layout, self.array_layout_index as usize)
    }

    pub fn len(&self) -> usize { self.elements.len() }
    pub fn is_empty(&self) -> bool { self.elements.is_empty() }

    pub fn element(&self, index: usize) -> Option<TagStruct<'a>> {
        let struct_data = self.elements.get(index)?;
        let size = self.layout.struct_layouts[self.element_struct_index() as usize].size;
        let start = index * size;
        let struct_raw = &self.array_raw[start..start + size];
        Some(TagStruct { layout: self.layout, struct_data, struct_raw })
    }

    pub fn iter(&self) -> impl Iterator<Item = TagStruct<'a>> + 'a {
        let TagArray { layout, array_layout_index, array_raw, elements } = *self;
        let size = element_struct_size(layout, array_layout_index);
        elements.iter().enumerate().map(move |(i, struct_data)| {
            let start = i * size;
            TagStruct {
                layout,
                struct_data,
                struct_raw: &array_raw[start..start + size],
            }
        })
    }

    fn element_struct_index(&self) -> u32 {
        self.layout.array_layouts[self.array_layout_index as usize].struct_index
    }
}

fn element_struct_size(layout: &TagLayout, array_layout_index: u32) -> usize {
    let element_struct_index =
        layout.array_layouts[array_layout_index as usize].struct_index as usize;
    layout.struct_layouts[element_struct_index].size
}

/// Read-only view onto a pageable resource field. Exposes shape
/// discriminant only — payload bytes are intentionally not surfaced
/// at the façade level (they're opaque engine data).
#[derive(Clone, Copy)]
pub struct TagResource<'a> {
    chunk: &'a TagResourceChunk,
}

impl<'a> TagResource<'a> {
    pub fn kind(&self) -> TagResourceKind {
        match self.chunk {
            TagResourceChunk::Null => TagResourceKind::Null,
            TagResourceChunk::Exploded { .. } => TagResourceKind::Exploded,
            TagResourceChunk::Xsync(_) => TagResourceKind::Xsync,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TagResourceKind {
    Null,
    Exploded,
    Xsync,
}

/// Enum or flags option set, as surfaced to the CLI `options`
/// command and to "did you mean?" value parsing.
pub enum TagOptions<'a> {
    Enum { names: Vec<&'a str>, current: Option<i64> },
    Flags(Vec<TagFlagOption<'a>>),
}

#[derive(Debug, Clone, Copy)]
pub struct TagFlagOption<'a> {
    pub name: &'a str,
    pub bit: u32,
    pub is_set: bool,
}

/// A single flag bit addressed by name.
pub struct TagFlag<'a> {
    field: TagField<'a>,
    bit: u32,
}

impl<'a> TagFlag<'a> {
    pub fn name(&self) -> &'a str { self.field.flag_from_bit(self.bit) }
    pub fn bit(&self) -> u32 { self.bit }
    pub fn is_set(&self) -> bool {
        self.field.value().and_then(|v| v.flag_bit(self.bit)).unwrap_or(false)
    }
}

//================================================================================
// Write-side: mirrors of the read types
//================================================================================

/// Mutable counterpart of [`TagStruct`].
pub struct TagStructMut<'a> {
    layout: &'a TagLayout,
    struct_data: &'a mut TagStructData,
    struct_raw: &'a mut [u8],
}

impl<'a> TagStructMut<'a> {
    /// Re-borrow as a read-only [`TagStruct`] for inspection.
    pub fn as_ref(&self) -> TagStruct<'_> {
        TagStruct {
            layout: self.layout,
            struct_data: &*self.struct_data,
            struct_raw: &*self.struct_raw,
        }
    }

    /// Resolve a single field by name (case-sensitive, no path
    /// descent).
    pub fn field_mut(&mut self, name: &str) -> Option<TagFieldMut<'_>> {
        let field_index = self.struct_data.find_field_by_name(self.layout, name)?;
        Some(TagFieldMut {
            layout: self.layout,
            struct_data: &mut *self.struct_data,
            struct_raw: &mut *self.struct_raw,
            field_index,
        })
    }

    /// Resolve a `/`-separated field path. Mirrors
    /// [`TagStruct::field_path`].
    pub fn field_path_mut(&mut self, path: &str) -> Option<TagFieldMut<'_>> {
        let cursor = crate::path::lookup_mut_from_struct(
            self.layout, &mut *self.struct_data, &mut *self.struct_raw, path,
        )?;
        Some(TagFieldMut {
            layout: self.layout,
            struct_data: cursor.struct_data,
            struct_raw: cursor.struct_raw,
            field_index: cursor.field_index,
        })
    }

    /// Walk the struct's fields in declaration order, yielding a
    /// mutable handle for each. Mirrors [`TagStruct::fields`]'s
    /// filtering (skips padding, explanations, terminators, unknown).
    ///
    /// Uses a visitor closure rather than returning an iterator
    /// because each yielded [`TagFieldMut`] reborrows through `self`
    /// — Rust's borrow checker rules out giving out multiple
    /// simultaneous `&mut` iterators.
    pub fn for_each_field_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(TagFieldMut<'_>),
    {
        let layout = self.layout;
        let struct_index = self.struct_data.struct_index as usize;
        let start = layout.struct_layouts[struct_index].first_field_index as usize;

        let mut i = start;
        loop {
            let ft = layout.fields[i].field_type;
            if ft == TagFieldType::Terminator {
                break;
            }
            let is_padding = matches!(
                ft,
                TagFieldType::Pad | TagFieldType::UselessPad | TagFieldType::Skip
                    | TagFieldType::Explanation | TagFieldType::Unknown,
            );
            if !is_padding {
                f(TagFieldMut {
                    layout,
                    struct_data: &mut *self.struct_data,
                    struct_raw: &mut *self.struct_raw,
                    field_index: i,
                });
            }
            i += 1;
        }
    }
}

/// Mutable counterpart of [`TagField`].
pub struct TagFieldMut<'a> {
    layout: &'a TagLayout,
    struct_data: &'a mut TagStructData,
    struct_raw: &'a mut [u8],
    field_index: usize,
}

impl<'a> TagFieldMut<'a> {
    pub fn as_ref(&self) -> TagField<'_> {
        TagField {
            layout: self.layout,
            struct_data: &*self.struct_data,
            struct_raw: &*self.struct_raw,
            field_index: self.field_index,
        }
    }

    /// Write `value`. Returns [`TagSetError::NotAssignable`] for
    /// container fields (struct/block/array/pageable_resource) —
    /// those must be mutated via [`TagFieldMut::as_struct_mut`] /
    /// `as_block_mut` / `as_array_mut`.
    pub fn set(&mut self, value: TagFieldData) -> Result<(), TagSetError> {
        let ft = self.layout.fields[self.field_index].field_type;
        if matches!(
            ft,
            TagFieldType::Struct
                | TagFieldType::Block
                | TagFieldType::Array
                | TagFieldType::PageableResource,
        ) {
            return Err(TagSetError::NotAssignable);
        }
        self.struct_data.set_field(self.layout, &mut *self.struct_raw, self.field_index, value);
        Ok(())
    }

    /// CLI-flavored convenience: parse `input` against this field's
    /// schema (integer / enum variant name / group-tag / etc.) and
    /// write it. Use [`TagField::parse`] on the immutable handle if
    /// you only want to validate without committing.
    pub fn parse_and_set(&mut self, input: &str) -> Result<(), TagSetError> {
        let value = self.as_ref().parse(input)?;
        self.set(value)
    }

    /// Look up a single flag by name and return a mutable handle.
    pub fn flag_mut(&mut self, name: &str) -> Option<TagFlagMut<'_>> {
        let field = &self.layout.fields[self.field_index];
        let bit = find_flag_bit(self.layout, field, name)?;
        Some(TagFlagMut {
            field: TagFieldMut {
                layout: self.layout,
                struct_data: &mut *self.struct_data,
                struct_raw: &mut *self.struct_raw,
                field_index: self.field_index,
            },
            bit,
        })
    }

    /// Same shape-vs-missing distinction as [`TagField::as_struct`] —
    /// Returns `None` either when this isn't a struct field OR when
    /// its sub-chunk is missing on the loaded tag.
    pub fn as_struct_mut(&mut self) -> Option<TagStructMut<'_>> {
        if self.layout.fields[self.field_index].field_type != TagFieldType::Struct {
            return None;
        }
        let field_index = self.field_index;
        let (struct_data, struct_raw) = self
            .struct_data
            .nested_struct_mut(self.layout, &mut *self.struct_raw, field_index)?;
        Some(TagStructMut { layout: self.layout, struct_data, struct_raw })
    }

    pub fn as_block_mut(&mut self) -> Option<TagBlockMut<'_>> {
        if self.layout.fields[self.field_index].field_type != TagFieldType::Block {
            return None;
        }
        let field_index = self.field_index;
        let block_data = self
            .struct_data
            .sub_chunks
            .iter_mut()
            .find(|e| e.field_index == Some(field_index as u32))
            .and_then(|e| match &mut e.content {
                TagSubChunkContent::Block(b) => Some(b),
                _ => None,
            })?;
        Some(TagBlockMut { layout: self.layout, block_data })
    }

    pub fn as_array_mut(&mut self) -> Option<TagArrayMut<'_>> {
        let field = &self.layout.fields[self.field_index];
        if field.field_type != TagFieldType::Array {
            return None;
        }
        let array_layout_index = field.definition;
        let array_def = &self.layout.array_layouts[array_layout_index as usize];
        let element_size = self.layout.struct_layouts[array_def.struct_index as usize].size;
        let start = field.offset as usize;

        let field_index = self.field_index;
        let elements = self
            .struct_data
            .sub_chunks
            .iter_mut()
            .find(|e| e.field_index == Some(field_index as u32))
            .and_then(|e| match &mut e.content {
                TagSubChunkContent::Array(elements) => Some(elements.as_mut_slice()),
                _ => None,
            })?;
        let end = start + elements.len() * element_size;
        let array_raw = &mut self.struct_raw[start..end];
        Some(TagArrayMut {
            layout: self.layout,
            array_layout_index,
            array_raw,
            elements,
        })
    }
}

/// Parse `input` as a value for `field_index`'s schema type.
/// Used by [`TagFieldMut::parse_and_set`]; kept private because the
/// mutation path is the only intended entry point for string-based
/// editing.
///
/// Enum fields accept either a variant name (case-insensitive) or a
/// raw integer. Flags fields take an integer mask (decimal or `0x`
/// hex) — use [`TagFieldMut::flag_mut`] to set individual bits by
/// name. Block-index fields additionally accept `"none"` as `-1`.
fn parse_value(
    layout: &TagLayout,
    field_index: usize,
    input: &str,
) -> Result<TagFieldData, TagSetError> {
    let field = &layout.fields[field_index];

    match field.field_type {
        TagFieldType::CharInteger => Ok(TagFieldData::CharInteger(input.parse().map_err(|_| TagSetError::ParseError("expected i8".into()))?)),
        TagFieldType::ShortInteger => Ok(TagFieldData::ShortInteger(input.parse().map_err(|_| TagSetError::ParseError("expected i16".into()))?)),
        TagFieldType::LongInteger => Ok(TagFieldData::LongInteger(input.parse().map_err(|_| TagSetError::ParseError("expected i32".into()))?)),
        TagFieldType::Int64Integer => Ok(TagFieldData::Int64Integer(input.parse().map_err(|_| TagSetError::ParseError("expected i64".into()))?)),
        TagFieldType::Tag => Ok(TagFieldData::Tag(
            parse_group_tag(input).ok_or_else(|| TagSetError::ParseError("group tag must be 1..=4 ASCII chars".into()))?,
        )),

        TagFieldType::Angle => Ok(TagFieldData::Angle(input.parse().map_err(|_| TagSetError::ParseError("expected f32".into()))?)),
        TagFieldType::Real => Ok(TagFieldData::Real(input.parse().map_err(|_| TagSetError::ParseError("expected f32".into()))?)),
        TagFieldType::RealSlider => Ok(TagFieldData::RealSlider(input.parse().map_err(|_| TagSetError::ParseError("expected f32".into()))?)),
        TagFieldType::RealFraction => Ok(TagFieldData::RealFraction(input.parse().map_err(|_| TagSetError::ParseError("expected f32".into()))?)),

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
            value: parse_int_mask(input)? as u8,
            names: Vec::new(),
        }),
        TagFieldType::WordFlags => Ok(TagFieldData::WordFlags {
            value: parse_int_mask(input)? as u16,
            names: Vec::new(),
        }),
        TagFieldType::LongFlags => Ok(TagFieldData::LongFlags {
            value: parse_int_mask(input)? as i32,
            names: Vec::new(),
        }),

        TagFieldType::ByteBlockFlags => Ok(TagFieldData::ByteBlockFlags(parse_int_mask(input)? as u8)),
        TagFieldType::WordBlockFlags => Ok(TagFieldData::WordBlockFlags(parse_int_mask(input)? as u16)),
        TagFieldType::LongBlockFlags => Ok(TagFieldData::LongBlockFlags(parse_int_mask(input)? as i32)),

        TagFieldType::CharBlockIndex => Ok(TagFieldData::CharBlockIndex(parse_block_index(input)? as i8)),
        TagFieldType::CustomCharBlockIndex => Ok(TagFieldData::CustomCharBlockIndex(parse_block_index(input)? as i8)),
        TagFieldType::ShortBlockIndex => Ok(TagFieldData::ShortBlockIndex(parse_block_index(input)? as i16)),
        TagFieldType::CustomShortBlockIndex => Ok(TagFieldData::CustomShortBlockIndex(parse_block_index(input)? as i16)),
        TagFieldType::LongBlockIndex => Ok(TagFieldData::LongBlockIndex(parse_block_index(input)?)),
        TagFieldType::CustomLongBlockIndex => Ok(TagFieldData::CustomLongBlockIndex(parse_block_index(input)?)),

        TagFieldType::String => Ok(TagFieldData::String(input.to_string())),
        TagFieldType::LongString => Ok(TagFieldData::LongString(input.to_string())),

        TagFieldType::StringId => Ok(TagFieldData::StringId(StringIdData { string: input.to_string() })),
        TagFieldType::OldStringId => Ok(TagFieldData::OldStringId(StringIdData { string: input.to_string() })),

        TagFieldType::TagReference => Ok(TagFieldData::TagReference(parse_tag_reference(input)?)),

        TagFieldType::Data => Err(TagSetError::ParseError("parsing 'data' fields from a string is not supported".into())),

        TagFieldType::Struct
        | TagFieldType::Block
        | TagFieldType::Array
        | TagFieldType::PageableResource => Err(TagSetError::NotAssignable),

        TagFieldType::ApiInterop => Ok(TagFieldData::ApiInterop(parse_api_interop(input)?)),

        TagFieldType::VertexBuffer => {
            Err(TagSetError::ParseError("parsing vertex_buffer fields is not supported".into()))
        }

        _ => Err(TagSetError::ParseError(format!(
            "parsing field type {:?} from a string is not supported",
            field.field_type,
        ))),
    }
}

fn parse_enum_value(
    layout: &TagLayout,
    field: &crate::layout::TagFieldLayout,
    input: &str,
) -> Result<i32, TagSetError> {
    if let Ok(n) = input.parse::<i32>() {
        return Ok(n);
    }
    if let Some(index) = find_enum_option_index(layout, field, input) {
        return Ok(index as i32);
    }
    Err(TagSetError::ParseError(format!("enum option '{}' not found", input)))
}

fn parse_int_mask(s: &str) -> Result<i64, TagSetError> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).map_err(|_| TagSetError::ParseError("expected hex integer".into()))
    } else {
        s.parse::<i64>().map_err(|_| TagSetError::ParseError("expected integer".into()))
    }
}

fn parse_block_index(s: &str) -> Result<i32, TagSetError> {
    if s.eq_ignore_ascii_case("none") {
        return Ok(-1);
    }
    s.parse().map_err(|_| TagSetError::ParseError("expected integer or 'none'".into()))
}

/// Parse an api_interop payload.
///
/// - `reset` / `none` → BCS's canonical reset pattern
///   (`{ 0, UINT_MAX, 0 }`). The usual way to scrub runtime handles
///   out of a tag before committing it.
/// - `0xDESCRIPTOR,0xADDRESS,0xDEFINITION_ADDRESS` → verbatim triple
///   (each field a 32-bit integer, decimal or `0x` hex).
fn parse_api_interop(s: &str) -> Result<crate::fields::ApiInteropData, TagSetError> {
    let trimmed = s.trim();
    if trimmed.eq_ignore_ascii_case("reset") || trimmed.eq_ignore_ascii_case("none") {
        return Ok(crate::fields::ApiInteropData::reset());
    }

    let parts: Vec<&str> = trimmed.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return Err(TagSetError::ParseError(
            "api_interop format: 'reset', or 'descriptor,address,definition_address' (each u32, decimal or 0x…)".into(),
        ));
    }
    let one = |p: &str| -> Result<u32, TagSetError> {
        let (radix, body) = match p.strip_prefix("0x").or_else(|| p.strip_prefix("0X")) {
            Some(hex) => (16, hex),
            None => (10, p),
        };
        u32::from_str_radix(body, radix)
            .map_err(|_| TagSetError::ParseError("expected u32 (decimal or 0x hex)".into()))
    };
    let descriptor = one(parts[0])?;
    let address = one(parts[1])?;
    let definition_address = one(parts[2])?;

    let mut raw = Vec::with_capacity(12);
    raw.extend_from_slice(&descriptor.to_le_bytes());
    raw.extend_from_slice(&address.to_le_bytes());
    raw.extend_from_slice(&definition_address.to_le_bytes());
    Ok(crate::fields::ApiInteropData { raw })
}

fn parse_tag_reference(s: &str) -> Result<TagReferenceData, TagSetError> {
    if s.eq_ignore_ascii_case("none") || s.is_empty() {
        return Ok(TagReferenceData { group_tag_and_name: None });
    }
    let (group_str, path) = s.split_once(':').ok_or_else(|| {
        TagSetError::ParseError(
            "tag reference format: GROUP:path (e.g. hlmt:objects/characters/elite), or 'none'".into(),
        )
    })?;
    let group_tag = parse_group_tag(group_str)
        .ok_or_else(|| TagSetError::ParseError("group tag must be 1..=4 ASCII chars".into()))?;
    Ok(TagReferenceData { group_tag_and_name: Some((group_tag, path.to_string())) })
}

/// Mutable counterpart of [`TagBlock`]. All structural edits
/// (`add`/`insert`/`delete`/`clear`) funnel through here so callers
/// never touch `TagBlockData` directly.
pub struct TagBlockMut<'a> {
    layout: &'a TagLayout,
    block_data: &'a mut TagBlockData,
}

impl<'a> TagBlockMut<'a> {
    /// The schema side of this block — its block definition.
    /// Bridges to the [`crate::definition`] façade.
    pub fn definition(&self) -> crate::TagBlockDefinition<'_> {
        crate::TagBlockDefinition::new(self.layout, self.block_data.block_index as usize)
    }

    pub fn len(&self) -> usize { self.block_data.elements.len() }
    pub fn is_empty(&self) -> bool { self.block_data.elements.is_empty() }

    pub fn element_mut(&mut self, index: usize) -> Option<TagStructMut<'_>> {
        if index >= self.block_data.elements.len() {
            return None;
        }
        let size = block_element_size(self.layout, &*self.block_data);
        let start = index * size;
        let struct_data = &mut self.block_data.elements[index];
        let struct_raw = &mut self.block_data.raw_data[start..start + size];
        Some(TagStructMut { layout: self.layout, struct_data, struct_raw })
    }

    /// Walk the block's elements in order, yielding a mutable handle
    /// for each. Visitor-closure form for the same borrow-checker
    /// reason as [`TagStructMut::for_each_field_mut`].
    pub fn for_each_element_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(TagStructMut<'_>),
    {
        let layout = self.layout;
        let size = block_element_size(layout, &*self.block_data);
        let count = self.block_data.elements.len();
        for i in 0..count {
            let start = i * size;
            let struct_data = &mut self.block_data.elements[i];
            let struct_raw = &mut self.block_data.raw_data[start..start + size];
            f(TagStructMut { layout, struct_data, struct_raw });
        }
    }

    /// Append a default-initialized element. Returns its new index.
    pub fn add(&mut self) -> usize {
        self.block_data.add_element(self.layout);
        self.block_data.elements.len() - 1
    }

    /// Insert a default element at `index`. Error on out-of-range
    /// (valid range is `0..=len`).
    pub fn insert(&mut self, index: usize) -> Result<(), TagIndexError> {
        let len = self.block_data.elements.len();
        if index > len {
            return Err(TagIndexError::OutOfRange { index, len });
        }
        self.block_data.insert_at(self.layout, index);
        Ok(())
    }

    /// Duplicate element `index`, placing the copy at `index + 1`.
    /// Returns the copy's index.
    pub fn duplicate(&mut self, index: usize) -> Result<usize, TagIndexError> {
        let len = self.block_data.elements.len();
        if index >= len {
            return Err(TagIndexError::OutOfRange { index, len });
        }
        self.block_data.duplicate_at(self.layout, index);
        Ok(index + 1)
    }

    pub fn delete(&mut self, index: usize) -> Result<(), TagIndexError> {
        let len = self.block_data.elements.len();
        if index >= len {
            return Err(TagIndexError::OutOfRange { index, len });
        }
        self.block_data.delete_at(self.layout, index);
        Ok(())
    }

    /// Swap elements at `i` and `j`.
    pub fn swap(&mut self, i: usize, j: usize) -> Result<(), TagIndexError> {
        let len = self.block_data.elements.len();
        if i >= len {
            return Err(TagIndexError::OutOfRange { index: i, len });
        }
        if j >= len {
            return Err(TagIndexError::OutOfRange { index: j, len });
        }
        self.block_data.swap_at(self.layout, i, j);
        Ok(())
    }

    /// Move the element at `from` to final position `to` (Vec::remove
    /// + Vec::insert semantics).
    pub fn move_to(&mut self, from: usize, to: usize) -> Result<(), TagIndexError> {
        let len = self.block_data.elements.len();
        if from >= len {
            return Err(TagIndexError::OutOfRange { index: from, len });
        }
        if to >= len {
            return Err(TagIndexError::OutOfRange { index: to, len });
        }
        self.block_data.move_at(self.layout, from, to);
        Ok(())
    }

    pub fn clear(&mut self) { self.block_data.clear(); }
}

/// Mutable counterpart of [`TagArray`]. Arrays are fixed-count, so no
/// add/remove — only per-element mutation.
pub struct TagArrayMut<'a> {
    layout: &'a TagLayout,
    array_layout_index: u32,
    array_raw: &'a mut [u8],
    elements: &'a mut [TagStructData],
}

impl<'a> TagArrayMut<'a> {
    /// The schema side of this array — its array definition.
    /// Bridges to the [`crate::definition`] façade.
    pub fn definition(&self) -> crate::TagArrayDefinition<'_> {
        crate::TagArrayDefinition::new(self.layout, self.array_layout_index as usize)
    }

    pub fn len(&self) -> usize { self.elements.len() }
    pub fn is_empty(&self) -> bool { self.elements.is_empty() }

    pub fn element_mut(&mut self, index: usize) -> Option<TagStructMut<'_>> {
        if index >= self.elements.len() {
            return None;
        }
        let size = element_struct_size(self.layout, self.array_layout_index);
        let start = index * size;
        let struct_data = &mut self.elements[index];
        let struct_raw = &mut self.array_raw[start..start + size];
        Some(TagStructMut { layout: self.layout, struct_data, struct_raw })
    }

    /// Swap elements at `i` and `j`. Arrays are fixed-count so
    /// reordering is the only structural edit available.
    pub fn swap(&mut self, i: usize, j: usize) -> Result<(), TagIndexError> {
        let len = self.elements.len();
        if i >= len {
            return Err(TagIndexError::OutOfRange { index: i, len });
        }
        if j >= len {
            return Err(TagIndexError::OutOfRange { index: j, len });
        }
        if i == j {
            return Ok(());
        }
        self.elements.swap(i, j);
        let size = element_struct_size(self.layout, self.array_layout_index);
        let (lo, hi) = if i < j { (i, j) } else { (j, i) };
        let lo_start = lo * size;
        let hi_start = hi * size;
        let mut buf = vec![0u8; size];
        buf.copy_from_slice(&self.array_raw[lo_start..lo_start + size]);
        self.array_raw.copy_within(hi_start..hi_start + size, lo_start);
        self.array_raw[hi_start..hi_start + size].copy_from_slice(&buf);
        Ok(())
    }

    /// Walk the array's elements in order, yielding a mutable handle
    /// for each. Visitor-closure form mirroring
    /// [`TagBlockMut::for_each_element_mut`].
    pub fn for_each_element_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(TagStructMut<'_>),
    {
        let layout = self.layout;
        let size = element_struct_size(layout, self.array_layout_index);
        let count = self.elements.len();
        for i in 0..count {
            let start = i * size;
            let struct_data = &mut self.elements[i];
            let struct_raw = &mut self.array_raw[start..start + size];
            f(TagStructMut { layout, struct_data, struct_raw });
        }
    }
}

/// Mutable single-flag handle.
pub struct TagFlagMut<'a> {
    field: TagFieldMut<'a>,
    bit: u32,
}

impl<'a> TagFlagMut<'a> {
    pub fn name(&self) -> &str {
        self.field.as_ref().flag_from_bit(self.bit)
    }

    pub fn bit(&self) -> u32 { self.bit }

    pub fn is_set(&self) -> bool {
        self.field.as_ref().value().and_then(|v| v.flag_bit(self.bit)).unwrap_or(false)
    }

    pub fn set(&mut self, on: bool) {
        let Some(mut value) = self.field.as_ref().value() else { return };
        if value.set_flag_bit(self.bit, on) {
            let _ = self.field.set(value);
        }
    }

    /// Toggle and return the new state.
    pub fn toggle(&mut self) -> bool {
        let new_state = !self.is_set();
        self.set(new_state);
        new_state
    }
}

impl<'a> TagField<'a> {
    /// Resolve bit `bit`'s display name via this field's string list.
    /// Internal helper shared between [`TagFlag::name`] and
    /// [`TagFlagMut::name`].
    fn flag_from_bit(&self, bit: u32) -> &'a str {
        let field = &self.layout.fields[self.field_index];
        let Some(string_list) = self.layout.string_lists.get(field.definition as usize) else {
            return "";
        };
        if bit >= string_list.count {
            return "";
        }
        let offset_index = (string_list.first + bit) as usize;
        let Some(&string_offset) = self.layout.string_offsets.get(offset_index) else {
            return "";
        };
        self.layout.get_string(string_offset).unwrap_or("")
    }
}

//================================================================================
// Errors
//================================================================================

#[derive(Debug)]
pub enum TagSetError {
    /// The supplied [`TagFieldData`] variant doesn't match the
    /// field's schema type.
    TypeMismatch { expected: &'static str, got: &'static str },
    /// [`TagFieldMut::parse_and_set`] couldn't parse `input`. Message
    /// is human-readable (e.g. `"expected i32"`, `"enum option 'foo'
    /// not found"`).
    ParseError(String),
    /// The field is a container — use `as_block_mut()` / etc.
    NotAssignable,
}

#[derive(Debug)]
pub enum TagIndexError {
    OutOfRange { index: usize, len: usize },
}
