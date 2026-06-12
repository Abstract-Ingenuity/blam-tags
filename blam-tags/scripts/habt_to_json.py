#!/usr/bin/env python3
"""Convert HABT (Halo-Asset-Blender-Development-Toolset) classic tag layout XML
into blam-tags Sapien-dump-shape JSON definitions.

HABT layouts live at:
  io_scene_halo/file_tag/tag_interface/layouts/{h1,h2}/<group>.xml

Output mirrors the existing definitions/<game>_mcc/*.json shape consumed by
TagLayout::from_json (blocks / structs / enums_flags / datas / arrays). The
synthesized layout drives STRUCTURE + field types + nesting only; the classic
byte-decoder owns encoding/sizing.

Usage:
  habt_to_json.py <layout.xml> [<out.json>]
  habt_to_json.py --dir <layouts/h1> <out_defs_dir>

Notes
- Every struct gets a trailing {"type":"terminator"} (the layout loop stops there).
- Pad/Skip/UselessPad length goes in the field's "definition" slot (an int).
- sizeofValue from each <FieldSet> becomes the struct "size" (authoritative).
- Enums/flags are deduped by (kind, options) and named from the field.
"""
import sys
import os
import re
import json
import xml.etree.ElementTree as ET

# XML element tag -> blam-tags canonical JSON field "type".
# Dimensional types carry their dimension in the XML tag name (RealPoint2D...).
TYPE_MAP = {
    "CharInteger": "char_integer",
    "ShortInteger": "short_integer",
    "LongInteger": "long_integer",
    "Int64Integer": "int64_integer",
    "Angle": "angle",
    "Tag": "tag",
    "CharEnum": "char_enum",
    "ShortEnum": "short_enum",
    "LongEnum": "long_enum",
    "ByteFlags": "byte_flags",
    "WordFlags": "word_flags",
    "LongFlags": "long_flags",
    "WordBlockFlags": "word_block_flags",
    "LongBlockFlags": "long_block_flags",
    "ByteBlockFlags": "byte_block_flags",
    "Point2D": "point_2d",
    "Rectangle2D": "rectangle_2d",
    "Rectangle": "rectangle_2d",
    "RgbColor": "rgb_color",
    "ArgbColor": "argb_color",
    "Real": "real",
    "RealFraction": "real_fraction",
    "RealPoint2D": "real_point_2d",
    "RealPoint3D": "real_point_3d",
    "RealVector2D": "real_vector_2d",
    "RealVector3D": "real_vector_3d",
    "RealQuaternion": "real_quaternion",
    "RealEulerAngles2D": "real_euler_angles_2d",
    "RealEulerAngles3D": "real_euler_angles_3d",
    "RealPlane2D": "real_plane_2d",
    "RealPlane3D": "real_plane_3d",
    "RealRgbColor": "real_rgb_color",
    "RealArgbColor": "real_argb_color",
    "ShortBounds": "short_bounds",
    "AngleBounds": "angle_bounds",
    "RealBounds": "real_bounds",
    "RealFractionBounds": "fraction_bounds",
    "String": "string",
    "LongString": "long_string",
    "StringId": "string_id",
    "OldStringId": "old_string_id",
    "Data": "data",
    "VertexBuffer": "vertex_buffer",
    "Ptr": "pointer",                  # classic 4-byte cache pointer (new field type)
    "Matrix3x3": "real_matrix_3x3",    # classic 3x3 float matrix = 36 bytes (new field type)
    # container types handled specially: Block, Struct, Array
    # zero-size doc/pad types handled specially: Explanation, Pad, UselessPad, Skip
    # block-index types handled specially (need a block reference)
}

# block-index XML tag -> (canonical type, fallback integer type, byte size)
BLOCK_INDEX_MAP = {
    "CharBlockIndex": ("char_block_index", "char_integer"),
    "ShortBlockIndex": ("short_block_index", "short_integer"),
    "LongBlockIndex": ("long_block_index", "long_integer"),
    "CustomCharBlockIndex": ("custom_char_block_index", "char_integer"),
    "CustomShortBlockIndex": ("custom_short_block_index", "short_integer"),
    "CustomLongBlockIndex": ("custom_long_block_index", "long_integer"),
}

ZERO_PAD = {"Pad": "pad", "UselessPad": "useless_pad", "Skip": "skip"}


def sanitize(name):
    if not name:
        return "unnamed"
    s = re.sub(r"[^0-9A-Za-z]+", "_", name.strip()).strip("_")
    return s.lower() or "unnamed"


