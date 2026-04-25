# blam-tags

A Rust library for reading, writing, and editing Halo 3 / Reach tag files.
No ManagedBlam, no .NET, no engine needed — the parser reads each tag's
embedded layout chunk and interprets its bytes directly.

**Byte-exact roundtrip validated across every tag in the Halo 3, Halo 3:
ODST, Halo Reach, Halo 4, and Halo 2: Anniversary MP MCC corpora.** Read a
tag, write it back, md5-compare — zero differences. Locally verified on
the 119,432-tag H3 + Reach subset; full-corpus validation (including H4
and H2A MP) contributed by the community.

## Quick start

```rust
use blam_tags::TagFile;

let mut tag = TagFile::read("masterchief.biped")?;

// Read a field by slash-separated path. `value()` returns the
// per-variant `TagFieldData` enum — pattern-match on it for typed
// access, or use `{:?}` for a Debug dump. The library does **not**
// ship a Display impl; UI rendering is the caller's job.
let jump = tag.root().field_path("jump velocity").unwrap();
println!("{} ({}): {:?}", jump.name(), jump.type_name(), jump.value().unwrap());

// Toggle a flag by name.
tag.root_mut()
    .field_path_mut("unit/flags").unwrap()
    .flag_mut("has_hull").unwrap()
    .toggle();

tag.write("masterchief.biped.edited")?;
```

## Common tasks

### Read a field

```rust
use blam_tags::TagFieldData;

let tag = TagFile::read("path/to/tag.biped")?;
let field = tag.root().field_path("jump velocity").unwrap();

// Schema metadata.
println!("{} : {}", field.name(), field.type_name());

// Parsed value. Returns None for container / padding fields.
// The library has no Display impl on TagFieldData — pattern-match
// for typed access, or use `{:?}` for a Debug dump.
match field.value() {
    Some(TagFieldData::Real(v)) => println!("  value = {v}"),
    Some(TagFieldData::LongInteger(v)) => println!("  value = {v} (0x{v:08X})"),
    Some(other) => println!("  value = {other:?}"),
    None => println!("  (container or padding)"),
}
```

### Walk all fields of the root struct

```rust
for field in tag.root().fields() {
    println!("{}: {}", field.name(), field.type_name());
}
```

`fields()` skips padding / explanations / terminators / unknowns. Use
`TagStructDefinition::fields()` (see below) if you need the raw walk.

### Mutate a scalar field

```rust
use blam_tags::TagFieldData;

tag.root_mut()
    .field_path_mut("jump velocity").unwrap()
    .set(TagFieldData::Real(3.14))?;

tag.write("edited.biped")?;
```

### Toggle, set, and read flag bits by name

```rust
let mut field = tag.root_mut().field_path_mut("unit/flags").unwrap();

// Per-bit operations.
field.flag_mut("has_hull").unwrap().set(true);
field.flag_mut("airborne").unwrap().toggle();

// Enumerate all bits (names + state).
if let Some(blam_tags::TagOptions::Flags(bits)) = field.as_ref().options() {
    for bit in bits {
        println!("  [{}] {}  (bit {})", if bit.is_set { "x" } else { " " }, bit.name, bit.bit);
    }
}
```

### Block element operations

```rust
let mut seats = tag.root_mut()
    .field_path_mut("unit/seats").unwrap()
    .as_block_mut().unwrap();

let new_index = seats.add_element();        // append default-initialized element
seats.insert_element(0)?;                   // insert default element at index 0
seats.duplicate_element(0)?;                // copy element 0, placed at index 1
seats.swap_elements(0, 3)?;                 // exchange elements 0 and 3
seats.move_element(5, 1)?;                  // relocate element 5 to index 1
seats.delete_element(2)?;                   // remove element 2
seats.clear();                              // remove all
println!("now have {} seats", seats.len());
```

### Walk all elements of a block, mutating as you go

```rust
let mut regions = tag.root_mut()
    .field_path_mut("regions").unwrap()
    .as_block_mut().unwrap();

regions.for_each_element_mut(|mut region| {
    if let Some(mut name) = region.field_mut("name") {
        // …inspect, edit, whatever.
    }
});
```

Visitor-closure form because each yielded handle reborrows through `self` — Rust's borrow checker rules out simultaneous mutable iterators. `TagStructMut::for_each_field_mut` and `TagArrayMut::for_each_element_mut` follow the same shape.

### Read or scrub an api_interop field

`api_interop` leaves carry a 12-byte runtime handle — BCS zeros them on save to `{ descriptor: 0, address: UINT_MAX, definition_address: 0 }`.
Typically you'll either read them for introspection or reset them before committing a tag.

