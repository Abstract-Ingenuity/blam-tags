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

# canonical type -> fixed byte size (for synthesizing inline-array struct sizes)
FIELD_SIZES = {
    "string": 32, "long_string": 256, "string_id": 4, "old_string_id": 4,
    "char_integer": 1, "short_integer": 2, "long_integer": 4, "int64_integer": 8,
    "angle": 4, "tag": 4, "char_enum": 1, "short_enum": 2, "long_enum": 4,
    "byte_flags": 1, "word_flags": 2, "long_flags": 4, "point_2d": 4, "rectangle_2d": 8,
    "rgb_color": 4, "argb_color": 4, "real": 4, "real_slider": 4, "real_fraction": 4,
    "real_point_2d": 8, "real_point_3d": 12, "real_vector_2d": 8, "real_vector_3d": 12,
    "real_quaternion": 16, "real_euler_angles_2d": 8, "real_euler_angles_3d": 12,
    "real_plane_2d": 12, "real_plane_3d": 16, "real_rgb_color": 12, "real_argb_color": 16,
    "short_bounds": 4, "angle_bounds": 8, "real_bounds": 8, "fraction_bounds": 8,
    "tag_reference": 16, "block": 12, "data": 20, "vertex_buffer": 32,
    "char_block_index": 1, "short_block_index": 2, "long_block_index": 4,
    "pointer": 4, "real_matrix_3x3": 36,
}


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
    def __init__(self, shared=None, group_to_path=None, parent_groups=None):
        self.group_to_path = group_to_path or {}  # group tag -> xml path (parent lookup)
        self.parent_groups = parent_groups or set()  # groups that are some tag's parent (intermediate bases)
        self.blocks = {}
        self.structs = {}
        self.enums_flags = {}
        self.datas = {}
        self.arrays = {}
        self._enum_by_content = {}   # (kind, options tuple) -> name
        self._used_names = set()
        # XRef resolution (h2): regolithID -> element.
        # Seeded with the cross-file shared layouts (common/*.xml), then
        # the tag file's own inline layouts override in convert().
        shared = shared or ({}, {})
        self.layout_by_id = dict(shared[0])   # "block:foo" -> <Layout> element
        self.options_by_id = dict(shared[1])  # "enum:foo"  -> <Options> element
        self.struct_by_id = {}                # layout regolithID -> struct name (dedup shared layouts)

    def unique(self, base):
        name = base
        i = 2
        while name in self._used_names:
            name = "%s_%d" % (base, i)
            i += 1
        self._used_names.add(name)
        return name

    def resolve_options(self, field_el):
        """The <Options> for an enum/flags field — inline or via OptionsXRef."""
        opts = field_el.find("Options")
        if opts is not None:
            return opts
        xref = field_el.find("OptionsXRef")
        if xref is not None and xref.text:
            return self.options_by_id.get(xref.text.strip())
        return None

    def resolve_layout(self, el):
        """The <Layout> for a Block/Struct/Array — inline or via LayoutXRef."""
        lay = el.find("Layout")
        if lay is not None:
            return lay
        xref = el.find("LayoutXRef")
        if xref is not None and xref.text:
            rid = xref.text.strip()
            lay = self.layout_by_id.get(rid)
            if lay is None:
                raise ValueError("unresolved LayoutXRef %r" % rid)
            return lay
        raise ValueError("Block/Struct/Array has neither <Layout> nor <LayoutXRef>")

    def intern_enum(self, field_el, kind):
        """kind: 'enum' or 'flags'. Returns the registry name."""
        opts = self.resolve_options(field_el)
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

    def process_layout(self, layout_el, struct_name_hint=None):
        """Register (once) the struct for a <Layout>, deduped by its
        regolithID so shared (XRef'd) layouts map to a single struct.
        Returns the struct name."""
        rid = layout_el.get("regolithID")
        if rid and rid in self.struct_by_id:
            return self.struct_by_id[rid]
        base = struct_name_hint or sanitize(
            layout_el.get("internalName") or layout_el.get("name") or "struct"
        )
        struct_name = self.unique(base)
        if rid:
            self.struct_by_id[rid] = struct_name
        fs = self.latest_field_set(layout_el)
        if fs is None:
            raise ValueError("Layout %r has no FieldSet" % struct_name)
        # reserve the entry so recursive refs resolve
        entry = {"size": int(fs.get("sizeofValue")), "fields": []}
        # H2 inline structs with a `tag` carry a 16-byte block-style
        # header on disk (e.g. mapping_function = MAPP). Record it so the
        # decoder knows to consume + preserve it.
        tag = layout_el.get("tag")
        if tag:
            entry["tag"] = tag
        self.structs[struct_name] = entry
        entry["fields"] = self.process_fields(fs, struct_name)
        entry["fields"].append({"type": "terminator", "name": None})
        return struct_name

    def process_fields(self, fieldset_el, owner):
        fields = []
        for el in list(fieldset_el):
            tag = el.tag
            name = el.get("name")

            if tag == "Explanation":
                continue  # documentation only, 0 bytes, not on disk
            if tag == "Custom":
                fields.append({"type": "custom", "name": name})  # editor-only, 0 bytes
                continue
            if tag in ZERO_PAD:
                # UselessPad is 0 bytes on disk in the non-legacy (MCC)
                # form — kept only for the editor. Pad/Skip occupy bytes.
                length = 0 if tag == "UselessPad" else int(el.get("length", "0"))
                fields.append({"type": ZERO_PAD[tag], "name": name, "definition": length})
                continue
            if tag == "Struct":
                sname = self.process_layout(self.resolve_layout(el))
                fields.append({"type": "struct", "name": name, "definition": sname})
                continue
            if tag == "Block":
                layout = self.resolve_layout(el)
                sname = self.process_layout(layout)
                bname = self.unique(sanitize(layout.get("internalName") or layout.get("name")))
                self.blocks[bname] = {
                    "max_count": parse_max_count(el.get("maxElementCount")),
                    "struct": sname,
                }
                fields.append({"type": "block", "name": name, "definition": bname})
                continue
            if tag == "Array":
                if el.find("Layout") is not None or el.find("LayoutXRef") is not None:
                    layout = self.resolve_layout(el)
                    sname = self.process_layout(layout)
                else:
                    # Inline fields directly under <Array> (no <Layout>):
                    # synthesize an element struct from the array's children.
                    sname = self.process_inline_struct(el, name)
                aname = self.unique(sanitize(name) + "_array")
                self.arrays[aname] = {"count": int(el.get("count", "1")), "struct": sname}
                fields.append({"type": "array", "name": name, "definition": aname})
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

    def struct_size_from_fields(self, fields):
        """Sum a field list's byte sizes (for synthesized inline structs)."""
        total = 0
        for f in fields:
            t = f["type"]
            if t == "terminator":
                break
            if t in ("pad", "useless_pad", "skip"):
                total += int(f["definition"])
            elif t == "struct":
                total += self.structs[f["definition"]]["size"]
            elif t == "array":
                a = self.arrays[f["definition"]]
                total += self.structs[a["struct"]]["size"] * a["count"]
            else:
                total += FIELD_SIZES.get(t, 0)
        return total

    def process_inline_struct(self, el, name_hint):
        """Build a struct from an element's direct field children (an
        <Array>/<Struct> that carries fields inline instead of a <Layout>)."""
        sname = self.unique(sanitize(name_hint or "inline") + "_struct")
        entry = {"size": 0, "fields": []}
        self.structs[sname] = entry
        entry["fields"] = self.process_fields(el, sname)
        entry["fields"].append({"type": "terminator", "name": None})
        entry["size"] = self.struct_size_from_fields(entry["fields"])
        return sname

    def build_group_root(self, group_root_el):
        """Process a TagGroup's root struct, recursively prepending its
        parent group's root struct (the `parent` attribute) as field [0].
        The child's sizeofValue already includes the inherited parent, so
        only the field is prepended (the size is not bumped)."""
        root_layout = group_root_el.find("Layout")
        hint = sanitize(root_layout.get("internalName") or root_layout.get("name")) + "_struct"
        struct_name = self.process_layout(root_layout, hint)

        parent = group_root_el.get("parent")
        if parent:
            ppath = self.group_to_path.get(parent)
            if ppath is None:
                raise ValueError("parent group %r not found for inheritance" % parent)
            proot = ET.parse(ppath).getroot()
            # seed the parent file's own inline layouts/options for XRefs
            for lay in proot.iter("Layout"):
                rid = lay.get("regolithID")
                if rid:
                    self.layout_by_id.setdefault(rid, lay)
            for opt in proot.iter("Options"):
                rid = opt.get("regolithID")
                if rid:
                    self.options_by_id.setdefault(rid, opt)
            parent_struct = self.build_group_root(proot)
            self.structs[struct_name]["fields"].insert(
                0, {"type": "struct", "name": proot.get("name"), "definition": parent_struct}
            )
            # Leaf tags' sizeofValue already includes the inherited chain;
            # intermediate base groups (themselves a parent of others)
            # store OWN-only sizes, so bump them by the parent's full size.
            if group_root_el.get("group") in self.parent_groups:
                self.structs[struct_name]["size"] += self.structs[parent_struct]["size"]
        return struct_name

    def convert(self, xml_path):
        tree = ET.parse(xml_path)
        root = tree.getroot()
        assert root.tag == "TagGroup", "expected <TagGroup>, got <%s>" % root.tag
        group = root.get("group")
        name = root.get("name")
        version = int(root.get("version", "0"))

        # Pre-scan: index this file's own inline <Layout>/<Options> by
        # regolithID (overriding any same-id shared layout), so LayoutXRef
        # / OptionsXRef references resolve.
        for lay in root.iter("Layout"):
            rid = lay.get("regolithID")
            if rid:
                self.layout_by_id[rid] = lay
        for opt in root.iter("Options"):
            rid = opt.get("regolithID")
            if rid:
                self.options_by_id[rid] = opt

        root_layout = root.find("Layout")
        root_block = self.unique(sanitize(root_layout.get("internalName") or root_layout.get("name")))
        root_struct = self.build_group_root(root)
        self.blocks[root_block] = {"max_count": 1, "struct": root_struct}

        # finalize struct entries -> include a stable empty guid + size_string
        structs_out = {}
        for sn, s in self.structs.items():
            entry = {
                "guid": "00000000000000000000000000000000",
                "size": s["size"],
                "fields": s["fields"],
            }
            if s.get("tag"):
                entry["tag"] = s["tag"]
            structs_out[sn] = entry
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


