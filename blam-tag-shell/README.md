# blam-tag-shell

## About

A command-line tool and interactive REPL for inspecting and editing Halo tag files. Built on the [`blam-tags`](../blam-tags/) library — no ManagedBlam, no .NET, no engine required.

See the workspace [root README](../README.md) for build instructions and an overview of the two crates.

## Install

```sh
cargo install --path blam-tag-shell
```

## Usage

```
blam-tag-shell --game <GAME> <COMMAND> [ARGS...] [FLAGS]
```

Every tag-bound command takes a `<FILE>` argument as its first positional. In the [REPL](#repl--interactive-shell) that argument is filled in automatically from the currently-loaded tag, so you can type `get "jump velocity"` without repeating the path.

### Required: `--game` / `-g`

Every invocation requires the global `--game <GAME>` flag (alias `-g`). The shell uses it for two things:

- **Schema lookup** — schemas are read from `definitions/<GAME>/<group>.json`. Required for `new`, the optional-stream attach commands, and any command that needs to know how to interpret a tag's contents.
- **Tag-reference rendering** — group-tag → group-name resolution comes from `definitions/<GAME>/_meta.json`, which is loaded eagerly at startup. Bad/missing path errors out before any command runs.

`<GAME>` is the directory name under `definitions/` — currently `halo3_mcc` and `haloreach_mcc`. The flag is global, so it can appear anywhere on the command line:

```sh
blam-tag-shell --game halo3_mcc header masterchief.biped
blam-tag-shell -g haloreach_mcc list /path/to/reach/tags --group bipd
blam-tag-shell get masterchief.biped "jump velocity" --game halo3_mcc
```

The example commands below elide `--game` for readability — add it to every invocation.

### Commands

| Command | Description |
|-|-|
| [`header`](#header--file-metadata) | Show tag/cache file header metadata |
| [`list`](#list--walk-a-directory-for-tags) | Walk a directory for tags; filter + list, or summarize by group |
| [`inspect`](#inspect--show-field-tree) | Show the field tree |
| [`get`](#get--read-a-field-value) | Read a field value |
| [`set`](#set--write-a-field-value) | Write a field value |
| [`flag`](#flag--get-or-set-a-flag-bit) | Get or set a flag bit by name |
| [`options`](#options--list-enumflag-options) | List enum/flag options for a field |
| [`block`](#block--block-element-operations) | Block element operations (count, add, insert, duplicate, delete, clear, swap, move) |
| [`layout-diff`](#layout-diff--compare-tag-schemas) | Diff the **schemas** of two tag files |
| [`data-diff`](#data-diff--compare-two-tag-values) | Diff the **values** of two tag files |
| [`deps`](#deps--list-tag-references) | List every `tag_reference` in a tag |
| [`find`](#find--search-values-across-a-directory) | Search a directory of tags for fields whose value matches a query |
| [`export`](#export--dump-tag-state-as-replay-commands) | Dump a tag's state as replayable `set` commands |
| [`extract-bitmap`](#extract-bitmap--bitmap-to-dds) | Extract a `.bitmap` tag's images as DDS files (one per image) |
| [`check`](#check--integrity-validator) | Integrity check — flag enum / flag / real / reference anomalies |
| [`new`](#new--create-a-fresh-tag-from-a-schema) | Create a fresh tag from a schema JSON |
| [`add-dependency-list`](#optional-stream-commands) | Attach an empty dependency-list stream |
| [`remove-dependency-list`](#optional-stream-commands) | Drop the dependency-list stream |
| [`rebuild-dependency-list`](#optional-stream-commands) | Repopulate the dependency list from the tag's own tag_references |
| [`add-import-info`](#optional-stream-commands) | Attach an empty import-info stream |
| [`remove-import-info`](#optional-stream-commands) | Drop the import-info stream |
| [`add-asset-depot-storage`](#optional-stream-commands) | Attach an empty asset-depot-storage stream |
| [`remove-asset-depot-storage`](#optional-stream-commands) | Drop the asset-depot-storage stream |
| [`repl`](#repl--interactive-shell) | Interactive shell against a loaded tag |

---

### `header` — File metadata

| Argument | Description |
|-|-|
| `<FILE>` | Path to a tag or cache file. |

| Long | Description |
|-|-|
| `--json` | Emit JSON. |

```sh
$ blam-tag-shell header masterchief.biped
Tag File
  Group:         bipd
  Group version: 3
  Build:         1.1
  Version:       11707
  Checksum:      0x2858868A
  File size:     35301 bytes
  Streams:       tag!, want
```

---

### `list` — Walk a directory for tags

| Argument | Description |
|-|-|
| `<DIR>` | Directory to walk (recursive). |

| Long | Description |
|-|-|
| `--group <TAG>` | Filter by group. Accepts either form: long name (`render_model`) or 4-byte group tag (`mode`). |
| `--starts-with <S>` | Only filenames starting with `S`. |
| `--contains <S>` | Only paths containing `S`. |
| `--ends-with <S>` | Only filenames ending with `S` (e.g. extension matching). |
| `--regex <PAT>` | Only full paths matching `PAT`. |
| `--from-file <F>` | Read candidate paths from `F` instead of walking. |
| `--summary` | Group tally instead of a path list. |
| `--sort-by-count` | Sort summary rows by count (desc) instead of name. |
| `--json` | Emit JSON. |

`list` is path-only — it never opens a tag file. For standalone tag files the file extension *is* the group name, so a full corpus walk runs in well under a second.

```sh
# Plain list of every biped under a tags root (long name or 4cc both work)
$ blam-tag-shell list /path/to/tags --group biped
$ blam-tag-shell list /path/to/tags --group bipd

# Group tally
$ blam-tag-shell list /path/to/tags/globals --summary
GROUP                               COUNT
--------------------------------------------
breakable_surface                       2
collision_damage                       22
damage_effect                          10
globals                                 1
wind                                    1
--------------------------------------------
14 types                               99

# JSON-sorted by most common
$ blam-tag-shell list /path/to/tags --summary --sort-by-count --json
```

---

### `inspect` — Show field tree

| Argument | Description |
|-|-|
| `<FILE>` | Path to a tag file. |
| `[PATH]` | Field path to start from (optional — defaults to the root). |

| Long | Description |
|-|-|
| `--all` | Include schema padding / explanation / skip / unknown fields. |
| `--full` | Recursively expand everything — including block elements. Default (flat): walk through structs / arrays / pageable_resources, but stop at blocks (display the count only). Drill into a single block element with `<path>[<index>]` instead. |
| `--json` | Emit JSON. |
| `--filter <S,...>` | Only show fields whose name contains any of the comma-separated substrings. |
| `--filter-not <S,...>` | Skip fields whose name contains any of these. |
| `--filter-value <S>` | Only leaves whose rendered value contains `S`. |

Two modes — flat (default) and full. **Flat** recurses through structs, arrays, and pageable_resources (the resource header struct walks like any other container) but stops at blocks: each block shows its `[<n> elements]` count and nothing more. **Full** recurses through blocks too. Arrays always expand regardless, since they're fixed-count from the schema.

When a block or array element collapses to a single scalar leaf (e.g. spherical-harmonic coefficient arrays where each element is a one-field `coefficient: real`), the `[i]` header and its leaf are merged onto a single line:

```
default lightprobe r: array [16 elements]
  [0] coefficient: real = 0.8951472
  [1] coefficient: real = -1.0852382
  ...
```

`--all` opts out of the collapse (every padding / explanation field gets its own line).

```sh
# Top-level fields — blocks show as count only
$ blam-tag-shell inspect masterchief.biped
unit: struct
  object: struct
  flags: long flags = 0x0046C708 [fires from camera, melee attackers cannot attach, ...]
  default team: short enum = 1 (player)
  attachments: block [3 elements]
  ...
jump velocity: real = 3.08

# Drill into a single element — blocks below it still gated by --full
$ blam-tag-shell inspect masterchief.biped "unit/seats[0]"

# Recursively expand everything, including blocks
$ blam-tag-shell inspect masterchief.biped --full

# Filter by name + value
$ blam-tag-shell inspect masterchief.biped --full \
    --filter velocity --filter-value 3

# Full tree as JSON
$ blam-tag-shell inspect masterchief.biped --full --json
```

A `pageable_resource` field renders the same way a struct does — its header struct walks like a normal container, so `cd "tag resource groups[0]/tag_resource"` lands inside the resource and `inspect --full` prints its fields and any nested blocks / data. Null and Xsync resources have no parsed tree, so they print just the kind summary line and stop.

---

### `get` — Read a field value

| Argument | Description |
|-|-|
| `<FILE>` | Path to a tag file. |
| `<PATH>` | Field path. |

| Long | Description |
|-|-|
| `--raw` | Output raw value only (no label). |
| `--json` | Emit JSON. |
| `--hex` | Output numeric values in hex. |

```sh
$ blam-tag-shell get masterchief.biped "jump velocity"
jump velocity: real = 3.08

$ blam-tag-shell get masterchief.biped "jump velocity" --raw
3.08

$ blam-tag-shell get masterchief.biped "unit/flags" --hex
```

---

### `set` — Write a field value

| Argument | Description |
|-|-|
| `<FILE>` | Path to a tag file. |
| `<PATH>` | Field path. |
| `<VALUE>` | Value to set. |

| Long | Description |
|-|-|
| `--output <FILE>` | Write to a different file instead of overwriting the source. |
| `--dry-run` | Preview changes without writing. |

The output reports `(was X)` so dry-runs and real runs are self-documenting.

```sh
# Float
$ blam-tag-shell set masterchief.biped "jump velocity" 99.5
set jump velocity = 99.5 (was 3.08)

# Enum by name (case-insensitive)
$ blam-tag-shell set masterchief.biped "unit/default team" covenant

# Tag reference: `GROUP:path` or `none`
$ blam-tag-shell set masterchief.biped "unit/melee damage" "jpt!:globals/melee_damage"

# Block index: integer or `none` (= -1)
$ blam-tag-shell set some.tag "block index field" none

# api_interop: `reset` writes BCS's canonical {0, UINT_MAX, 0}
#              or a comma triple (decimal or 0x…)
$ blam-tag-shell set asset.polyart "vertex buffer interop" reset
$ blam-tag-shell set asset.polyart "vertex buffer interop" 0xDEADBEEF,0x12345678,0

# Preview without writing
$ blam-tag-shell set masterchief.biped "jump velocity" 0.5 --dry-run
(dry run) would set jump velocity = 0.5 (was 3.08)

# Write to a different file
$ blam-tag-shell set masterchief.biped "jump velocity" 0.5 --output modified.biped
```

---

### `flag` — Get or set a flag bit

| Argument | Description |
|-|-|
| `<FILE>` | Path to a tag file. |
| `<PATH>` | Field path to a flags field. |
| `<FLAG_NAME>` | Flag name. |
| `[ACTION]` | `on`, `off`, or `toggle`. Omit to read. |

| Long | Description |
|-|-|
| `--output <FILE>` | Write to a different file. |
| `--dry-run` | Preview without writing. |

```sh
$ blam-tag-shell flag masterchief.biped "unit/flags" "fires from camera"
unit/flags.fires from camera = on

$ blam-tag-shell flag masterchief.biped "unit/flags" "fires from camera" off
set unit/flags.fires from camera = off (was on)

$ blam-tag-shell flag masterchief.biped "unit/flags" "fires from camera" toggle --dry-run
```

---

### `options` — List enum/flag options

| Argument | Description |
|-|-|
| `<FILE>` | Path to a tag file. |
| `<PATH>` | Field path to an enum or flags field. |

| Long | Description |
|-|-|
| `--json` | Emit JSON. |

```sh
# Enum — current value marked with an arrow
$ blam-tag-shell options masterchief.biped "unit/default team"
Enum options for 'unit/default team':
  0: default
  1: player <-
  2: human
  3: covenant

# Flags — checkboxes for each bit
$ blam-tag-shell options masterchief.biped "unit/flags"
Flag options for 'unit/flags':
  0: [ ] circular aiming
  1: [ ] destroyed after dying
  3: [x] fires from camera
  ...
```

---

### `block` — Block element operations

| Argument | Description |
|-|-|
| `<FILE>` | Path to a tag file. |
| `<PATH>` | Field path to a block. |
| `<ACTION>` | `count`, `add`, `insert`, `duplicate`, `delete`, `clear`, `swap`, or `move`. |
| `[INDEX]` | First index (`insert`/`duplicate`/`delete`, first of `swap`, from for `move`). |
| `[INDEX2]` | Second index (`swap` second, `move` to). |

| Long | Description |
|-|-|
| `--output <FILE>` | Write to a different file. |
| `--dry-run` | Preview without writing. |
| `--json` | Emit JSON (only meaningful for `count`). |

Unknown actions are rejected at parse time — `blam-tag-shell block … foo` fails with clap's `invalid value` listing the valid set.

```sh
$ blam-tag-shell block masterchief.biped "contact points" count
2

$ blam-tag-shell block masterchief.biped "contact points" add
$ blam-tag-shell block masterchief.biped "contact points" insert 0
$ blam-tag-shell block masterchief.biped "contact points" duplicate 1
$ blam-tag-shell block masterchief.biped "contact points" swap 0 1
$ blam-tag-shell block masterchief.biped "contact points" move 3 0
$ blam-tag-shell block masterchief.biped "contact points" delete 0
$ blam-tag-shell block masterchief.biped "contact points" clear --dry-run
```

---

### `layout-diff` — Compare tag schemas

| Argument | Description |
|-|-|
| `<FILE_A>` | First tag file. |
| `<FILE_B>` | Second tag file. |

Reports field adds / removes / type changes between two tags' schemas. For value comparison use [`data-diff`](#data-diff--compare-two-tag-values).

```sh
$ blam-tag-shell layout-diff h3/masterchief.biped reach/masterchief.biped
```

---

### `data-diff` — Compare two tag values

| Argument | Description |
|-|-|
| `<FILE_A>` | First tag file. |
| `<FILE_B>` | Second tag file. |

| Long | Description |
|-|-|
| `--only <PATH>` | Restrict both walks to a subtree. |
| `--json` | Emit JSON. |

Walks every leaf in both tags and reports `~ changed`, `- only in a`, `+ only in b`, plus a summary.

```sh
$ blam-tag-shell data-diff h3/masterchief.biped h3/floodcombat_elite.biped
~ jump velocity: 3.08 -> 4.8
~ physics/dead material name: "hard_metal_thin_hum_masterchief" -> "tough_floodflesh_combatform"
…
73 changed, 76 only in a, 6 only in b

$ blam-tag-shell data-diff a.biped b.biped --only 'unit/unit camera'
```

---

### `deps` — List tag references

| Argument | Description |
|-|-|
| `<FILE>` | Path to a tag file. |

| Long | Description |
|-|-|
| `--unique` | De-duplicate repeated references. |
| `--json` | Emit JSON. |

```sh
$ blam-tag-shell deps masterchief.biped
unit/object/model: hlmt:objects\characters\masterchief\masterchief
unit/object/collision damage: cddf:globals\collision_damage\biped_player
…
```

---

### `find` — Search values across a directory

| Argument | Description |
|-|-|
| `<DIR>` | Directory to walk. |
| `<VALUE>` | Substring (or regex with `--regex`) to search for. |

| Long | Description |
|-|-|
| `--group <TAG>` | Only search tags of this group. |
| `--field-name <PAT>` | Only check fields whose name matches `PAT` (regex). |
| `--regex` | Interpret `<VALUE>` as a regex. |
| `--json` | Emit JSON. |
| `--strict` | Fail on any unreadable tag. |

```sh
# Which weapons reference a specific sound tag?
$ blam-tag-shell find /path/to/tags 'weapons/fire_sound' \
    --group weap --field-name 'sound'

# Regex match — all numeric velocities > 5
$ blam-tag-shell find /path/to/tags '^[5-9]\.' \
    --field-name 'velocity' --regex
```

---

### `export` — Dump tag state as replay commands

| Argument | Description |
|-|-|
| `<FILE>` | Path to a tag file. |
| `[SUBTREE]` | Optional field path; only export fields under this subtree. |

| Long | Description |
|-|-|
| `--output <FILE>` | Write to a file instead of stdout. |

Emits one `set <file> <path> <value>` line per round-trippable leaf, plus a trailing comment block listing skipped types (data blobs, math composites, colors, bounds, api_interop runtime handles, etc.). Useful for diffing tag states, committing tag edits as reviewable patches, and reproducible authoring pipelines.

```sh
# Dump all settable leaves
$ blam-tag-shell export masterchief.biped > mc.cmds

# Diff two tags at field level (strip the tag-path column first)
$ blam-tag-shell export a.biped > /tmp/a.cmds
$ blam-tag-shell export b.biped > /tmp/b.cmds
$ awk '{$2="";print}' /tmp/a.cmds > /tmp/a.clean
$ awk '{$2="";print}' /tmp/b.cmds > /tmp/b.clean
$ diff -u /tmp/a.clean /tmp/b.clean

# Scope to a subtree
$ blam-tag-shell export masterchief.biped 'unit/unit camera' --output cam.cmds
```

---

### `extract-bitmap` — Bitmap to DDS

| Argument | Description |
|-|-|
| `<FILE>` | Path to a `.bitmap` tag file. |

| Long | Description |
|-|-|
| `--output <PATH>` | Where to write. Path ending in `.dds` → that exact file (single-image tags only). Otherwise treated as a directory. Default: current directory. |

Reads pixel bytes straight from the tag's `processed pixel data` blob and emits one DDS per image. No resource cache files needed — halo3_mcc / haloreach_mcc bitmaps keep their pixels inline. Validated against 25,908 / 25,908 bitmap-tag images across both corpora.

Output naming:

- `--output <FILE>.dds` → writes to that exact filename. Errors on multi-image tags (one filename can't hold all of them).
- `--output <DIR>` (anything not ending in `.dds`):
  - 1-image tag → `<DIR>/<tag_stem>.dds`.
  - N-image tag → `<DIR>/<tag_stem>/<i>.dds` (per-tag subdirectory).

Format coverage:

- **Legacy DDS** (fourcc / pixelformat masks): `dxt1`, `dxt3`, `dxt5`, `dxt5a`, `dxn`, `a8`, `y8`, `r8`, `ay8`, `a8y8`, `a4r4g4b4`, `x8r8g8b8`, `a8r8g8b8`, `v8u8`, `q8w8v8u8`, `abgrfp16`, `abgrfp32`, `a16b16g16r16`.
- **DXT10 extension**: array textures of any of the above + `signedr16g16b16a16`.
- **CPU-decoded to A8R8G8B8**: `dxn_mono_alpha` (BC5-shaped layout with luminance + alpha sub-blocks — port of TagTool's `DecompressDXNMonoAlpha`).

```sh
$ blam-tag-shell extract-bitmap masterchief.bitmap
masterchief.dds: 256×256 a8r8g8b8 (2D texture, 9 mips)

# Specific filename (single-image tags only)
$ blam-tag-shell extract-bitmap masterchief.bitmap --output ~/Downloads/chief.dds
/Users/.../Downloads/chief.dds: 256×256 a8r8g8b8 (2D texture, 9 mips)

# Directory output
$ blam-tag-shell extract-bitmap envmap.bitmap --output extracted/
extracted/envmap.dds: 256×256 dxt5 (cube map, 9 mips)

# Multi-image tag → directory of DDS files
$ blam-tag-shell extract-bitmap weapon_atlas.bitmap --output extracted/
extracted/weapon_atlas/0.dds: 512×512 dxt1 (2D texture, 10 mips)
extracted/weapon_atlas/1.dds: 256×256 dxt5 (2D texture, 9 mips)
…
```

---

### `check` — Integrity validator

| Argument | Description |
|-|-|
| `<FILE>` | Path to a tag file. |

| Long | Description |
|-|-|
| `--tags-root <DIR>` | Tags root directory; required for tag-reference existence checks. |
| `--only <KINDS>` | Comma-separated subset: `enum`, `flag`, `real`, `reference` (default: all). |
| `--json` | Emit JSON. |
| `--strict` | Non-zero exit status on any finding (for CI). |

Surfaces:

- **Enum out of range** — the stored int didn't resolve to a named variant.
- **Unknown flag bits** — bits set without a declared name.
- **Non-finite reals** — `NaN` / `±inf` in any real field.
- **Missing tag references** — with `--tags-root`, references that don't resolve to a file on disk.

```sh
$ blam-tag-shell check floodcombat_elite.biped --tags-root /path/to/tags
[reference] unit/dialogue variants[0]/dialogue: no file with stem 'sound\dialog\combat\floodcombat_elite'

1 finding(s)

# Fail the build if anything shows up
$ blam-tag-shell check masterchief.biped --tags-root /path/to/tags --strict
```

---

### `new` — Create a fresh tag from a schema

| Argument | Description |
|-|-|
| `<GROUP>` | Group name — matches the JSON filename under `definitions/<GAME>/` (e.g. `biped`, `render_model`, `scenario`). |

| Long | Description |
|-|-|
| `--output <FILE>` | Write to this path. Default: `./<GROUP>.<GROUP>` in the cwd. Refuses to overwrite an existing file. |

Game comes from the global `--game` flag. Resolves the schema at `definitions/<GAME>/<GROUP>.json`, builds a zero-filled tag (one default-initialized root element, no optional streams), and writes it. Header gets `group_tag` / `group_version` from the schema, `signature = 'BLAM'`, and `checksum = 0`.

Run from the workspace root (or wherever your `definitions/` tree lives) — paths are resolved relative to cwd.

```sh
$ blam-tag-shell --game halo3_mcc new render_model
created render_model.render_model from definitions/halo3_mcc/render_model.json

$ blam-tag-shell --game halo3_mcc new biped --output characters/chief.biped
created characters/chief.biped from definitions/halo3_mcc/biped.json
```

---

### Optional-stream commands

Six commands manage the three optional streams (`want`, `info`, `assd`) that can hang off a tag file after the mandatory `tag!` stream. All follow the same shape: load the tag, attach/remove/rebuild the stream, write back (or to `--output`).

The attach/rebuild commands need the matching stream schema, which they resolve from the global `--game` flag:

- `tag_dependency_list.json` for `want`
- `tag_import_information.json` for `info`
- `asset_depot_storage.json` for `assd`

| Command | Positionals | Description |
|-|-|-|
| `add-dependency-list` | `<FILE>` | Attach an empty `want` stream. No-op if already present. |
| `remove-dependency-list` | `<FILE>` | Drop `want` if present. |
| `rebuild-dependency-list` | `<FILE>` | Walk the tag's data, collect every non-null non-`impo` `tag_reference`, and write one entry per ref (flags=0) into `want`. Creates the stream first if missing. Matches authoring-toolset output exactly for 98.8% of real tags. |
| `add-import-info` | `<FILE>` | Attach an empty `info` stream. Caller populates build / version / culprit / import date / files / events fields via `set` if needed. |
| `remove-import-info` | `<FILE>` | Drop `info` if present. |
| `add-asset-depot-storage` | `<FILE>` | Attach an empty `assd` stream (tag-editor icon pixel data). Zero presence in the observed H3/Reach corpus. |
| `remove-asset-depot-storage` | `<FILE>` | Drop `assd` if present. |

Every command takes `--output <FILE>` to write elsewhere instead of overwriting the source.

```sh
$ blam-tag-shell --game halo3_mcc new biped
created biped.biped from definitions/halo3_mcc/biped.json

$ blam-tag-shell --game halo3_mcc add-dependency-list biped.biped
attached empty dependency-list stream

$ blam-tag-shell --game halo3_mcc rebuild-dependency-list biped.biped
rebuilt dependency-list (0 entries)

$ blam-tag-shell --game halo3_mcc header biped.biped
  ...
  Streams:       tag!, want
```

Typical authoring flow:

```sh
# Set --game once via the alias to keep examples short.
shopt -s expand_aliases
alias bts='blam-tag-shell --game halo3_mcc'

# 1. Create the tag.
bts new biped --output mychief.biped

# 2. Populate the data via `set` / `flag` / `block`.
bts set mychief.biped "jump velocity" 3.2
bts set mychief.biped "unit/integrated light toggle" \
    "effe:fx/flashlight"
# …

# 3. Build the dependency list from the references we just wrote.
bts rebuild-dependency-list mychief.biped
```

---

### `repl` — Interactive shell

| Argument | Description |
|-|-|
| `[FILE]` | Optional tag to load at startup. |

Opens a persistent session against a loaded tag. Tag-bound commands always operate on the loaded tag — the file positional is filled in automatically, so `inspect materials[0]` drills into the loaded tag's `materials[0]` element rather than being misread as `inspect materials[0] (no path)`. To work on a different tag, use `open <path>` to switch. Dirty-tracking means the REPL asks for confirmation before discarding unsaved edits. History is persisted to `~/.blam-tag-shell-history`.

**Session verbs:**

| | |
|-|-|
| `open <path>` | Load a tag. |
| `close` | Close the current tag. |
| `save [path]` | Write the tag (back to source, or to `path`). |
| `revert` | Reload from disk, discarding edits. |
| `exit` / `quit` | Leave the REPL (prompts on unsaved changes; `exit --force` skips the prompt). |
| `help` / `?` | Show inline help. |

**Navigation** (Unix-cd semantics — leading `/` resets to absolute):

| | |
|-|-|
| `edit-block <path>` | Push a sub-struct / block-element / array-element onto the nav stack. Alias: `cd`. |
| `back` | Pop one level. |
| `exit-to <segment>` | Pop until the named segment is the tail; `exit-to root` clears. |
| `pwd` | Show the current nav path. |

The prompt reflects the current tag, nav stack, and dirty state:

```
blam> open masterchief.biped
blam masterchief.biped> cd unit/seats[0]
blam masterchief.biped/unit/seats[0]> flag "flags" "invisible" toggle
set unit/seats[0]/flags.invisible = on (was off)
blam masterchief.biped*/unit/seats[0]> save
saved to masterchief.biped
blam masterchief.biped/unit/seats[0]> exit
```

---

## Field paths

Fields are addressed by `/`-separated paths. Block and array elements use `[index]` notation:

```
"jump velocity"                      # root-level field
"unit/flags"                         # inline struct -> field
"unit/seats[0]/flags"                # struct -> block element -> field
"regions[2]/permutations[0]/name"    # nested block elements
```

An optional `Type:` prefix on a segment disambiguates fields that share a name across types (`Block:regions`, `Struct:definitions[0]`). Type filters are case-insensitive; field names are case-sensitive. Element indices default to `0` when omitted.

## Fuzzy matching

Mistyped field names get suggestions:

```sh
$ blam-tag-shell get masterchief.biped "jmup velocity"
Error: field 'jmup velocity' not found. Did you mean 'jump velocity'?
```

## JSON output

Most commands support `--json` for machine-readable output, compatible with `jq`:

```sh
$ blam-tag-shell inspect masterchief.biped --json --depth 2 | jq '.[0].name'
"unit"

$ blam-tag-shell list /path/to/tags --summary --json | jq '.[] | select(.count > 100)'

$ blam-tag-shell check floodcombat_elite.biped --tags-root /path/to/tags --json \
    | jq '.[] | select(.kind == "reference")'
```
