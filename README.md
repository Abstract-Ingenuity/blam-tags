# blam-tags workspace

A Rust implementation of the Halo tag file format: a byte-exact roundtrip-capable library plus a CLI for inspecting and editing tags.

No ManagedBlam, no .NET, no engine required. The parser reads each tag's embedded layout chunk and interprets the bytes directly.

## Crates

| Crate | Role |
|---|---|
| [<code>blam&#8209;tags</code>](./blam-tags/) | The library. Reads, writes, navigates, and edits tag files. |
| [<code>blam&#8209;tag&#8209;shell</code>](./blam-tag-shell/) | Command-line front-end + interactive REPL. Subcommands for header metadata, directory listing / search / dependency walking, field tree inspection, get / set / flag / block edits, options enumeration, schema and value diffing, integrity checks, replay-script export, and bitmap-tag → DDS extraction. |

Each crate has its own README with API shape / command reference.

## Status

- **Byte-exact roundtrip validated across every tag in the Halo 3, Halo 3: ODST, Halo Reach, Halo 4, and Halo 2: Anniversary MP MCC corpora.** Read → write → md5 compare yields zero differences. Locally verified on the 119,432-tag H3 + Reach subset; full-corpus, validation (including H4 and H2A MP) contributed by the community.

- **Layout versions 1 – 4** all read/write and exercised in the above sweep.

- **Read path is panic-free on malformed input.** Every wire-format failure surfaces as a typed [`TagReadError`](blam-tags/src/error.rs) — `BadChunkSignature`, `BadChunkVersion`, `ChunkSizeMismatch`, `CountMismatch`, `InvalidUtf8`, etc. Corruption-suite tests live at [`blam-tags/tests/corruption.rs`](blam-tags/tests/corruption.rs).

- **Pageable resources walk like any other container.** Exploded resources expose a `TagResource::as_struct()` view onto the header struct (raw bytes pulled from the `tgdt` payload, sub-chunks parsed from `tgst`); the path resolver, REPL `cd`, and `inspect` all step through them transparently.

- **Bitmap → DDS extraction with 100% format coverage** across the halo3_mcc + haloreach_mcc bitmap corpora (25,908 / 25,908 images). Pure-tag-file path: pixels come from `processed pixel data`, DDS wrapper is generated per format (legacy fourcc/pixelformat for the common cases, DXT10 for arrays and `signedr16g16b16a16`, CPU decode to A8R8G8B8 for `dxn_mono_alpha`). See [`blam-tag-shell extract-bitmap`](./blam-tag-shell/README.md#extract-bitmap--bitmap-tag--dds) and the [`blam_tags::bitmap`](./blam-tags/src/bitmap.rs) module.

## Build

```sh
cargo build --release --workspace
```

Builds the library and the CLI binary (`blam-tag-shell`).

## Use the CLI

The shell needs a `--game <GAME>` flag (alias `-g`) on every invocation — it scopes schema lookups and group-name resolution to `definitions/<GAME>/`. `<GAME>` is a directory name under `definitions/` (currently `halo3_mcc` or `haloreach_mcc`).

```sh
cargo run --release -p blam-tag-shell -- --game halo3_mcc header path/to/masterchief.biped
cargo run --release -p blam-tag-shell -- --game halo3_mcc get    path/to/masterchief.biped "jump velocity"
cargo run --release -p blam-tag-shell -- --game halo3_mcc set    path/to/masterchief.biped "jump velocity" 3.14
```

Full command reference in [`blam-tag-shell/README.md`](./blam-tag-shell/README.md).

## Use the library

```rust
use blam_tags::TagFile;

let mut tag = TagFile::read("path/to/masterchief.biped")?;

// Read a field by slash-separated path. `value()` returns the
// per-variant `TagFieldData` (or `None` for container/padding fields).
let jump = tag.root().field_path("jump velocity").unwrap();
println!("{} ({}): {:?}", jump.name(), jump.type_name(), jump.value().unwrap());

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
├── blam-tags/      — library crate (modules: io, math, error, fields,
│   ├── src/          layout, schema, data, path, stream, file, api,
│   └── tests/        definition; integration tests in tests/)
└── blam-tag-shell/ — CLI crate
    └── src/        — Clap entry point + per-command implementations
```