def load_shared_layouts(root_dir):
    """Index every <Layout>/<Options> with a regolithID across ALL xml
    under root_dir (recursively, incl. common/). H2 shares layouts both
    via common/*.xml and inline in sibling tag files (e.g.
    campaign_metagame_bucket lives in unit.xml, referenced by character).
    Returns (layout_by_id, options_by_id)."""
    layout_by_id, options_by_id = {}, {}
    if not os.path.isdir(root_dir):
        return layout_by_id, options_by_id
    for dirpath, _, files in os.walk(root_dir):
        for fn in sorted(files):
            if not fn.endswith(".xml"):
                continue
            try:
                root = ET.parse(os.path.join(dirpath, fn)).getroot()
            except Exception:
                continue
            for lay in root.iter("Layout"):
                rid = lay.get("regolithID")
                if rid and rid not in layout_by_id:
                    layout_by_id[rid] = lay
            for opt in root.iter("Options"):
                rid = opt.get("regolithID")
                if rid and rid not in options_by_id:
                    options_by_id[rid] = opt
    return layout_by_id, options_by_id


def build_group_to_path(in_dir):
    """Map group tag -> xml path, plus the set of groups that are some
    other group's parent (intermediate base structs)."""
    group_to_path, parent_groups = {}, set()
    for fn in sorted(os.listdir(in_dir)):
        if not fn.endswith(".xml"):
            continue
        try:
            r = ET.parse(os.path.join(in_dir, fn)).getroot()
            g = r.get("group")
            if g:
                group_to_path[g] = os.path.join(in_dir, fn)
            if r.get("parent"):
                parent_groups.add(r.get("parent"))
        except Exception:
            pass
    return group_to_path, parent_groups


def convert_file(xml_path, shared=None, group_to_path=None, parent_groups=None):
    return Converter(shared, group_to_path, parent_groups).convert(xml_path)


def main():
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        sys.exit(1)
    if args[0] == "--dir":
        in_dir, out_dir = args[1], args[2]
        game = args[3] if len(args) > 3 else os.path.basename(out_dir.rstrip("/"))
        os.makedirs(out_dir, exist_ok=True)
        shared = load_shared_layouts(in_dir)
        group_to_path, parent_groups = build_group_to_path(in_dir)
        ok = fail = 0
        tag_index = {}
        for fn in sorted(os.listdir(in_dir)):
            if not fn.endswith(".xml"):
                continue
            try:
                d = convert_file(os.path.join(in_dir, fn), shared, group_to_path, parent_groups)
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