```rust
use blam_tags::{ApiInteropData, TagFieldData};

// Read.
let field = tag.root().field_path("vertex buffer interop").unwrap();
if let Some(TagFieldData::ApiInterop(i)) = field.value() {
    println!("descriptor=0x{:08X} address=0x{:08X} defaddr=0x{:08X}",
        i.descriptor().unwrap_or(0),
        i.address().unwrap_or(0),
        i.definition_address().unwrap_or(0));
}

// Scrub to BCS's reset pattern before saving.
tag.root_mut()
    .field_path_mut("vertex buffer interop").unwrap()
    .set(TagFieldData::ApiInterop(ApiInteropData::reset()))?;
```

### Inspect the schema (definitions)

The library exposes a second facade rooted at `tag.definitions()` for schema traversal without going through instance data.

```rust
let root = tag.definitions().root_struct();
println!("root struct: {} ({} bytes)", root.name(), root.size());

for field in root.fields() {
    println!("  {} @ {} : {}", field.name(), field.offset(), field.type_name());
    if let Some(block_def) = field.as_block() {
        println!("    block of {} (max {})",
            block_def.struct_definition().name(),
            block_def.max_count());
    }
}
```

From an instance you can always jump to its schema — `tag_struct.definition()`, `tag_field.definition()`, `tag_block.definition()`, `tag_array.definition()`.
`TagFieldDefinition::as_api_interop()` returns the `TagApiInteropDefinition` for interop fields, exposing the linked descriptor struct, a stable 16-byte guid, and the declared type name.

### Create a new tag from a schema

Schemas live under `definitions/<game>/<group>.json`, dumped from the engine DLLs by a pair of IDAPython scripts under `$HALO_ROOT/halo3_mcc/`:

- `halo3_dump_tag_definitions_json.py` — H3 guerilla.exe
- `haloreach_dump_tag_definitions_json.py` — Reach sapien.exe

The library builds a zero-filled tag directly from a schema:

```rust
use blam_tags::TagFile;

let mut tag = TagFile::new("definitions/halo3_mcc/biped.json")?;
// tag has: header with group_tag='bipd', signature='BLAM', checksum=0.
// tag_stream has: one zero-filled root element with default sub-chunks
// (empty blocks, null tag_references, reset api_interops).

tag.write("my_biped.biped")?;
```

`TagFile::new` validates every struct's computed size against the dumped `size` field. If the computed sum is short, it resolves any `tmpl` custom fields in that struct by loading the target group's sibling JSON, walks the target's parent chain, and adds each ancestor's root-struct size — matching Reach's factored shader layout (where `shader_decal_struct_definition` is 4 bytes of decal-specific data and `render_method_struct_definition` is inlined via the `tmpl` custom to supply the 100 bytes of common shader fields). H3 schemas keep the common fields inlined directly, so no expansion kicks in and the size check passes as-is.

A helper example validates every dumped schema against a sample real tag:

```sh
cargo run --release -p blam-tags --example schema_match -- \
    definitions/halo3_mcc /path/to/halo3_mcc/tags
```

### Optional streams (want / info / assd)

Three optional streams can hang off the tag file — `want` (dependency list), `info` (import info), `assd` (asset-depot icon storage). They're off by default on freshly created tags; attach as needed:

```rust
tag.add_dependency_list("definitions/halo3_mcc/tag_dependency_list.json")?;
tag.add_import_info("definitions/halo3_mcc/tag_import_information.json")?;
tag.add_asset_depot_storage("definitions/halo3_mcc/asset_depot_storage.json")?;

// Populate the dependency list from the tag's tag_reference fields
// (walks the tag tree, collects every non-null non-`impo` reference,
// matches the authoring toolset 98.8% exact on real tags):
tag.rebuild_dependency_list("definitions/halo3_mcc/tag_dependency_list.json")?;

// Drop a stream:
tag.remove_import_info();
tag.remove_asset_depot_storage();

// Read the root element of each stream via the facade:
if let Some(info) = tag.import_info() {
    let build = info.field_path("build").unwrap().value().unwrap();
    println!("build: {build:?}");
}
```

**Header checksum** is left at `0` on new tags, matching BCS's behaviour (see [`NOTES.md`](../NOTES.md) for the checksum-research trail — primitives are known but the byte-span isn't, deferred until a concrete load-failure forces the issue). A `TagFile::recompute_checksum()` stub exists for when we come back to it.

### Roundtrip (read → write → compare)

```rust
use blam_tags::TagFile;

let tag = TagFile::read("path/to/source.biped")?;
tag.write("path/to/temp.biped")?;

let source = std::fs::read("path/to/source.biped")?;
let round  = std::fs::read("path/to/temp.biped")?;
assert_eq!(md5::compute(&source), md5::compute(&round));
```

For in-memory pipelines (fuzzing, archive embedding, tests), use the
byte-buffer entry points instead of touching the filesystem:

