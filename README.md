# blam-tags workspace

A Rust implementation of the Halo tag file format: a byte-exact roundtrip-capable library plus a CLI for inspecting and editing tags.

No ManagedBlam, no .NET, no engine required. The parser reads each tag's embedded layout chunk and interprets the bytes directly.

## Crates

| Crate | Role |
|---|---|
| [<code>blam&#8209;tags</code>](./blam-tags/) | The library. Reads, writes, navigates, and edits tag files. Plus group-specific extractors: `bitmap` → DDS, `model_animation_graph` → JMA-family, `render_model`/`collision_model`/`physics_model` → JMS, `scenario_structure_bsp` → ASS. |
| [<code>blam&#8209;tag&#8209;shell</code>](./blam-tag-shell/) | Command-line front-end + interactive REPL. Subcommands for header metadata, directory listing / search / dependency walking, field tree inspection, get / set / flag / block edits, options enumeration, schema and value diffing, integrity checks, replay-script export, raw `tag_data` field dump, and the four group-specific extractors: bitmap → DDS, `.model` → JMS (render + collision + physics), `.scenario` → ASS (one per BSP), and animation → JMA-family text. |

Each crate has its own README with API shape / command reference.

## Status

- **Byte-exact roundtrip validated across every tag in the Halo 3, Halo 3: ODST, Halo Reach, Halo 4, and Halo 2: Anniversary MP MCC corpora.** Read → write → md5 compare yields zero differences. Locally verified on the 119,432-tag H3 + Reach subset; full-corpus, validation (including H4 and H2A MP) contributed by the community.

- **Layout versions 1 – 4** all read/write and exercised in the above sweep.

- **Read path is panic-free on malformed input.** Every wire-format failure surfaces as a typed [`TagReadError`](blam-tags/src/error.rs) — `BadChunkSignature`, `BadChunkVersion`, `ChunkSizeMismatch`, `CountMismatch`, `InvalidUtf8`, etc. Corruption-suite tests live at [`blam-tags/tests/corruption.rs`](blam-tags/tests/corruption.rs).

- **Pageable resources walk like any other container.** Exploded resources expose a `TagResource::as_struct()` view onto the header struct (raw bytes pulled from the `tgdt` payload, sub-chunks parsed from `tgst`); the path resolver, REPL `cd`, and `inspect` all step through them transparently.

- **Bitmap → DDS extraction with 100% format coverage** across the halo3_mcc + haloreach_mcc bitmap corpora (25,908 / 25,908 images). Pure-tag-file path: pixels come from `processed pixel data`, DDS wrapper is generated per format (legacy fourcc/pixelformat for the common cases, DXT10 for arrays and `signedr16g16b16a16`, CPU decode to A8R8G8B8 for `dxn_mono_alpha`). See [`blam-tag-shell extract-bitmap`](./blam-tag-shell/README.md#extract-bitmap--bitmap-to-dds) and the [`blam_tags::bitmap`](./blam-tags/src/bitmap.rs) module.

- **`model_animation_graph` → JMA-family text export with 100% codec coverage** across the H3 + Reach MCC corpora (36,270 / 36,270 animations). Decodes all 10 implemented codec slots — UncompressedStatic, UncompressedAnimated, EightByteQuantizedRotationOnly, ByteKeyframe, WordKeyframe, ReverseByte/WordKeyframe, BlendScreen, Curve, RevisedCurve — composes static + animated tracks against the skeleton via per-bone flag bitarrays, and emits `.JMM/.JMA/.JMT/.JMZ/.JMO/.JMR/.JMW` text files re-importable by Halo content tooling. Per-frame movement deltas are composed local→world at write time per Foundry's fix (translation × 100, conjugate quaternion serialization). See [`blam-tag-shell extract-animation`](./blam-tag-shell/README.md#extract-animation--decode-and-export-an-animation) / [`list-animations`](./blam-tag-shell/README.md#list-animations--enumerate-jmad-animations) and the [`blam_tags::animation`](./blam-tags/src/animation.rs) module.

- **`.model` → JMS export with full coverage** of the H3 MCC corpus (4,354 / 4,354 reconstructions). Polymorphic over `render_model`/`collision_model`/`physics_model` — emits per-purpose JMS files in the H3EK source-tree layout (`render/`, `collision/`, `physics/`). Render path walks `regions × permutations × meshes × parts` with bounds-decompressed positions/UVs and triangle-strip → list conversion; collision path walks BSP edge rings; physics path emits Havok primitives + ragdoll/hinge constraints. The skeleton from `render_model` provides world-space placement for `coll`/`phmo`. See [`blam-tag-shell extract-jms`](./blam-tag-shell/README.md#extract-jms--model-to-source-tree-jms-files) and the [`blam_tags::jms`](./blam-tags/src/jms.rs) module.

- **`.scenario` → ASS export with full corpus coverage** (147 / 147 BSPs across 49 H3 scenarios). Emits one ASS file per `scenario.structure_bsps[]` entry, pairing each `scenario_structure_bsp` with its `scenario_structure_lighting_info`. Categories emitted: cluster MESHes, per-IGD-def MESHes + per-placement INSTANCEs, real `BM_LIGHTING_*` material metadata, cluster portals, weather polyhedra, structure collision BSP, sbsp markers (as SPHERE primitives), `environment_objects[]` xref placements, and SPOT/DIRECT/OMNI/AMBIENT generic lights. Output mirrors H3EK's `data/levels/<map>/structure/<bsp>.ASS` layout for re-import as artist source. See [`blam-tag-shell extract-ass`](./blam-tag-shell/README.md#extract-ass--scenariostructurebsp-to-ass) and the [`blam_tags::ass`](./blam-tags/src/ass.rs) module.

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
├── blam-tags/      — library crate
│   ├── src/          — generic tag tree: io, math, error, fields,
│   │                   layout, schema, data, path, stream, file, api,
│   │                   definition
│   │                 — group-specific extractors: bitmap, animation,
│   │                   jms, ass (sharing the geometry helper module)
│   └── tests/      — integration tests (corruption suite, etc)
└── blam-tag-shell/ — CLI crate
    └── src/        — Clap entry point + per-command implementations
```
