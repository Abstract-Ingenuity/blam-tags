# blam-tags workspace

A Rust implementation of the Halo 3 / Reach tag file format: a
byte-exact roundtrip-capable library plus a CLI for inspecting and
editing tags.

No ManagedBlam, no .NET, no engine required. The parser reads each
tag's embedded layout chunk and interprets the bytes directly.

## Crates

| Crate | Role |
|---|---|
| [`blam-tags`](./blam-tags/) | The library. Reads, writes, navigates, and edits tag files. |
| [`blam-tag-shell`](./blam-tag-shell/) | Command-line front-end. Subcommands for header metadata, tag-type scans, field tree inspection, primitive and flag get/set, block element operations, enum/flag option listing, and layout diffs. |

Each crate has its own README with API shape / command reference.

## Status

- **Byte-exact roundtrip validated on 119,432 tags** across the Halo
  3 and Halo Reach MCC tag files. Read → write → md5 compare yields
  zero differences.
- **Layout versions 1 – 4** all read/write. V4 is implemented from
  format references; no V4 tags are present in the H3/Reach corpus,
  so that path hasn't been exercised in the roundtrip yet.

## Build

```sh
cargo build --release --workspace
```

Builds the library and the CLI binary (`blam-tag-shell`).

## Use the CLI

```sh
cargo run --release -p blam-tag-shell -- header path/to/masterchief.biped
cargo run --release -p blam-tag-shell -- get    path/to/masterchief.biped "jump velocity"
cargo run --release -p blam-tag-shell -- set    path/to/masterchief.biped "jump velocity" 3.14
```

Full command reference in [`blam-tag-shell/README.md`](./blam-tag-shell/README.md).

## Use the library

```rust
use blam_tags::TagFile;

let mut tag = TagFile::read("path/to/masterchief.biped")?;

// Read a field by slash-separated path.
let jump = tag.root().field_path("jump velocity").unwrap();
println!("{}: {} = {}", "jump velocity", jump.type_name(), jump.value().unwrap());

// Toggle a flag and write the edit back to a new file.
tag.root_mut()
    .field_path_mut("unit/flags").unwrap()
    .flag_mut("has_hull").unwrap()
    .toggle();

tag.write("path/to/edited.biped")?;
```

Full API tour with more examples in [`blam-tags/README.md`](./blam-tags/README.md).

## Layout

```
blam-tags/          — workspace root
├── Cargo.toml      — virtual manifest
├── blam-tags/      — library crate (modules: io, math, fields, layout,
│   └── src/          data, path, stream, file)
└── blam-tag-shell/ — CLI crate
    └── src/        — Clap entry point + per-command implementations
```
