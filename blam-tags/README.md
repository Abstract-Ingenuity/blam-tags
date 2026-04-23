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

// Read a field by slash-separated path.
let jump = tag.root().field_path("jump velocity").unwrap();
println!("{} = {}", jump.name(), jump.value().unwrap());

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
let tag = TagFile::read("path/to/tag.biped")?;
let field = tag.root().field_path("jump velocity").unwrap();

// Schema metadata.
println!("{} : {}", field.name(), field.type_name());

// Parsed value. Returns None for container / padding fields.
if let Some(value) = field.value() {
    println!("  value = {}", value);        // Display impl does the formatting.
    println!("  hex   = {:#}", value);      // Alternate flag → hex for int variants.
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

### Parse a value from a string (CLI-style)

```rust
tag.root_mut()
    .field_path_mut("unit/default_team").unwrap()
    .parse_and_set("red")?;   // handles enums, ints (incl. hex), reals, tag refs, etc.
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

let new_index = seats.add();             // append default-initialized element
seats.insert(0)?;                        // insert default element at index 0
seats.duplicate(0)?;                     // copy element 0, placed at index 1
seats.swap(0, 3)?;                       // exchange elements 0 and 3
seats.move_to(5, 1)?;                    // relocate element 5 to index 1
seats.delete(2)?;                        // remove element 2
seats.clear();                           // remove all
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

### Roundtrip (read → write → compare)

```rust
use blam_tags::TagFile;

let tag = TagFile::read("path/to/source.biped")?;
tag.write("path/to/temp.biped")?;

let source = std::fs::read("path/to/source.biped")?;
let round  = std::fs::read("path/to/temp.biped")?;
assert_eq!(md5::compute(&source), md5::compute(&round));
```

The corpus-wide sweep lives in [`examples/roundtrip.rs`](examples/roundtrip.rs).
Run against one or more tag roots:

```sh
cargo run --release -p blam-tags --example roundtrip -- \
    /path/to/halo3_mcc/tags /path/to/haloreach_mcc/tags
```

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
