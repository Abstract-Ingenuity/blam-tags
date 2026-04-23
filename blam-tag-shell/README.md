# blam-tag-shell

## About

A command-line tool for inspecting and editing Halo 3 / Reach tag files. Built on the [`blam-tags`](../blam-tags/) library — no ManagedBlam, no .NET, no engine required.

Any assistance or valid criticism would be appreciated. See the workspace [root README](../README.md) for build instructions and an overview of the two crates.

## Install

```sh
cargo install --path blam-tag-shell
```

## Usage

```
blam-tag-shell <COMMAND> [ARGS...] [FLAGS]
```

### Commands

| Command | Description |
|-|-|
| `header` | Show tag/cache file header metadata |
| `scan` | Catalog tag types in a directory |
| `inspect` | Show the field tree |
| `get` | Read a field value |
| `set` | Write a field value |
| `flag` | Get or set a flag bit |
| `options` | List enum/flag options for a field |
| `block` | Block element operations (count, add, insert, duplicate, delete, clear) |
| `layout-diff` | Diff the layouts of two tag files |

---

### `header` — File metadata

| Argument | Description |
|-|-|
| `<FILE>` | Path to a tag or cache file. |

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

### `scan` — Catalog tag types in a directory

| Argument | Description |
|-|-|
| `<DIR>` | Directory to scan (recursive). |

| Long | Description |
|-|-|
| `--json` | Output as JSON. |
| `--sort <name\|count>` | Sort order (default `name`). |

```sh
$ blam-tag-shell scan /path/to/tags/globals
GROUP    EXTENSION                   COUNT
--------------------------------------------
bsdt     breakable_surface               2
cddf     collision_damage               22
jpt!     damage_effect                  10
matg     globals                         1
wind     wind                            1
--------------------------------------------
14 types                                99

# Sort by count, output as JSON
$ blam-tag-shell scan /path/to/tags --sort count --json
```

---

### `inspect` — Show field tree

| Argument | Description |
|-|-|
| `<FILE>` | Path to a tag file. |
| `[PATH]` | Field path to start from (optional — defaults to the root). |

| Long | Description |
|-|-|
| `--depth <N>` | Maximum depth to display (default `1`). |
| `--all` | Show all fields including hidden. |
| `--json` | Output as JSON. |

```sh
# Top-level fields (depth 1)
$ blam-tag-shell inspect masterchief.biped
unit: struct
  object: struct
  flags: long flags = 0x0046C708 [fires from camera, melee attackers cannot attach, ...]
  default team: short enum = 1 (player)
  ...
moving turning speed: angle = 8.4858 rad (486.20 deg)
jump velocity: real = 3.08
standing camera height: real = 0.62
contact points: block [2 elements]
  [0]
    marker name: string_id = "right_foot"
  [1]
    marker name: string_id = "left_foot"

# Inspect a nested struct
$ blam-tag-shell inspect masterchief.biped "unit" --depth 0

# Full tree as JSON
$ blam-tag-shell inspect masterchief.biped --depth 3 --json
```

---

### `get` — Read a field value

| Argument | Description |
|-|-|
| `<FILE>` | Path to a tag file. |
| `<PATH>` | Field path (e.g. `"jump velocity"` or `"unit/seats[0]/flags"`). |

| Long | Description |
|-|-|
| `--raw` | Output raw value only (no label). |
| `--json` | Output as JSON. |
| `--hex` | Output numeric values in hex. |

```sh
$ blam-tag-shell get masterchief.biped "jump velocity"
jump velocity: real = 3.08

# Raw value only (for scripting)
$ blam-tag-shell get masterchief.biped "jump velocity" --raw
3.08

# Navigate nested paths
$ blam-tag-shell get masterchief.biped "unit/default team"
unit/default team: short enum = 1 (player)

# Block elements by index
$ blam-tag-shell get masterchief.biped "contact points[1]/marker name"

# JSON output
$ blam-tag-shell get masterchief.biped "unit/flags" --json

# Hex output for numeric fields
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

```sh
# Set a float
$ blam-tag-shell set masterchief.biped "jump velocity" 99.5
set jump velocity = 99.5

