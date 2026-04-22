# blam-tag-shell

A command-line tool for inspecting and editing Halo 3 / Reach tag files. Built on the `blam-tags` library.

## Install

```sh
cargo install --path cli
```

## Commands

### `blam-tag-shell header` — File metadata

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

### `blam-tag-shell scan` — Catalog tag types in a directory

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
```

```sh
# Sort by count, output as JSON
$ blam-tag-shell scan /path/to/tags --sort count --json
```

### `blam-tag-shell inspect` — Show field tree

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

### `blam-tag-shell get` — Read a field value

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

### `blam-tag-shell set` — Write a field value

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

### `blam-tag-shell flag` — Get or set flag bits

```sh
# Read a flag
$ blam-tag-shell flag masterchief.biped "unit/flags" "fires from camera"
unit/flags.fires from camera = on

# Set a flag
$ blam-tag-shell flag masterchief.biped "unit/flags" "fires from camera" off

# Toggle a flag
$ blam-tag-shell flag masterchief.biped "unit/flags" "fires from camera" toggle --dry-run
```

### `blam-tag-shell options` — List enum/flag options

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

### `blam-tag-shell block` — Block element operations

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

### `blam-tag-shell layout-diff` — Compare tag layouts

```sh
$ blam-tag-shell layout-diff h3/masterchief.biped reach/masterchief.biped
```

## Field Paths

Fields are addressed by `/`-separated paths. Block and array elements use `[index]` notation:

```
"jump velocity"                      # root-level field
"unit/flags"                         # inline struct -> field
"unit/seats[0]/flags"                # struct -> block element -> field
"regions[2]/permutations[0]/name"    # nested block elements
```

An optional `Type:` prefix on a segment disambiguates fields that share a name across types (`Block:regions`, `Struct:definitions[0]`). Type filters are case-insensitive; field names are case-sensitive.

## Fuzzy Matching

Mistyped field names get suggestions:

```sh
$ blam-tag-shell get masterchief.biped "jmup velocity"
Error: field 'jmup velocity' not found. Did you mean 'jump velocity'?
```

## JSON Output

Most commands support `--json` for machine-readable output, compatible with `jq`:

```sh
$ blam-tag-shell inspect masterchief.biped --json --depth 2 | jq '.[0].name'
"unit"

$ blam-tag-shell scan /path/to/tags --json | jq '.[] | select(.count > 100)'
```