def parse_max_count(raw):
    """maxElementCount is sometimes a python-dict literal string like
    "{'mcc-cea': 65536, 'default': 2048}"; take 'default'."""
    if raw is None:
        return 0
    raw = raw.strip()
    if raw.startswith("{"):
        try:
            import ast
            d = ast.literal_eval(raw)
            return int(d.get("default", max(d.values())))
        except Exception:
            return 0
    try:
        return int(raw)
    except ValueError:
        return 0


class Converter:
    def __init__(self):
        self.blocks = {}
        self.structs = {}
        self.enums_flags = {}
        self.datas = {}
        self.arrays = {}
        self._enum_by_content = {}   # (kind, options tuple) -> name
        self._used_names = set()

    def unique(self, base):
        name = base
        i = 2
        while name in self._used_names:
            name = "%s_%d" % (base, i)
            i += 1
        self._used_names.add(name)
        return name

    def intern_enum(self, field_el, kind):
        """kind: 'enum' or 'flags'. Returns the registry name."""
        opts = field_el.find("Options")
        names = []
        if opts is not None:
            tag = "Enum" if kind == "enum" else "Bit"
            for o in opts.findall(tag):
                names.append(o.get("name"))
        key = (kind, tuple(names))
        if key in self._enum_by_content:
            return self._enum_by_content[key]
        base = sanitize(field_el.get("CStyleName") or field_el.get("name")) + ("_enum" if kind == "enum" else "_flags")
        name = self.unique(base)
        self.enums_flags[name] = {"options": names}
        self._enum_by_content[key] = name
        return name

    def intern_data(self, field_el):
        base = sanitize(field_el.get("name")) + "_data"
        name = self.unique(base)
        self.datas[name] = {}
        return name

    def latest_field_set(self, layout_el):
        """Pick the <FieldSet> marked isLatest, else version 0, else first."""
        sets = layout_el.findall("FieldSet")
        for fs in sets:
            if str(fs.get("isLatest")).lower() == "true":
                return fs
        for fs in sets:
            if fs.get("version") == "0":
                return fs
        return sets[0] if sets else None

    def process_layout(self, layout_el, struct_name):
        """Register a struct (name struct_name) from a <Layout>'s latest FieldSet."""
        if struct_name in self.structs:
            return self.structs[struct_name]["size"]
        fs = self.latest_field_set(layout_el)
        if fs is None:
            raise ValueError("Layout %r has no FieldSet" % struct_name)
        size = int(fs.get("sizeofValue"))
        # reserve the name so recursive refs resolve
        entry = {"size": size, "fields": []}
        self.structs[struct_name] = entry
        entry["fields"] = self.process_fields(fs, struct_name)
        entry["fields"].append({"type": "terminator", "name": None})
        return size

    def process_fields(self, fieldset_el, owner):
        fields = []
        for el in list(fieldset_el):
            tag = el.tag
            name = el.get("name")

            if tag == "Explanation":
                continue  # documentation only, 0 bytes, not on disk
            if tag in ZERO_PAD:
                length = int(el.get("length", "0"))
                fields.append({"type": ZERO_PAD[tag], "name": name, "definition": length})
                continue
            if tag == "Struct":
                layout = el.find("Layout")
                sname = sanitize(layout.get("internalName") or layout.get("name"))
                sname = self._register_unique_struct(layout, sname)
                fields.append({"type": "struct", "name": name, "definition": sname})
                continue
            if tag == "Block":
                layout = el.find("Layout")
                bname = sanitize(layout.get("internalName") or layout.get("name"))
                bstruct = bname + "_struct"
                bname_u = self.unique(bname)
                # struct name unique too
                bstruct = self.unique(bstruct)
                self.blocks[bname_u] = {
                    "max_count": parse_max_count(el.get("maxElementCount")),
                    "struct": bstruct,
                }
                self.process_layout(layout, bstruct)
                fields.append({"type": "block", "name": name, "definition": bname_u})
                continue
            if tag == "Array":
                layout = el.find("Layout")
                aname = sanitize(layout.get("internalName") or layout.get("name"))
                astruct = self._register_unique_struct(layout, aname + "_struct")
                aname_u = self.unique(aname + "_array")
                count = int(el.get("count", "1"))
                self.arrays[aname_u] = {"count": count, "struct": astruct}
                fields.append({"type": "array", "name": name, "definition": aname_u})
                continue
            if tag in ("ShortEnum", "LongEnum", "CharEnum"):
                ename = self.intern_enum(el, "enum")
                fields.append({"type": TYPE_MAP[tag], "name": name, "definition": ename})
                continue
            if tag in ("ByteFlags", "WordFlags", "LongFlags"):
                ename = self.intern_enum(el, "flags")
                fields.append({"type": TYPE_MAP[tag], "name": name, "definition": ename})
                continue
            if tag == "Data":
                dname = self.intern_data(el)
                fields.append({"type": "data", "name": name, "definition": dname})
                continue
            if tag in BLOCK_INDEX_MAP:
                # No block reference info in classic XML; emit as the
                # width-equivalent integer (byte-identical, no sub-chunk).
                _, fallback = BLOCK_INDEX_MAP[tag]
                fields.append({"type": fallback, "name": name})
                continue
            if tag == "TagReference":
                fields.append({"type": "tag_reference", "name": name})
                continue
            if tag in TYPE_MAP:
                fields.append({"type": TYPE_MAP[tag], "name": name})
                continue
            raise ValueError("Unhandled XML element <%s> in %r" % (tag, owner))
        return fields

    def _register_unique_struct(self, layout_el, base):
        sname = self.unique(base)
        self.process_layout(layout_el, sname)
        return sname

    def convert(self, xml_path):
        tree = ET.parse(xml_path)
        root = tree.getroot()
        assert root.tag == "TagGroup", "expected <TagGroup>, got <%s>" % root.tag
        group = root.get("group")
        name = root.get("name")
        version = int(root.get("version", "0"))

        root_layout = root.find("Layout")
        root_block = sanitize(root_layout.get("internalName") or root_layout.get("name"))
        root_block = self.unique(root_block)
        root_struct = self.unique(root_block + "_struct")
        self.blocks[root_block] = {"max_count": 1, "struct": root_struct}
        self.process_layout(root_layout, root_struct)

        # finalize struct entries -> include a stable empty guid + size_string
        structs_out = {}
        for sn, s in self.structs.items():
            structs_out[sn] = {
                "guid": "00000000000000000000000000000000",
                "size": s["size"],
                "fields": s["fields"],
            }
        blocks_out = {bn: {"max_count": b["max_count"],
                           "max_count_string": str(b["max_count"]),
                           "struct": b["struct"]} for bn, b in self.blocks.items()}
        datas_out = {dn: {"alignment": 4, "flags": 0, "max_size": 0, "max_size_string": "0"}
                     for dn in self.datas}
        arrays_out = {an: {"count": a["count"], "struct": a["struct"]} for an, a in self.arrays.items()}

        return {
            "name": name,
            "tag": group,
            "version": version,
            "flags": 0,
            "block": root_block,
            "blocks": blocks_out,
            "structs": structs_out,
            "arrays": arrays_out,
            "enums_flags": self.enums_flags,
            "datas": datas_out,
            "resources": {},
            "interops": {},
        }


