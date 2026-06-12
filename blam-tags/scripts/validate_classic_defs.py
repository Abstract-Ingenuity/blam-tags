#!/usr/bin/env python3
"""Validate converted classic tag JSON defs the way the Rust loader does:
  - every field 'definition' resolves in the right registry
  - each struct's summed field sizes == declared 'size' (sizeofValue)
  - the loop is terminator-bounded

Run: validate_classic_defs.py <defs_dir-or-file>...
"""
import sys
import os
import json

# canonical type -> fixed byte size (matches blam-tags field_type_info).
SIZES = {
    "string": 32, "long_string": 256, "string_id": 4, "old_string_id": 4,
    "char_integer": 1, "short_integer": 2, "long_integer": 4, "int64_integer": 8,
    "byte_integer": 1, "word_integer": 2, "dword_integer": 4, "qword_integer": 8,
    "angle": 4, "tag": 4,
    "char_enum": 1, "short_enum": 2, "long_enum": 4,
    "long_flags": 4, "word_flags": 2, "byte_flags": 1,
    "point_2d": 4, "rectangle_2d": 8, "rgb_color": 4, "argb_color": 4,
    "real": 4, "real_slider": 4, "real_fraction": 4,
    "real_point_2d": 8, "real_point_3d": 12, "real_vector_2d": 8, "real_vector_3d": 12,
    "real_quaternion": 16, "real_euler_angles_2d": 8, "real_euler_angles_3d": 12,
    "real_plane_2d": 12, "real_plane_3d": 16,
    "real_rgb_color": 12, "real_argb_color": 16, "real_hsv_color": 12, "real_ahsv_color": 16,
    "short_bounds": 4, "angle_bounds": 8, "real_bounds": 8, "fraction_bounds": 8,
    "tag_reference": 16, "block": 12,
    "long_block_flags": 4, "word_block_flags": 2, "byte_block_flags": 1,
    "char_block_index": 1, "custom_char_block_index": 1,
    "short_block_index": 2, "custom_short_block_index": 2,
    "long_block_index": 4, "custom_long_block_index": 4,
    "data": 20, "vertex_buffer": 32,
    "pad": 0, "useless_pad": 0, "skip": 0, "explanation": 0, "custom": 0,
    "terminator": 0,
    # new classic field types (must be added to Rust field_type_info too):
    "pointer": 4, "real_matrix_3x3": 36,
}


def struct_size(name, d, cache, stack):
    if name in cache:
        return cache[name]
    if name in stack:
        raise ValueError("recursive struct %r" % name)
    stack = stack | {name}
    s = d["structs"][name]
    total = 0
    seen_terminator = False
    for f in s["fields"]:
        t = f["type"]
        if t == "terminator":
            seen_terminator = True
            break
        elif t == "struct":
            total += struct_size(f["definition"], d, cache, stack)
        elif t == "array":
            a = d["arrays"][f["definition"]]
            total += struct_size(a["struct"], d, cache, stack) * a["count"]
        elif t in ("pad", "useless_pad", "skip"):
            total += int(f["definition"])
        else:
            if t not in SIZES:
                raise ValueError("unknown type %r in struct %r" % (t, name))
            total += SIZES[t]
    if not seen_terminator:
        raise ValueError("struct %r missing terminator" % name)
    cache[name] = total
    return total


def check(path):
    d = json.load(open(path))
    errs = []
    # definition resolution
    for sn, s in d["structs"].items():
        for f in s["fields"]:
            t, df = f["type"], f.get("definition")
            reg = {"struct": "structs", "block": "blocks", "array": "arrays",
                   "data": "datas"}.get(t)
            if t in ("char_enum", "short_enum", "long_enum", "byte_flags", "word_flags", "long_flags"):
                reg = "enums_flags"
            if reg and df not in d.get(reg, {}):
                errs.append("%s.%s -> missing %s %r" % (sn, f.get("name"), reg, df))
    # size checks
    cache = {}
    for sn, s in d["structs"].items():
        try:
            computed = struct_size(sn, d, cache, set())
        except ValueError as e:
            errs.append(str(e)); continue
        declared = s["size"]
        if computed != declared:
            errs.append("SIZE %s computed=%d declared=%d (delta %+d)" % (sn, computed, declared, computed - declared))
    return errs


def main():
    targets = []
    for a in sys.argv[1:]:
        if os.path.isdir(a):
            targets += [os.path.join(a, f) for f in sorted(os.listdir(a)) if f.endswith(".json")]
        else:
            targets.append(a)
    total_err = 0
    clean = 0
    for p in targets:
        errs = check(p)
        if errs:
            total_err += len(errs)
            print("== %s ==" % os.path.basename(p))
            for e in errs[:20]:
                print("   ", e)
        else:
            clean += 1
    print("\n%d clean, %d files with errors (%d total errors)" % (clean, len(targets) - clean, total_err))


if __name__ == "__main__":
    main()