```rust
let bytes = std::fs::read("path/to/source.biped")?;
let tag = TagFile::read_from_bytes(&bytes)?;
let round_bytes = tag.write_to_bytes()?;
assert_eq!(bytes, round_bytes);
```

The corpus-wide sweep lives in [`examples/roundtrip.rs`](examples/roundtrip.rs).
Run against one or more tag roots:

```sh
cargo run --release -p blam-tags --example roundtrip -- \
    /path/to/halo3_mcc/tags /path/to/haloreach_mcc/tags
```

### Error handling on the read path

Every wire-format failure surfaces as a typed [`TagReadError`](src/error.rs) — never a panic. The variants carry enough context to diagnose a malformed tag without re-running with prints:

```rust
use blam_tags::{TagFile, TagReadError};

match TagFile::read("path/to/tag.biped") {
    Ok(tag) => { /* … */ }
    Err(TagReadError::BadChunkSignature { offset, expected, got }) => {
        eprintln!("bad signature at 0x{offset:X}: expected {expected:?}, got {got:?}");
    }
    Err(TagReadError::ChunkSizeMismatch { chunk, started_at, ended_at, expected_end }) => {
        eprintln!("{chunk} ran from 0x{started_at:X} to 0x{ended_at:X}, expected 0x{expected_end:X}");
    }
    Err(TagReadError::Io(e)) => eprintln!("I/O error: {e}"),
    Err(other) => eprintln!("read failed: {other}"),
}
```

`TagReadError` is `#[non_exhaustive]` — match with a catch-all arm so adding new variants in future versions doesn't break callers. Schema-import errors (JSON shape, parent-chain resolution) live separately in [`TagSchemaError`](src/schema.rs).

The corruption test suite at [`tests/corruption.rs`](tests/corruption.rs) covers the major variants by feeding deliberately malformed bytes through `TagFile::read_from_bytes`.

## Architecture

Tag files are schema-driven — every tag carries its own layout description (`blay` chunk), so the parser is **generic**: nothing is hard-coded per tag type.
The library's job is to (a) read the embedded schema, (b) read the payload bytes into a tree that mirrors the schema, and (c) write that tree back byte-exact.

Two facades sit on top of the raw storage types:

- **[`api`]** — data-side facade. `TagStruct`, `TagField`, `TagBlock`,
  `TagArray`, `TagFlag`, `TagResource` and their mutable counterparts.
  Reachable from `TagFile`.
- **[`definition`]** — schema-side facade. `TagStructDefinition`,
  `TagFieldDefinition`, `TagBlockDefinition`, `TagArrayDefinition`,
  `TagResourceDefinition`. Reachable from `TagFile::definitions()`.

Everything the user-facing code should need is one or the other.
Lower-level modules (`data`, `path`, `stream`, `io`, `layout::TagLayout`) are available but no user code in this workspace (CLI, examples) reaches into them.

Error types:

- **[`error::TagReadError`]** — every failure on the binary read path. Carries chunk names, byte offsets, expected vs. actual values.
- **[`schema::TagSchemaError`]** — JSON-schema import failures (parse errors, missing parent chain, struct-size mismatches).

## Field paths

Paths match the shape the CLI uses:

```
"jump velocity"                      — root-level field
"unit/flags"                         — inline struct → field
"unit/seats[0]/flags"                — struct → block element → field
"regions[2]/permutations[0]/name"    — nested block elements
"Block:regions[0]/name"              — with optional Type: filter
```

Block and array element indices default to `0` on descent if omitted.
Field names are case-sensitive; `Type:` filters are case-insensitive.

## Version coverage

| Format                                         | Read | Write | Notes |
|------------------------------------------------|------|-------|-------|
| V1 layouts (flat `agro` records)               | ✓    | ✓     | Reconstructs `stv2` + `blv2` from paired aggregate records on write. |
| V2 layouts (`tgly` with `stv2`)                | ✓    | ✓     | Main Halo 3 / Reach format. |
| V3 layouts (adds `]==[` interop)               | ✓    | ✓     | Main Halo 3 / Reach format. |
| V4 layouts (`stv4` with per-struct version)    | ✓    | ✓     | Exercised on H4 / H2A MP tags in the community corpus sweep. |

Pageable-resource shapes handled: `tg\0c` (null), `tgrc` (exploded with inner `tgdt` + nested struct), `tgxc` (xsync, opaque payload).
ApiInterop (`ti][`) fields are parsed into `TagFieldData::ApiInterop` with `descriptor` / `address` / `definition_address` accessors and a `reset()` builder for BCS's canonical `{0, UINT_MAX, 0}` pattern.
VertexBuffer fields are preserved as raw bytes through the roundtrip but not yet parsed into typed values.