def convert_file(xml_path):
    return Converter().convert(xml_path)


def main():
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        sys.exit(1)
    if args[0] == "--dir":
        in_dir, out_dir = args[1], args[2]
        game = args[3] if len(args) > 3 else os.path.basename(out_dir.rstrip("/"))
        os.makedirs(out_dir, exist_ok=True)
        ok = fail = 0
        tag_index = {}
        for fn in sorted(os.listdir(in_dir)):
            if not fn.endswith(".xml"):
                continue
            try:
                d = convert_file(os.path.join(in_dir, fn))
                with open(os.path.join(out_dir, fn[:-4] + ".json"), "w") as f:
                    json.dump(d, f, indent=1)
                # tag_index key is the literal 4-char group tag (space-padded).
                tag_index["%-4s" % d["tag"]] = d["name"]
                ok += 1
            except Exception as e:
                fail += 1
                print("FAIL %-40s %s" % (fn, e))
        meta = {"game": game, "dumped_from": "HABT classic XML layouts", "tag_index": tag_index}
        with open(os.path.join(out_dir, "_meta.json"), "w") as f:
            json.dump(meta, f, indent=1)
        print("converted %d ok, %d failed; wrote _meta.json (%d groups)" % (ok, fail, len(tag_index)))
    else:
        xml_path = args[0]
        out = args[1] if len(args) > 1 else None
        d = convert_file(xml_path)
        s = json.dumps(d, indent=1)
        if out:
            with open(out, "w") as f:
                f.write(s)
            print("wrote", out)
        else:
            print(s)


if __name__ == "__main__":
    main()