# Set an enum by name (case-insensitive)
$ blam-tag-shell set masterchief.biped "unit/default team" covenant

# Set a tag reference (GROUP:path format)
$ blam-tag-shell set masterchief.biped "unit/melee damage" "jpt!:globals/melee_damage"

# Set a string_id
$ blam-tag-shell set masterchief.biped "unit/right_hand_node" "right_hand"

# Set a block index (-1 or "none" for unset)
$ blam-tag-shell set some.tag "block index field" none

# Preview without writing
$ blam-tag-shell set masterchief.biped "jump velocity" 0.5 --dry-run
(dry run) would set jump velocity = 0.5

# Write to a different file
$ blam-tag-shell set masterchief.biped "jump velocity" 0.5 --output modified.biped
```

---

### `flag` — Get or set flag bits

| Argument | Description |
|-|-|
| `<FILE>` | Path to a tag file. |
| `<PATH>` | Field path to a flags field. |
| `<FLAG_NAME>` | Flag name. |
| `[ACTION]` | `on`, `off`, or `toggle`. Omit to read. |

| Long | Description |
|-|-|
| `--output <FILE>` | Write to a different file instead of overwriting the source. |
| `--dry-run` | Preview changes without writing. |

```sh
# Read a flag
$ blam-tag-shell flag masterchief.biped "unit/flags" "fires from camera"
unit/flags.fires from camera = on

# Set a flag
$ blam-tag-shell flag masterchief.biped "unit/flags" "fires from camera" off

# Toggle a flag
$ blam-tag-shell flag masterchief.biped "unit/flags" "fires from camera" toggle --dry-run
```

---

### `options` — List enum/flag options

| Argument | Description |
|-|-|
| `<FILE>` | Path to a tag file. |
| `<PATH>` | Field path to an enum or flags field. |

```sh
# Enum field — shows all options, marks current
$ blam-tag-shell options masterchief.biped "unit/default team"
Enum options for 'unit/default team':
  0: default
  1: player <-
  2: human
  3: covenant
  ...

# Flags field — shows checkboxes
$ blam-tag-shell options masterchief.biped "unit/flags"
Flag options for 'unit/flags':
  0: [ ] circular aiming
  1: [ ] destroyed after dying
  3: [x] fires from camera
  15: [x] melee attackers cannot attach
  ...
```

---

### `block` — Block element operations

| Argument | Description |
|-|-|
| `<FILE>` | Path to a tag file. |
| `<PATH>` | Field path to a block. |
| `<ACTION>` | One of `count`, `add`, `insert`, `duplicate`, `delete`, `clear`. |
| `[INDEX]` | Element index for `insert` / `duplicate` / `delete`. |

| Long | Description |
|-|-|
| `--output <FILE>` | Write to a different file instead of overwriting the source. |
| `--dry-run` | Preview changes without writing. |

```sh
# Count elements
$ blam-tag-shell block masterchief.biped "contact points" count
2

# Add a new default element
$ blam-tag-shell block masterchief.biped "contact points" add

# Insert at index
$ blam-tag-shell block masterchief.biped "contact points" insert 0

# Duplicate an element
$ blam-tag-shell block masterchief.biped "contact points" duplicate 1

# Delete an element
$ blam-tag-shell block masterchief.biped "contact points" delete 0

# Clear all elements
$ blam-tag-shell block masterchief.biped "contact points" clear --dry-run
```

---

### `layout-diff` — Compare tag layouts

| Argument | Description |
|-|-|
| `<FILE_A>` | First tag file. |
| `<FILE_B>` | Second tag file. |

```sh
$ blam-tag-shell layout-diff h3/masterchief.biped reach/masterchief.biped
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

$ blam-tag-shell scan /path/to/tags --json | jq '.[] | select(.count > 100)'
```
