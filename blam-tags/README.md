# blam-tags

A Rust library for reading, writing, and editing Halo 3 / Reach tag files.
No ManagedBlam, no .NET, no engine needed — the parser reads each tag's
embedded layout chunk and interprets its bytes directly.

**Byte-exact roundtrip validated across 119,432 tags** from the Halo 3 and
Halo Reach MCC tag files. Read a tag, write it back, md5-compare — zero
differences.

## Quick start

```rust
use blam_tags::file::TagFile;
use blam_tags::path::lookup;

let tag = TagFile::read("path/to/masterchief.biped")?;

// Navigate by /-separated path, read a primitive.
let layout = &tag.tag_stream.layout.layout;
let cursor = lookup(layout, &tag.tag_stream.data, "jump velocity").unwrap();
let value  = cursor.parse(layout).unwrap();
// value is a TagFieldData::Real(2.1)
```

Writing:

```rust
use blam_tags::fields::TagFieldData;
use blam_tags::path::lookup_mut;

let mut tag = TagFile::read("path/to/masterchief.biped")?;
{
    let tag_stream = &mut tag.tag_stream;
    let layout = &tag_stream.layout.layout;
    let mut cursor = lookup_mut(layout, &mut tag_stream.data, "jump velocity").unwrap();
    cursor.set(layout, TagFieldData::Real(3.14));
}
tag.write("path/to/modified.biped")?;
```

## Architecture

Tag files are schema-driven. Every tag carries its own layout
description (`blay` → `tgly` chunks), so the parser is **generic** —
nothing hardcoded per tag type. The library's job is to (a) read the
embedded schema, (b) read the payload bytes into a tree that mirrors
the schema, and (c) write that tree back byte-exact.

The parser keeps data separate from schema:

- **Layout** ([`layout`]) — the schema: struct definitions, field
  definitions, field-type registry, block/resource/interop/array
  definitions, string pool. Immutable at runtime.
- **Data tree** ([`data`]) — per-tag instance data. One
  `TagBlockData` owns the raw bytes of *all* its elements in a single
  buffer; nested structs / inline arrays are offset regions inside
  that buffer; nested blocks open fresh buffers. Matches the on-disk
  `tgbl` layout 1:1.
- **Path navigation** ([`path`]) — `/`-separated path strings with
  `[N]` element indices and optional `Type:` filters. Returns a
  `FieldCursor` (or `FieldCursorMut`) bundling a raw-data slice, the
  containing `TagStruct`, and the final field's index.
- **Field values** ([`fields`]) — `TagFieldData` enum of parsed
  per-field values. Covers primitives, enums+flags (with name
  resolution), math composites (`RealPoint3d` etc.), colors, bounds,
  strings, tag references, and data blobs.

## Module tour

| Module | Purpose |
|---|---|
| [`io`] | Primitive fixed-width integer readers/writers + the 12-byte `TagChunkHeader` helpers. |
| [`math`] | Bounds, colors, vectors, points, euler angles — used by field values. |
| [`fields`] | `TagFieldType` dispatch enum; `TagFieldData` per-field value enum; `deserialize_field` / `serialize_field`; `find_enum_option_index`, `find_flag_bit`, `format_group_tag`, `parse_group_tag`. |
| [`layout`] | `TagLayout` (the schema) + every definition record (`TagStructDefinition`, `TagFieldDefinition`, etc.). V1 (flat `agro`) and V2 / V3 / V4 (`tgly`-wrapped) all parse into the same in-memory shape. |
| [`data`] | `TagStruct` (schema-index + sub-chunks), `TagBlockData` (owns `raw_data` for its elements), `TagSubChunkContent`, `TagResourceChunk`. Methods: `parse_field` / `set_field`, `new_default`, block operations (`add_element` / `insert_at` / `duplicate_at` / `delete_at` / `clear`). |
| [`path`] | `lookup` / `lookup_mut` → `FieldCursor` / `FieldCursorMut`. Immutable descent helpers on `FieldCursor`. |
| [`stream`] | `TagStream` for `tag!` / `want` / `info` chunks (the three top-level stream types in a tag file). |
| [`file`] | `TagFileHeader` (64-byte preamble with `BLAM` signature) and `TagFile` (fully parsed tag file). |

## Field paths

Paths match the shape the CLI uses:

```
"jump velocity"                       — root-level field
"unit/flags"                          — inline struct → field
"unit/seats[0]/flags"                 — struct → block element → field
"regions[2]/permutations[0]/name"    — nested block elements
"Block:regions[0]/name"               — with optional Type: filter
```

Block and array element indices default to `0` on descent if omitted.
Field names are case-sensitive; `Type:` filters are case-insensitive.

## Version coverage

| Format | Read | Write | Notes |
|---|---|---|---|
| V1 layouts (flat `agro` records) | ✓ | ✓ | Reconstructs `stv2` + `blv2` from paired aggregate records on write. |
| V2 layouts (`tgly` with `stv2`) | ✓ | ✓ | Main Halo 3 / Reach format. |
| V3 layouts (adds `]==[` interop) | ✓ | ✓ | Main Halo 3 / Reach format. |
| V4 layouts (`stv4` with per-struct version) | ✓ | ✓ | Not present in the H3/Reach corpus; implemented for forward compatibility with later MCC games. |

Pageable-resource shapes handled: `tg\0c` (null), `tgrc` (exploded with
inner `tgdt` + nested struct), `tgxc` (xsync, opaque payload).
ApiInterop and VertexBuffer fields are preserved as raw bytes through
the roundtrip but not yet parsed into typed values.

## Roundtrip example

The `examples/roundtrip.rs` sweep walks one or more tag root
directories, reads each tag, writes it to a temp file via
`TagFile::write`, and md5-compares source vs temp. Panics on the first
mismatch — used to gate read/write changes.

```sh
# One or more roots; --exclude (or -x) can be repeated to skip
# known-broken tags in a corpus.
cargo run --release -p blam-tags --example roundtrip -- \
    /path/to/halo3_mcc/tags \
    /path/to/haloreach_mcc/tags \
    --exclude /path/to/halo3_mcc/tags/some/broken.tag
```
