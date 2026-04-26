//! `model_animation_graph` (jmad) animation extraction.
//!
//! [`Animation::new(&tag)`] walks the user-facing `definitions/animations`
//! block (or `resources/animations` for older inline-layout tags) and
//! pairs each entry with its `tag resource groups[r]/tag_resource/
//! group_members[m]` runtime payload. Each [`AnimationGroup`] carries
//! header metadata + the raw `animation_data` blob; call
//! [`AnimationGroup::decode`] to turn the blob into an
//! [`AnimationClip`] (static + animated tracks + flags + movement),
//! then [`AnimationClip::pose`] composes against a [`Skeleton`] and
//! [`Pose::write_jma`] emits a JMA-family text file.
//!
//! Inheriting jmads (zero local animations, parent reference set) are
//! a normal success: `Animation::len() == 0` with `parent()` non-null.
//!
//! See `project_jmad_extraction_shipped` in auto-memory for the
//! engine-specific blob layouts (H3 hardcoded vs Reach cumulative-sum)
//! and the binary references they were verified against.

use crate::api::TagStruct;
use crate::fields::TagFieldData;
use crate::file::TagFile;
use crate::math::{RealPoint3d, RealQuaternion};

/// Errors returned by [`Animation::new`] / [`AnimationGroup::decode`].
#[derive(Debug)]
pub enum AnimationError {
    /// Root struct doesn't expose the fields we expect from a jmad
    /// (no `definitions/animations` block).
    NotAnAnimationGraph,
    /// Animation has no codec payload (empty `animation_data` blob).
    /// Either the animation is inherited or the tag is malformed.
    NoCodecPayload,
    /// First byte of the codec stream isn't a recognized
    /// `e_animation_codec_types` value (0..=11).
    UnknownCodec(u8),
    /// Codec recognized but no decoder implemented for this slot.
    UnsupportedCodec(Codec),
    /// Codec stream is shorter than the codec's required header bytes.
    TruncatedHeader { codec: Codec, want: usize, have: usize },
    /// A computed slice into the codec stream goes past the blob's
    /// end. Common cause: the codec header's offsets disagree with
    /// the engine's actual layout.
    TruncatedPayload { codec: Codec, want_end: usize, blob_size: usize },
}

impl std::fmt::Display for AnimationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnAnimationGraph => write!(f, "tag is not a recognizable model_animation_graph (missing `definitions/animations`)"),
            Self::NoCodecPayload => write!(f, "animation has no codec payload (empty blob)"),
            Self::UnknownCodec(b) => write!(f, "unknown codec byte 0x{b:02x} (expected 0..=11)"),
            Self::UnsupportedCodec(c) => write!(f, "codec {c:?} not yet supported"),
            Self::TruncatedHeader { codec, want, have } =>
                write!(f, "{codec:?} header: need {want} bytes, blob has {have}"),
            Self::TruncatedPayload { codec, want_end, blob_size } =>
                write!(f, "{codec:?} payload: slice ends at {want_end} but blob is {blob_size} bytes"),
        }
    }
}

impl std::error::Error for AnimationError {}

/// Per-animation `data sizes` breakdown — the engine's record of how
/// the `animation_data` blob is internally partitioned. Useful for
/// sanity-checking total length against the sum of subsections.
///
/// Held as a name/value list rather than a fixed struct because the
/// shape varies by engine: H3 uses `packed_data_sizes_struct` (16
/// bytes, mixed i8/i16/i32), Reach widens to a `_reach` variant with
/// 17+ i32 fields (`blend_screen_data`, `object_space_offset_data`,
/// `ik_chain_*`, `uncompressed_object_space_data`, …). All fields
/// are byte counts and decode losslessly to i64.
#[derive(Debug, Clone, Default)]
pub struct PackedDataSizes {
    pub fields: Vec<(String, i64)>,
}

impl PackedDataSizes {
    /// Sum of all subsection bytes — what the blob length should equal
    /// in a well-formed tag.
    pub fn total(&self) -> i64 {
        self.fields.iter().map(|(_, v)| *v).sum()
    }

    /// Lookup a named subsection size. Returns 0 if absent.
    pub fn get(&self, name: &str) -> i64 {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, v)| *v).unwrap_or(0)
    }

    /// Which engine's `data sizes` struct this is. H3 uses
    /// `packed_data_sizes_struct` (0x10 bytes, 7 mixed-width fields);
    /// Reach widens to `packed_data_sizes_reach_struct` (0x44 bytes,
    /// 17 i32 fields adding `blend_screen_data`, `object_space_*`,
    /// `ik_chain_*`, `compressed_event_curve`, etc.). The presence of
    /// any of those Reach-only fields tells us we're reading a
    /// Reach-style blob, where the schema field NAMES are kept but
    /// their SEMANTICS shift (e.g. `static_node_flags` becomes the
    /// static-codec byte size, not the static-flag byte size).
    pub fn layout(&self) -> SizeLayout {
        let reach_only = ["blend_screen_data", "object_space_offset_data", "ik_chain_event_data"];
        if self.fields.iter().any(|(n, _)| reach_only.iter().any(|r| r == n)) {
            SizeLayout::Reach
        } else {
            SizeLayout::H3
        }
    }
}

/// Engine-version flavor of the `data sizes` struct. Determines how
/// the blob's sections are addressed: H3 packs the static codec
/// stream first then a single animated codec; Reach uses the same
/// names but with different meanings (the `static_node_flags` field
/// is repurposed to carry the static codec stream's byte size).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeLayout {
    /// Pre-Reach (H3, ODST). 7 fields, mixed widths.
    H3,
    /// Reach onward. 17 fields, all i32. Field names kept the same,
    /// but several now carry codec/flag sizes rather than what the
    /// names suggest. See [`Self::reach_static_codec_size`] etc.
    Reach,
}

/// One animation entry — header metadata from `animations[i]` joined
/// with the matching `group_members[m]` runtime payload.
#[derive(Debug)]
pub struct AnimationGroup<'a> {
    /// Index in `definitions/animations[]`.
    pub index: usize,
    /// Resolved `name` string-id.
    pub name: Option<String>,
    /// `animation type` enum name (`base`, `overlay`, `replacement`,
    /// `world`, …).
    pub animation_type: Option<String>,
    /// `frame info type` enum name from `animations[i]`. The matching
    /// resource group_member's `movement_data_type` should agree;
    /// disagreement is surfaced via [`Self::movement_type_mismatch`].
    pub frame_info_type: Option<String>,
    /// Engine-recorded `frame count` (from `animations[i]`).
    pub frame_count: i16,
    /// Engine-recorded `node count` (from `animations[i]`).
    pub node_count: i8,
    /// `node list checksum` from `animations[i]` — used to verify the
    /// jmad targets the same skeleton as the consuming render_model.
    pub node_list_checksum: i32,
    /// `(resource_group, resource_group_member)` pair from
    /// `animations[i]`. `(-1, -1)` is a valid sentinel meaning the
    /// animation has no local payload (inherited).
    pub resource_group: i16,
    pub resource_group_member: i16,
    /// `animation_checksum` from the matching group_member. `None` if
    /// the group_member couldn't be resolved.
    pub checksum: Option<i32>,
    /// `frame count*` from the matching group_member — the codec
    /// stream's frame count, which can differ from the animation's
    /// user-visible `frame_count` (notably for slot-8 BlendScreen
    /// animations, where the codec count is the number of variants
    /// in the screen, not the playback length). Falls back to
    /// `frame_count` when the group_member isn't resolved.
    pub codec_frame_count: Option<i16>,
    /// `movement_data_type` enum name from the matching group_member.
    pub movement_type: Option<String>,
    /// `data sizes` struct from the matching group_member.
    pub data_sizes: Option<PackedDataSizes>,
    /// First byte of the `animation_data` blob — the codec enum value
    /// per `e_animation_codec_types` (0..=8 for Halo 3, 0..=11 for
    /// Reach+). `None` if blob is empty or missing.
    pub codec_byte: Option<u8>,
    /// Raw `animation_data` blob bytes. Empty slice if the
    /// group_member is missing or its data field is empty.
    pub blob: &'a [u8],
}

impl<'a> AnimationGroup<'a> {
    /// True when the per-animation `frame info type` and the
    /// group_member's `movement_data_type` disagree. Indicates the
    /// tag was authored against a different schema than what the
    /// consumer expects — usually benign but worth flagging in sweeps.
    pub fn movement_type_mismatch(&self) -> bool {
        match (&self.frame_info_type, &self.movement_type) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        }
    }

    /// Codec byte for the **animated** stream that follows the static
    /// rest-pose stream. Engine-aware: H3 reads `default_data` (the
    /// static codec stream's size = animated stream offset); Reach
    /// reads positional index 0 (= `static_node_flags` field, but
    /// semantically the static codec stream's size in Reach's
    /// repurposed field naming). When the offset is 0 there's no
    /// static stream at all, so the byte at offset 0 IS the animated
    /// codec — return that.
    pub fn animated_codec_byte(&self) -> Option<u8> {
        let sizes = self.data_sizes.as_ref()?;
        let off = match sizes.layout() {
            SizeLayout::H3 => sizes.get("default_data") as usize,
            SizeLayout::Reach => sizes.fields.first().map(|(_, v)| *v as usize).unwrap_or(0),
        };
        self.blob.get(off).copied()
    }
}

/// All animations in a jmad, paired with their per-animation payload
/// from the tag's resource groups. Construct via [`Animation::new`].
pub struct Animation<'a> {
    animations: Vec<AnimationGroup<'a>>,
    parent: Option<String>,
}

/// Top-level struct name varies by jmad layout version: newer tags
/// call it `definitions`, older ones (e.g. mongoose, layout version
/// ~4601) call it `resources`. Try both.
const TOP_LEVEL_NAMES: &[&str] = &["definitions", "resources"];

impl<'a> Animation<'a> {
    /// Walk a parsed `model_animation_graph` tag and pair each
    /// per-animation entry with its `tag resource groups[]` runtime
    /// payload. Inheriting tags (zero local animations + non-null
    /// `parent animation graph`) succeed with `len() == 0` and a
    /// non-`None` [`Self::parent`].
    pub fn new(tag: &'a TagFile) -> Result<Self, AnimationError> {
        let root = tag.root();

        let top_prefix = TOP_LEVEL_NAMES.iter().copied()
            .find(|name| root.field_path(&format!("{name}/animations")).is_some())
            .ok_or(AnimationError::NotAnAnimationGraph)?;

        let animations_block = root
            .field_path(&format!("{top_prefix}/animations"))
            .and_then(|f| f.as_block())
            .ok_or(AnimationError::NotAnAnimationGraph)?;

        // Pre-walk all `tag resource groups[]` entries. Each one is a
        // `tag_resource` field; its `as_struct()` exposes a
        // `group_members` block whose elements carry the per-animation
        // payload. Build a (resource_group, resource_group_member) →
        // group_member-struct lookup so the per-animation join below
        // stays O(1) per lookup.
        let resource_groups_block = root
            .field_path("tag resource groups")
            .and_then(|f| f.as_block());

        let mut group_member_table: Vec<Option<Vec<TagStruct<'a>>>> = Vec::new();
        if let Some(rg) = resource_groups_block {
            for r in 0..rg.len() {
                let elem = match rg.element(r) {
                    Some(e) => e,
                    None => { group_member_table.push(None); continue; }
                };
                let resource = elem.field("tag_resource").and_then(|f| f.as_resource());
                let header = match resource.and_then(|r| r.as_struct()) {
                    Some(h) => h,
                    None => { group_member_table.push(None); continue; }
                };
                let members_block = header.field("group_members").and_then(|f| f.as_block());
                let members = match members_block {
                    Some(b) => (0..b.len()).filter_map(|i| b.element(i)).collect(),
                    None => Vec::new(),
                };
                group_member_table.push(Some(members));
            }
        }

        // Walk per-animation entries.
        let mut animations = Vec::with_capacity(animations_block.len());
        for i in 0..animations_block.len() {
            let anim = match animations_block.element(i) {
                Some(a) => a,
                None => continue,
            };
            let name = read_string_id(&anim, "name");

            // Reach moved most per-animation codec metadata into a
            // nested `shared animation data[0]` block. H3-era tags keep
            // it on the outer `animations[i]` struct directly. Pick
            // whichever struct actually carries the metadata fields.
            let metadata = anim
                .field("shared animation data")
                .and_then(|f| f.as_block())
                .and_then(|b| b.element(0))
                .filter(|s| s.field("resource_group").is_some()
                         || s.field("frame count").is_some())
                .unwrap_or(anim);

            // Older layouts (e.g. mongoose, version ~4601) call this
            // field `type`; newer ones name it `animation type`.
            let animation_type = read_enum_name(&metadata, "animation type")
                .or_else(|| read_enum_name(&metadata, "type"));
            let frame_info_type = read_enum_name(&metadata, "frame info type");
            let frame_count = read_i16(&metadata, "frame count").unwrap_or(0);
            let node_count = read_i8(&metadata, "node count").unwrap_or(0);
            let node_list_checksum = read_i32(&metadata, "node list checksum").unwrap_or(0);
            let resource_group = read_i16(&metadata, "resource_group").unwrap_or(-1);
            let resource_group_member = read_i16(&metadata, "resource_group_member").unwrap_or(-1);

            let (mut checksum, mut codec_frame_count, mut movement_type, mut data_sizes, mut codec_byte, mut blob) =
                resolve_member(&group_member_table, resource_group, resource_group_member);

            // Inline payload — older layouts skip the tgrc resource and
            // store `animation data` / `data sizes` directly on each
            // animation block element. Try inline only when the
            // resource lookup didn't find anything.
            if blob.is_empty() && data_sizes.is_none() {
                if let Some(inline_blob) = read_inline_animation_data(&metadata) {
                    blob = inline_blob;
                    codec_byte = blob.first().copied();
                }
                data_sizes = read_packed_data_sizes(&metadata);
                if movement_type.is_none() {
                    movement_type = frame_info_type.clone();
                }
                if checksum.is_none() {
                    checksum = read_i32(&metadata, "production checksum");
                }
                if codec_frame_count.is_none() {
                    codec_frame_count = Some(frame_count);
                }
            }

            animations.push(AnimationGroup {
                index: i,
                name,
                animation_type,
                frame_info_type,
                frame_count,
                node_count,
                node_list_checksum,
                resource_group,
                resource_group_member,
                checksum,
                codec_frame_count,
                movement_type,
                data_sizes,
                codec_byte,
                blob,
            });
        }

        // Parent reference for the inheritance case. Same prefix as
        // the animations block.
        let parent = root
            .field_path(&format!("{top_prefix}/parent animation graph"))
            .and_then(|f| f.value())
            .and_then(|v| match v {
                TagFieldData::TagReference(r) => r.group_tag_and_name.map(|(_, p)| p),
                _ => None,
            })
            .filter(|p| !p.is_empty());

        Ok(Self { animations, parent })
    }

    /// Number of animations in `definitions/animations[]`. Zero on
    /// inheriting tags.
    pub fn len(&self) -> usize { self.animations.len() }

    /// `true` when there are no local animations (commonly because
    /// the tag inherits from [`Self::parent`]).
    pub fn is_empty(&self) -> bool { self.animations.is_empty() }

    /// Iterate every animation group in declaration order.
    pub fn iter(&self) -> impl Iterator<Item = &AnimationGroup<'a>> {
        self.animations.iter()
    }

    /// Look up an animation by index into `definitions/animations[]`.
    pub fn get(&self, index: usize) -> Option<&AnimationGroup<'a>> {
        self.animations.get(index)
    }

    /// Look up an animation by its resolved string-id name.
    pub fn find(&self, name: &str) -> Option<&AnimationGroup<'a>> {
        self.animations.iter().find(|a| a.name.as_deref() == Some(name))
    }

    /// Path of `definitions/parent animation graph` if non-null.
    /// Inheriting jmads with `len() == 0` will typically have this set.
    pub fn parent(&self) -> Option<&str> { self.parent.as_deref() }

    /// Count of animations whose `(resource_group, resource_group_member)`
    /// didn't resolve. In a typical jmad this is 0; inheriting tags
    /// may have many.
    pub fn unresolved_count(&self) -> usize {
        self.animations.iter().filter(|a| a.checksum.is_none()).count()
    }
}

fn resolve_member<'a>(
    table: &[Option<Vec<TagStruct<'a>>>],
    rg: i16,
    rgm: i16,
) -> (Option<i32>, Option<i16>, Option<String>, Option<PackedDataSizes>, Option<u8>, &'a [u8]) {
    if rg < 0 || rgm < 0 {
        return (None, None, None, None, None, &[]);
    }
    let Some(Some(members)) = table.get(rg as usize) else {
        return (None, None, None, None, None, &[]);
    };
    let Some(member) = members.get(rgm as usize) else {
        return (None, None, None, None, None, &[]);
    };

    let checksum = read_i32(member, "animation_checksum");
    let codec_frame_count = read_i16(member, "frame count");
    let movement_type = read_enum_name(member, "movement_data_type");
    let data_sizes = read_packed_data_sizes(member);
    let blob = member.field("animation_data").and_then(|f| f.as_data()).unwrap_or(&[]);
    let codec_byte = blob.first().copied();

    (checksum, codec_frame_count, movement_type, data_sizes, codec_byte, blob)
}

/// Inline `animation data` field on an `animations[i]` element —
/// the older layout's storage spot. Tries underscore and space
/// variants of the field name.
fn read_inline_animation_data<'a>(anim: &TagStruct<'a>) -> Option<&'a [u8]> {
    anim.field("animation data").and_then(|f| f.as_data())
        .or_else(|| anim.field("animation_data").and_then(|f| f.as_data()))
}

fn read_packed_data_sizes(member: &TagStruct<'_>) -> Option<PackedDataSizes> {
    let s = member.field("data sizes").and_then(|f| f.as_struct())?;
    let mut fields = Vec::new();
    for f in s.fields() {
        let name = f.name().to_string();
        if let Some(v) = read_int_any(&s, &name) {
            fields.push((name, v));
        }
    }
    Some(PackedDataSizes { fields })
}

/// Read any integer field as i64 — accepts char/short/long widths
/// since `data sizes` field widths differ between engines (H3 uses
/// mixed i8/i16/i32; Reach widens everything to i32).
fn read_int_any(s: &TagStruct<'_>, name: &str) -> Option<i64> {
    match s.field(name)?.value()? {
        TagFieldData::CharInteger(v) => Some(v as i64),
        TagFieldData::ShortInteger(v) => Some(v as i64),
        TagFieldData::LongInteger(v) => Some(v as i64),
        TagFieldData::Int64Integer(v) => Some(v),
        _ => None,
    }
}

fn read_string_id(s: &TagStruct<'_>, name: &str) -> Option<String> {
    match s.field(name)?.value()? {
        TagFieldData::StringId(sid) | TagFieldData::OldStringId(sid) => {
            Some(sid.string).filter(|s| !s.is_empty())
        }
        _ => None,
    }
}

fn read_enum_name(s: &TagStruct<'_>, name: &str) -> Option<String> {
    match s.field(name)?.value()? {
        TagFieldData::CharEnum { name, .. } => name,
        TagFieldData::ShortEnum { name, .. } => name,
        TagFieldData::LongEnum { name, .. } => name,
        _ => None,
    }
}

fn read_i8(s: &TagStruct<'_>, name: &str) -> Option<i8> {
    match s.field(name)?.value()? {
        TagFieldData::CharInteger(v) => Some(v),
        _ => None,
    }
}

fn read_i16(s: &TagStruct<'_>, name: &str) -> Option<i16> {
    match s.field(name)?.value()? {
        TagFieldData::ShortInteger(v) => Some(v),
        _ => None,
    }
}

fn read_i32(s: &TagStruct<'_>, name: &str) -> Option<i32> {
    match s.field(name)?.value()? {
        TagFieldData::LongInteger(v) => Some(v),
        _ => None,
    }
}

// ---------------------------------------------------------------------
// Codec dispatch + per-slot decoders.
// ---------------------------------------------------------------------

/// Animation codec selector — the first byte of every animation's
/// codec stream. Slots 0..=8 are present in Halo 3; 9..=11 added in
/// Reach / Halo Online / later. Verified against
/// `g_codec_descriptions[9]` at `0x181170f90` in
/// `halo3_dllcache_play.dll`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Codec {
    NoCompression = 0,
    UncompressedStatic = 1,
    UncompressedAnimated = 2,
    EightByteQuantizedRotationOnly = 3,
    ByteKeyframeLightlyQuantized = 4,
    WordKeyframeLightlyQuantized = 5,
    ReverseByteKeyframeLightlyQuantized = 6,
    ReverseWordKeyframeLightlyQuantized = 7,
    BlendScreen = 8,
    Curve = 9,
    RevisedCurve = 10,
    SharedStatic = 11,
}

impl Codec {
    /// Map the codec byte at the start of an `animation_data` blob to
    /// a [`Codec`] variant. Returns `None` for bytes outside `0..=11`.
    pub fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::NoCompression,
            1 => Self::UncompressedStatic,
            2 => Self::UncompressedAnimated,
            3 => Self::EightByteQuantizedRotationOnly,
            4 => Self::ByteKeyframeLightlyQuantized,
            5 => Self::WordKeyframeLightlyQuantized,
            6 => Self::ReverseByteKeyframeLightlyQuantized,
            7 => Self::ReverseWordKeyframeLightlyQuantized,
            8 => Self::BlendScreen,
            9 => Self::Curve,
            10 => Self::RevisedCurve,
            11 => Self::SharedStatic,
            _ => return None,
        })
    }
}

/// Fields we actually use from the 20-byte `s_animation_codec_header`.
/// `compression_type` (byte 0) is dispatched separately via
/// [`Codec::from_byte`]; `error_value` and `compression_rate` are
/// surfaced via [`PackedDataSizes`] when needed. The struct shape is
/// verified against `animation_compute_orientations_interface.h` in
/// the Halo 3 PDB; `SIZE` reflects the on-disk size, not this Rust
/// struct's size.
#[derive(Debug, Clone, Copy)]
struct AnimationCodecHeader {
    total_rotated_nodes: u8,
    total_translated_nodes: u8,
    total_scaled_nodes: u8,
    translation_offset: u32,
    scale_offset: u32,
}

impl AnimationCodecHeader {
    const SIZE: usize = 20;

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::SIZE { return None; }
        Some(Self {
            total_rotated_nodes: bytes[1],
            total_translated_nodes: bytes[2],
            total_scaled_nodes: bytes[3],
            translation_offset: u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
            scale_offset: u32::from_le_bytes(bytes[16..20].try_into().unwrap()),
        })
    }
}

/// 32-byte fullframe codec header — base + three per-component
/// strides (bytes per node × frame_count for animated codecs, bytes
/// per node for static).
#[derive(Debug, Clone, Copy)]
struct FullframeCodecHeader {
    base: AnimationCodecHeader,
    rotation_stride: u32,
    translation_stride: u32,
    scale_stride: u32,
}

impl FullframeCodecHeader {
    const SIZE: usize = 32;

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::SIZE { return None; }
        Some(Self {
            base: AnimationCodecHeader::from_bytes(bytes)?,
            rotation_stride: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
            translation_stride: u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            scale_stride: u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
        })
    }
}

/// 48-byte keyframe codec header — base + per-component time-table
/// and payload-table byte offsets (from blob start). Order verified
/// against `s_keyframe_codec_header` in `compression_tools.h` from
/// the Halo 3 PDB.
#[derive(Debug, Clone, Copy)]
struct KeyframeCodecHeader {
    base: AnimationCodecHeader,
    rotation_key_time_offset: u32,
    translation_key_time_offset: u32,
    scale_key_time_offset: u32,
    rotation_key_payload_offset: u32,
    translation_key_payload_offset: u32,
    scale_key_payload_offset: u32,
}

impl KeyframeCodecHeader {
    const SIZE: usize = 48;

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::SIZE { return None; }
        Some(Self {
            base: AnimationCodecHeader::from_bytes(bytes)?,
            rotation_key_time_offset: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
            translation_key_time_offset: u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            scale_key_time_offset: u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
            rotation_key_payload_offset: u32::from_le_bytes(bytes[32..36].try_into().unwrap()),
            translation_key_payload_offset: u32::from_le_bytes(bytes[36..40].try_into().unwrap()),
            scale_key_payload_offset: u32::from_le_bytes(bytes[40..44].try_into().unwrap()),
            // bytes[44..48] is the trailing pad we don't need.
        })
    }
}

/// One codec stream's worth of decoded transforms, indexed by
/// `[codec_node_index][frame_index]`. The outer index is the codec's
/// own node enumeration (as counted by `total_*_nodes` in the codec
/// header) — NOT the global skeleton node index. Composition with the
/// per-bone flag bitarrays maps back to skeleton nodes; that step
/// happens in [`AnimationClip::pose`].
///
/// Translations are in **codec-native units** (raw `real_point3d`
/// floats — no `×100` JMA convention applied here). Rotations are
/// normalized quaternions. Scales are raw f32. `frame_count` is `1`
/// for static codecs; it's the animation's frame count for animated
/// codecs.
#[derive(Debug, Clone)]
pub struct AnimationTracks {
    pub codec: Codec,
    pub frame_count: u16,
    pub rotations: Vec<Vec<RealQuaternion>>,
    pub translations: Vec<Vec<RealPoint3d>>,
    pub scales: Vec<Vec<f32>>,
}

/// Animated-stream decode outcome attached to [`AnimationClip`] when
/// the animated codec wasn't (yet) decoded. `Unsupported` carries the
/// codec we recognized but can't yet decode (slot 4/5/6/7/8 etc.);
/// `Unknown` carries the raw byte when it didn't map to a known slot.
#[derive(Debug, Clone)]
pub enum AnimatedStreamStatus {
    /// `data sizes/default_data == 0` — animation is static-only.
    NoAnimatedStream,
    /// Decoded successfully — values live in `clip.animated_tracks`.
    Decoded,
    /// Recognized codec but no decoder yet.
    Unsupported(Codec),
    /// Codec byte ∉ 0..=11.
    Unknown(u8),
}

/// A fully-decoded animation. The blob's two codec streams come back
/// separately:
///
/// - [`static_tracks`](Self::static_tracks): the rest-pose / default
///   section, addressed by the *static* node-flag bitarrays (which
///   bones use this fixed transform). For most MCC animations this is
///   [`Codec::UncompressedStatic`].
/// - [`animated_tracks`](Self::animated_tracks): the per-frame
///   section, addressed by the *animated* node-flag bitarrays. `None`
///   when `data sizes/default_data == 0` (the animation is purely a
///   rest pose with no per-frame motion). The animated codec varies:
///   slot 3 (8-byte fullframe), slot 4/5/6/7 (keyframe), slot 8
///   (blend-screen) all common.
///
/// Combining the two via the bitarray flags into a single `(node,
/// frame) → transform` table happens in [`AnimationClip::pose`]; the
/// data model here keeps them separate so the JMA exporter, JSON
/// dump, and any future GUI can compose them differently.
#[derive(Debug, Clone)]
pub struct AnimationClip {
    pub frame_count: u16,
    pub static_tracks: AnimationTracks,
    pub animated_tracks: Option<AnimationTracks>,
    /// Why `animated_tracks` is what it is. `Decoded` when the animated
    /// codec was recognized and read; `NoAnimatedStream` for purely
    /// static animations; `Unsupported(_)` / `Unknown(_)` when the
    /// animated stream exists but couldn't be decoded — the static
    /// tracks are still valid in those cases (rest pose).
    pub animated_status: AnimatedStreamStatus,
    /// Per-component node-flag bitarrays for static and animated
    /// streams. `None` when the blob doesn't carry them (older inline
    /// layouts) — composition then falls back to "all bones use
    /// static_tracks" for static-only animations.
    pub node_flags: Option<NodeFlags>,
    /// Per-frame root-bone movement (dx/dy/dz/dyaw deltas). Empty
    /// when the animation has no movement (`frame info type = none`).
    pub movement: MovementData,
}

/// Per-frame root-bone movement deltas, keyed by `frame_info_type`.
/// All values are **local-space** as stored on disk — JMA's
/// world-space convention is applied at export time only.
///
/// - [`DxDy`](MovementKind::DxDy): 2 f32 per frame (dx, dy).
/// - [`DxDyDyaw`](MovementKind::DxDyDyaw): 3 f32 per frame (dx, dy, dyaw radians).
/// - [`DxDyDzDyaw`](MovementKind::DxDyDzDyaw): 4 f32 (dx, dy, dz, dyaw radians).
///
/// Layout matches TagTool's `MovementData.Read` (per-frame, packed
/// little-endian floats).
#[derive(Debug, Clone, Default)]
pub struct MovementData {
    pub kind: MovementKind,
    pub frames: Vec<MovementFrame>,
}

/// Kind of per-frame root movement encoded in the animation —
/// matches the schema's `frame_info_type_enum`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MovementKind {
    #[default]
    None,
    DxDy,
    DxDyDyaw,
    DxDyDzDyaw,
    /// `DxDyDz + angle_axis` — Reach addition. Read but not yet
    /// supported in JMA export.
    DxDyDzDangleAxis,
}

impl MovementKind {
    /// Bytes per frame on disk for this kind.
    pub fn bytes_per_frame(self) -> usize {
        match self {
            Self::None => 0,
            Self::DxDy => 8,
            Self::DxDyDyaw => 12,
            Self::DxDyDzDyaw => 16,
            Self::DxDyDzDangleAxis => 24,
        }
    }

    /// Resolve the `frame_info_type_enum` schema option name to a
    /// [`MovementKind`]. Unknown / missing names map to [`Self::None`].
    pub fn from_schema_name(name: &str) -> Self {
        match name {
            "dx,dy" => Self::DxDy,
            "dx,dy,dyaw" => Self::DxDyDyaw,
            "dx,dy,dz,dyaw" => Self::DxDyDzDyaw,
            "dx,dy,dz,dangle_axis" => Self::DxDyDzDangleAxis,
            _ => Self::None,
        }
    }
}

/// One frame of root-bone movement. Unused fields stay at default
/// (0.0) — the [`MovementKind`] tells you which fields are populated.
#[derive(Debug, Clone, Copy, Default)]
pub struct MovementFrame {
    pub dx: f32,
    pub dy: f32,
    pub dz: f32,
    /// Yaw delta in **radians** (matches the engine's stored unit).
    /// `DxDyDyaw` and `DxDyDzDyaw` populate this; `DxDyDzDangleAxis`
    /// stores a 3-vector that gets folded into a quaternion at export.
    pub dyaw: f32,
}

/// Per-component node-flag bitarrays for static + animated codec
/// streams. Six BitArrays total (rotation, translation, scale × static,
/// animated). Bone N is "static-rotated" iff
/// `static_rotation.bit(N)` is set; in that case its codec_node_index
/// in `static_tracks.rotations` is `popcount(static_rotation[0..N])`.
#[derive(Debug, Clone, Default)]
pub struct NodeFlags {
    pub static_rotation: BitArray,
    pub static_translation: BitArray,
    pub static_scale: BitArray,
    pub animated_rotation: BitArray,
    pub animated_translation: BitArray,
    pub animated_scale: BitArray,
}

/// Tightly-packed bit array used by the animation flag tables. Stored
/// as `u32` words (matching how the engine writes them).
#[derive(Debug, Clone, Default)]
pub struct BitArray {
    /// Underlying u32 words, little-endian on disk.
    pub words: Vec<u32>,
}

impl BitArray {
    /// Parse a tightly packed flag bitarray from little-endian u32
    /// words. Trailing bytes that don't fill a full u32 are dropped.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let words = bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        Self { words }
    }

    /// Read a single bit. Returns `false` for indices past the end.
    pub fn bit(&self, index: usize) -> bool {
        let (w, b) = (index / 32, index % 32);
        self.words.get(w).copied().unwrap_or(0) & (1u32 << b) != 0
    }

    /// Number of set bits in `[0..bound)`. Used to translate a
    /// skeleton-bone index into the codec's own flat node enumeration.
    pub fn popcount_below(&self, bound: usize) -> usize {
        let full_words = bound / 32;
        let mut count = 0u32;
        for w in self.words.iter().take(full_words) {
            count += w.count_ones();
        }
        if let Some(&w) = self.words.get(full_words) {
            let trailing_bits = bound % 32;
            if trailing_bits > 0 {
                let mask = (1u32 << trailing_bits) - 1;
                count += (w & mask).count_ones();
            }
        }
        count as usize
    }
}

impl<'a> AnimationGroup<'a> {
    /// Decode the blob into an [`AnimationClip`] — both the static
    /// rest-pose stream and the per-frame animated stream, plus
    /// per-bone flag bitarrays and per-frame movement deltas.
    ///
    /// Animated-stream codecs that aren't implemented (currently just
    /// `SharedStatic`) surface as
    /// [`AnimatedStreamStatus::Unsupported`] on the returned clip
    /// rather than an `Err` — the static rest pose is independently
    /// useful even if the animated stream can't be read.
    ///
    /// Hard errors (`Err`) only fire for genuinely malformed input:
    /// truncated codec headers, codec_byte ∉ 0..=11, no payload at all.
    pub fn decode(&self) -> Result<AnimationClip, AnimationError> {
        let codec_byte = self.codec_byte.ok_or(AnimationError::NoCodecPayload)?;
        let codec = Codec::from_byte(codec_byte).ok_or(AnimationError::UnknownCodec(codec_byte))?;
        // Some Reach animations have no static rest pose at all —
        // the blob starts directly with an animated codec. Detect via
        // either codec ≠ UncompressedStatic OR (Reach AND data_sizes[0] == 0).
        let static_first_size = self.data_sizes.as_ref()
            .and_then(|d| d.fields.first())
            .map(|(_, v)| *v as usize)
            .unwrap_or(0);
        let has_static_stream = matches!(codec, Codec::UncompressedStatic)
            && static_first_size > 0;
        let static_tracks = if has_static_stream {
            decode_uncompressed_static(self.blob)?
        } else {
            // No static stream — start with empty static tracks so
            // pose composition has something to fall back to. The
            // animated stream then starts at offset 0.
            AnimationTracks {
                codec: Codec::UncompressedStatic,
                frame_count: 1,
                rotations: Vec::new(),
                translations: Vec::new(),
                scales: Vec::new(),
            }
        };

        // Animated stream (if any) starts at the `default_data`
        // offset. TagTool's `AnimationResourceData.Read` consumes the
        // static stream then jumps to this position to find the next
        // codec_byte.
        //
        // When the animated codec isn't yet supported we don't fail
        // the whole decode — the static rest pose is independently
        // useful, so we surface the animated-stream outcome via
        // `animated_status` instead.
        let frame_count = self.codec_frame_count
            .or(Some(self.frame_count))
            .map(|f| f.max(1) as u16)
            .unwrap_or(1);
        // Reach uses cumulative-sum from positional indices in the
        // (renamed-but-misleading) `data sizes` struct: index 0 = static
        // codec stream, index 1 = animated codec stream, 2/3 = flag
        // triplets, 4 = movement, 5 = pill, 6+ = Reach-only extras.
        // H3 instead has hardcoded offsets where `default_data` is the
        // static codec stream, animated codec immediately follows.
        let layout = self.data_sizes.as_ref().map(|d| d.layout()).unwrap_or(SizeLayout::H3);
        let static_size = if has_static_stream {
            match layout {
                SizeLayout::H3 => self.data_sizes.as_ref().map(|d| d.get("default_data") as usize).unwrap_or(0),
                SizeLayout::Reach => static_first_size,
            }
        } else { 0 };
        // Per the Reach binary's `c_animation_data::get_animation_compression_codec`
        // (`animation_data.cpp:74`), the animated codec_byte ALWAYS lives at
        // `get_data_offset(e_internal_data_type::1) = cumsum(sizes[0..0])`.
        // For Reach with cumulative-sum layout that's `static_size` (which is 0
        // when no static stream). For H3 the static codec stream is at offset 0
        // when present, so the animated section starts at `static_size` either
        // way — no-static animations get the whole blob routed through the
        // animated decoder.
        let animated_offset = static_size;
        let animated_blob_len = match layout {
            SizeLayout::Reach => self.data_sizes.as_ref()
                .and_then(|d| d.fields.get(1))
                .map(|(_, v)| *v as usize)
                .unwrap_or(0),
            SizeLayout::H3 => self.blob.len().saturating_sub(animated_offset),
        };
        let (animated_tracks, animated_status, animated_codec_size) = if animated_offset >= self.blob.len() || animated_blob_len == 0 {
            (None, AnimatedStreamStatus::NoAnimatedStream, None)
        } else {
            let anim_end = (animated_offset + animated_blob_len).min(self.blob.len());
            let anim_blob = &self.blob[animated_offset..anim_end];
            let anim_byte = anim_blob[0];
            let (tracks, status) = match Codec::from_byte(anim_byte) {
                None => (None, AnimatedStreamStatus::Unknown(anim_byte)),
                Some(c @ Codec::EightByteQuantizedRotationOnly) =>
                    try_animated(c, || decode_fullframe(anim_blob, c, frame_count, true)),
                Some(c @ (Codec::UncompressedAnimated | Codec::BlendScreen)) =>
                    try_animated(c, || decode_fullframe(anim_blob, c, frame_count, false)),
                Some(c @ (Codec::ByteKeyframeLightlyQuantized
                    | Codec::ReverseByteKeyframeLightlyQuantized)) =>
                    try_animated(c, || decode_keyframe(anim_blob, c, frame_count, 1)),
                Some(c @ (Codec::WordKeyframeLightlyQuantized
                    | Codec::ReverseWordKeyframeLightlyQuantized)) =>
                    try_animated(c, || decode_keyframe(anim_blob, c, frame_count, 2)),
                Some(c @ Codec::Curve) =>
                    try_animated(c, || decode_curve(anim_blob, c, frame_count, false)),
                Some(c @ Codec::RevisedCurve) =>
                    try_animated(c, || decode_curve(anim_blob, c, frame_count, true)),
                Some(other) => (None, AnimatedStreamStatus::Unsupported(other)),
            };
            // For Reach, the animated codec size is recorded
            // explicitly at positional index 1 in `data sizes`. For
            // H3 we infer it from the codec header / payload extents.
            let size = match layout {
                SizeLayout::Reach => self.data_sizes.as_ref()
                    .and_then(|d| d.fields.get(1))
                    .map(|(_, v)| *v as usize),
                SizeLayout::H3 => matches!(status, AnimatedStreamStatus::Decoded)
                    .then(|| Codec::from_byte(anim_byte).and_then(|c| animated_codec_stream_size(anim_blob, c)))
                    .flatten(),
            };
            (tracks, status, size)
        };

        let node_flags = self.data_sizes.as_ref().and_then(|d| {
            // Reach stores flags at positional indices 2 + 3 with
            // explicit sizes; H3 places them right after the animated
            // codec with sizes carried by the named fields.
            let (off, static_total, animated_total) = match layout {
                SizeLayout::Reach => {
                    let cumsum = d.fields.iter().take(2).map(|(_, v)| *v as usize).sum::<usize>();
                    let s = d.fields.get(2).map(|(_, v)| *v as usize).unwrap_or(0);
                    let a = d.fields.get(3).map(|(_, v)| *v as usize).unwrap_or(0);
                    (cumsum, s, a)
                }
                SizeLayout::H3 => (
                    static_size + animated_codec_size?,
                    d.get("static_node_flags") as usize,
                    d.get("animated_node_flags") as usize,
                ),
            };
            read_node_flags(self.blob, off, static_total, animated_total)
        });

        // Movement offset+size: Reach has it at positional index 4;
        // H3 sits at the trailing end of the blob (just before pill).
        let movement = self.data_sizes.as_ref().map(|d| {
            let (off, size) = match layout {
                SizeLayout::Reach => (
                    d.fields.iter().take(4).map(|(_, v)| *v as usize).sum::<usize>(),
                    d.fields.get(4).map(|(_, v)| *v as usize).unwrap_or(0),
                ),
                SizeLayout::H3 => {
                    let m = d.get("movement_data") as usize;
                    let p = d.get("pill_offset_data") as usize;
                    let off = self.blob.len().saturating_sub(p).saturating_sub(m);
                    (off, m)
                }
            };
            read_movement_at(
                self.blob, off, size,
                self.movement_type.as_deref().or(self.frame_info_type.as_deref()),
                frame_count as usize,
            )
        }).unwrap_or_default();

        Ok(AnimationClip {
            frame_count,
            static_tracks,
            animated_tracks,
            animated_status,
            node_flags,
            movement,
        })
    }
}

/// Read per-frame root movement from `blob[offset..offset+size]`.
/// Returns the `MovementKind::None` empty default if the slice is
/// out of bounds, the kind doesn't divide cleanly into the slice, or
/// `frame_info_type` is `none`.
///
/// `DxDyDzDangleAxis` reads the 3-vector at `+12..+24` and uses the
/// component at `+20` as dyaw. Full angle-axis composition isn't
/// implemented — kept as-is for parity with TagTool/Foundry.
fn read_movement_at(
    blob: &[u8],
    offset: usize,
    movement_bytes: usize,
    frame_info_type: Option<&str>,
    frame_count: usize,
) -> MovementData {
    let kind = frame_info_type.map(MovementKind::from_schema_name).unwrap_or(MovementKind::None);
    if kind == MovementKind::None || movement_bytes == 0 { return MovementData::default(); }
    let bpf = kind.bytes_per_frame();
    if bpf == 0 || movement_bytes % bpf != 0 { return MovementData::default(); }
    if offset.checked_add(movement_bytes).map_or(true, |end| end > blob.len()) {
        return MovementData::default();
    }
    let read_count = (movement_bytes / bpf).min(frame_count);
    let mut frames = Vec::with_capacity(read_count);
    for i in 0..read_count {
        let off = offset + i * bpf;
        let f = match kind {
            MovementKind::DxDy => MovementFrame {
                dx: f32_at(blob, off), dy: f32_at(blob, off + 4),
                ..Default::default()
            },
            MovementKind::DxDyDyaw => MovementFrame {
                dx: f32_at(blob, off), dy: f32_at(blob, off + 4),
                dyaw: f32_at(blob, off + 8), ..Default::default()
            },
            MovementKind::DxDyDzDyaw => MovementFrame {
                dx: f32_at(blob, off), dy: f32_at(blob, off + 4),
                dz: f32_at(blob, off + 8), dyaw: f32_at(blob, off + 12),
            },
            MovementKind::DxDyDzDangleAxis => MovementFrame {
                dx: f32_at(blob, off), dy: f32_at(blob, off + 4),
                dz: f32_at(blob, off + 8), dyaw: f32_at(blob, off + 20),
            },
            MovementKind::None => MovementFrame::default(),
        };
        frames.push(f);
    }
    MovementData { kind, frames }
}

/// Read the 6 node-flag BitArrays from a blob given the start offset
/// and total byte sizes for each flag triplet (rotation/translation/
/// scale × static/animated). Each triplet is split into 3 equal-sized
/// u32 bitarrays. Returns `None` if both triplets are zero or sizes
/// don't divide cleanly into 3.
fn read_node_flags(
    blob: &[u8],
    static_off: usize,
    static_total: usize,
    animated_total: usize,
) -> Option<NodeFlags> {
    if static_total == 0 && animated_total == 0 { return None; }
    if static_total % 3 != 0 || animated_total % 3 != 0 { return None; }
    let static_end = static_off.checked_add(static_total)?;
    let animated_end = static_end.checked_add(animated_total)?;
    if animated_end > blob.len() { return None; }
    let static_per = static_total / 3;
    let animated_per = animated_total / 3;
    let mut out = NodeFlags::default();
    if static_per > 0 {
        out.static_rotation = BitArray::from_bytes(&blob[static_off..static_off + static_per]);
        out.static_translation = BitArray::from_bytes(&blob[static_off + static_per..static_off + 2 * static_per]);
        out.static_scale = BitArray::from_bytes(&blob[static_off + 2 * static_per..static_end]);
    }
    if animated_per > 0 {
        let a = static_end;
        out.animated_rotation = BitArray::from_bytes(&blob[a..a + animated_per]);
        out.animated_translation = BitArray::from_bytes(&blob[a + animated_per..a + 2 * animated_per]);
        out.animated_scale = BitArray::from_bytes(&blob[a + 2 * animated_per..animated_end]);
    }
    Some(out)
}

/// Compute the on-disk byte size of an animated codec stream so the
/// caller can locate the flag table that follows it.
///
/// - Fullframe codecs (slots 1, 2, 3, 8): size = `32 + n_rot * rot_stride
///   + n_trans * trans_stride + n_scale * scale_stride`. Equivalent to
///   `scale_offset + n_scale * scale_stride` but we compute from the
///   header strides directly to be robust to MCC reorderings.
/// - Keyframe codecs (slots 4-7): size = max-payload-end across the
///   three components. `*_key_payload_offset + count * sizeof(elem)`
///   for whichever component had the highest payload offset.
fn animated_codec_stream_size(blob: &[u8], codec: Codec) -> Option<usize> {
    use Codec::*;
    match codec {
        UncompressedStatic | UncompressedAnimated | EightByteQuantizedRotationOnly | BlendScreen => {
            let h = FullframeCodecHeader::from_bytes(blob)?;
            let n_rot = h.base.total_rotated_nodes as usize;
            let n_trans = h.base.total_translated_nodes as usize;
            let n_scale = h.base.total_scaled_nodes as usize;
            // Use header offsets when set (translation_offset and
            // scale_offset are absolute from codec base); fall back to
            // contiguous stride math when zero.
            let rot_end = 32 + n_rot * h.rotation_stride as usize;
            let trans_end = if n_trans > 0 {
                h.base.translation_offset as usize + n_trans * h.translation_stride as usize
            } else { 0 };
            let scale_end = if n_scale > 0 {
                h.base.scale_offset as usize + n_scale * h.scale_stride as usize
            } else { 0 };
            Some(rot_end.max(trans_end).max(scale_end))
        }
        ByteKeyframeLightlyQuantized | WordKeyframeLightlyQuantized
        | ReverseByteKeyframeLightlyQuantized | ReverseWordKeyframeLightlyQuantized => {
            let h = KeyframeCodecHeader::from_bytes(blob)?;
            let n_rot = h.base.total_rotated_nodes as usize;
            let n_trans = h.base.total_translated_nodes as usize;
            let n_scale = h.base.total_scaled_nodes as usize;
            // Per-node packed_data array sits right after the 48-byte
            // header. Sum the per-node `count` (low 12 bits) across
            // each component to get total keys.
            let key_count = |start: usize, count: usize| -> usize {
                (start..start + count)
                    .filter_map(|i| {
                        let off = 48 + i * 4;
                        let pd = u32::from_le_bytes(blob.get(off..off + 4)?.try_into().ok()?);
                        Some((pd & 0xFFF) as usize)
                    })
                    .sum()
            };
            let rot_keys = key_count(0, n_rot);
            let trans_keys = key_count(n_rot, n_trans);
            let scale_keys = key_count(n_rot + n_trans, n_scale);
            let rot_payload_end = h.rotation_key_payload_offset as usize + rot_keys * 8;
            let trans_payload_end = h.translation_key_payload_offset as usize + trans_keys * 12;
            let scale_payload_end = h.scale_key_payload_offset as usize + scale_keys * 4;
            // Time tables also have ends — include for safety.
            let time_size = match codec {
                ByteKeyframeLightlyQuantized | ReverseByteKeyframeLightlyQuantized => 1,
                _ => 2,
            };
            let rot_time_end = h.rotation_key_time_offset as usize + rot_keys * time_size;
            let trans_time_end = h.translation_key_time_offset as usize + trans_keys * time_size;
            let scale_time_end = h.scale_key_time_offset as usize + scale_keys * time_size;
            Some(
                rot_payload_end
                    .max(trans_payload_end)
                    .max(scale_payload_end)
                    .max(rot_time_end)
                    .max(trans_time_end)
                    .max(scale_time_end),
            )
        }
        _ => None,
    }
}

/// Slot 1: `c_uncompressed_static_data_codec`. One frame's worth of
/// transforms, packed:
///
/// - `rotations[i]`: `total_rotated_nodes` × 4× i16 (i,j,k,w),
///   contiguous starting at byte 32 (right after the fullframe header).
///   Each component decoded as `s / 32767.0`, then quaternion
///   normalized.
/// - `translations[i]`: `total_translated_nodes` × 3× f32, contiguous
///   starting at `translation_offset` from the codec base.
/// - `scales[i]`: `total_scaled_nodes` × 1× f32, contiguous starting
///   at `scale_offset` from the codec base.
///
/// Verified against `c_uncompressed_static_data_codec::decompress_*`
/// in the Halo 3 PDB / dllcache.
fn decode_uncompressed_static(blob: &[u8]) -> Result<AnimationTracks, AnimationError> {
    decode_fullframe(blob, Codec::UncompressedStatic, 1, /*quat_8byte=*/true)
}

/// Wrap a codec decode so any error demotes to
/// `AnimatedStreamStatus::Unsupported(codec)`. Used for animated-stream
/// dispatch where a Reach blob's coincidental codec_byte match
/// shouldn't fail the whole `decode()`.
fn try_animated(
    codec: Codec,
    decode: impl FnOnce() -> Result<AnimationTracks, AnimationError>,
) -> (Option<AnimationTracks>, AnimatedStreamStatus) {
    match decode() {
        Ok(t) => (Some(t), AnimatedStreamStatus::Decoded),
        Err(_) => (None, AnimatedStreamStatus::Unsupported(codec)),
    }
}

/// Shared fullframe decoder used by slots 1 (`frame_count = 1`) and 3
/// (`frame_count = animation.frame_count`). Generic over per-frame
/// count; the rotation-stride / translation-offset / scale-offset
/// fields in the header drive the per-node-outermost layout.
///
/// `quat_8byte = true` → 4× i16 quaternion (8 bytes). The slot-2 raw
/// `real_quaternion` variant (slot 8 BlendScreen also) is a future
/// path with `quat_8byte = false` (4× f32, 16 bytes per quat).
fn decode_fullframe(
    blob: &[u8],
    codec: Codec,
    frame_count: u16,
    quat_8byte: bool,
) -> Result<AnimationTracks, AnimationError> {
    let header = FullframeCodecHeader::from_bytes(blob)
        .ok_or(AnimationError::TruncatedHeader {
            codec, want: FullframeCodecHeader::SIZE, have: blob.len(),
        })?;

    let n_rot = header.base.total_rotated_nodes as usize;
    let n_trans = header.base.total_translated_nodes as usize;
    let n_scale = header.base.total_scaled_nodes as usize;
    let frames = frame_count as usize;
    let quat_size = if quat_8byte { 8 } else { 16 };

    // Rotation block: `node × rotation_stride + frame × quat_size`,
    // anchored at byte 32. For static (frame_count=1) the stride per
    // node should equal quat_size; for animated, stride = quat_size ×
    // frame_count.
    let rot_start = FullframeCodecHeader::SIZE;
    let rot_stride = header.rotation_stride as usize;
    let rot_end = rot_start
        .checked_add(n_rot.checked_mul(rot_stride).unwrap_or(usize::MAX))
        .ok_or(AnimationError::TruncatedPayload { codec, want_end: usize::MAX, blob_size: blob.len() })?;
    if rot_end > blob.len() {
        return Err(AnimationError::TruncatedPayload { codec, want_end: rot_end, blob_size: blob.len() });
    }

    let trans_start = header.base.translation_offset as usize;
    let trans_stride = header.translation_stride as usize;
    let trans_end = trans_start
        .checked_add(n_trans.checked_mul(trans_stride).unwrap_or(usize::MAX))
        .ok_or(AnimationError::TruncatedPayload { codec, want_end: usize::MAX, blob_size: blob.len() })?;
    if trans_end > blob.len() {
        return Err(AnimationError::TruncatedPayload { codec, want_end: trans_end, blob_size: blob.len() });
    }

    let scale_start = header.base.scale_offset as usize;
    let scale_stride = header.scale_stride as usize;
    let scale_end = scale_start
        .checked_add(n_scale.checked_mul(scale_stride).unwrap_or(usize::MAX))
        .ok_or(AnimationError::TruncatedPayload { codec, want_end: usize::MAX, blob_size: blob.len() })?;
    if scale_end > blob.len() {
        return Err(AnimationError::TruncatedPayload { codec, want_end: scale_end, blob_size: blob.len() });
    }

    let mut rotations = Vec::with_capacity(n_rot);
    for node in 0..n_rot {
        let mut frames_vec = Vec::with_capacity(frames);
        for f in 0..frames {
            let off = rot_start + node * rot_stride + f * quat_size;
            let q = if quat_8byte {
                RealQuaternion {
                    i: i16_to_unit(blob, off),
                    j: i16_to_unit(blob, off + 2),
                    k: i16_to_unit(blob, off + 4),
                    w: i16_to_unit(blob, off + 6),
                }
            } else {
                RealQuaternion {
                    i: f32_at(blob, off),
                    j: f32_at(blob, off + 4),
                    k: f32_at(blob, off + 8),
                    w: f32_at(blob, off + 12),
                }
            };
            frames_vec.push(normalize_quat(q));
        }
        rotations.push(frames_vec);
    }

    let mut translations = Vec::with_capacity(n_trans);
    for node in 0..n_trans {
        let mut frames_vec = Vec::with_capacity(frames);
        for f in 0..frames {
            let off = trans_start + node * trans_stride + f * 12;
            frames_vec.push(RealPoint3d {
                x: f32_at(blob, off),
                y: f32_at(blob, off + 4),
                z: f32_at(blob, off + 8),
            });
        }
        translations.push(frames_vec);
    }

    let mut scales = Vec::with_capacity(n_scale);
    for node in 0..n_scale {
        let mut frames_vec = Vec::with_capacity(frames);
        for f in 0..frames {
            let off = scale_start + node * scale_stride + f * 4;
            frames_vec.push(f32_at(blob, off));
        }
        scales.push(frames_vec);
    }

    Ok(AnimationTracks {
        codec,
        frame_count,
        rotations,
        translations,
        scales,
    })
}


/// Slots 4 / 5 / 6 / 7 — `c_keyframe_codec_template`.
///
/// Layout, after the 48-byte [`KeyframeCodecHeader`]:
/// - Per-node `packed_data` u32 array, in order: rotation nodes,
///   translation nodes, scale nodes. Each entry encodes
///   `(time_offset << 12) | count` — `time_offset` is the index into
///   that component's time/payload tables where this node's keys
///   start; `count` is how many keys this node has.
/// - Time table for each component starts at the matching
///   `*_key_time_offset` from the header. Entries are u8 (slots 4/6)
///   or u16 (slots 5/7).
/// - Payload table for each component starts at the matching
///   `*_key_payload_offset` from the header. Entries are sizeof(elem):
///   8 bytes for the 4×i16 quaternion, 12 for `real_point3d`, 4 for
///   `float`.
///
/// Forward (slots 4/5) and reverse (slots 6/7) keyfinder variants
/// produce **bit-identical** byte layouts — the same decoder reads
/// both. (TagTool's `.Reverse()` workaround in
/// `ReverseKeyframeLightlyQuantizedCodec.Read` is a bug stemming from
/// not parsing `packed_data` properly; we don't replicate it.)
///
/// Output is densely interpolated to `frame_count` per node — short-arc
/// nlerp for quaternions, linear for translations and scales. Single-
/// key nodes hold the value across all frames; out-of-bracket frame
/// indices clamp to the first/last key.
fn decode_keyframe(
    blob: &[u8],
    codec: Codec,
    frame_count: u16,
    time_byte_size: usize,
) -> Result<AnimationTracks, AnimationError> {
    let header = KeyframeCodecHeader::from_bytes(blob)
        .ok_or(AnimationError::TruncatedHeader {
            codec, want: KeyframeCodecHeader::SIZE, have: blob.len(),
        })?;

    let n_rot = header.base.total_rotated_nodes as usize;
    let n_trans = header.base.total_translated_nodes as usize;
    let n_scale = header.base.total_scaled_nodes as usize;

    // Per-node packed_data array starts at offset 48 (right after
    // header). Order: rotation nodes, translation nodes, scale nodes.
    let packed_start = KeyframeCodecHeader::SIZE;
    let packed_total = n_rot + n_trans + n_scale;
    let packed_end = packed_start
        .checked_add(packed_total.checked_mul(4).unwrap_or(usize::MAX))
        .ok_or(AnimationError::TruncatedPayload { codec, want_end: usize::MAX, blob_size: blob.len() })?;
    if packed_end > blob.len() {
        return Err(AnimationError::TruncatedPayload { codec, want_end: packed_end, blob_size: blob.len() });
    }

    let read_packed = |idx: usize| -> (u32, u32) {
        let off = packed_start + idx * 4;
        let pd = u32::from_le_bytes(blob[off..off + 4].try_into().unwrap());
        (pd >> 12, pd & 0xFFF) // (time_offset, key_count)
    };

    // Decode one component (rotations, translations, or scales) for
    // all of its nodes. Generic over element type via closures so
    // we don't repeat the per-node bracket-finding loop three times.
    fn decode_component<T, F>(
        blob: &[u8],
        codec: Codec,
        frame_count: u16,
        time_byte_size: usize,
        time_table_start: usize,
        payload_table_start: usize,
        element_size: usize,
        node_packs: impl Iterator<Item = (u32, u32)>,
        identity: T,
        read_element: impl Fn(&[u8], usize) -> T,
        interpolate: F,
    ) -> Result<Vec<Vec<T>>, AnimationError>
    where
        T: Clone,
        F: Fn(&T, &T, f32) -> T,
    {
        let mut out = Vec::new();
        for (time_off, key_count) in node_packs {
            let key_count = key_count as usize;
            let frames_count = frame_count as usize;
            if key_count == 0 {
                out.push(vec![identity.clone(); frames_count]);
                continue;
            }
            let time_start = time_table_start + (time_off as usize) * time_byte_size;
            let time_end = time_start + key_count * time_byte_size;
            let payload_start = payload_table_start + (time_off as usize) * element_size;
            let payload_end = payload_start + key_count * element_size;
            if time_end > blob.len() || payload_end > blob.len() {
                return Err(AnimationError::TruncatedPayload {
                    codec, want_end: time_end.max(payload_end), blob_size: blob.len(),
                });
            }
            let read_time = |i: usize| -> u32 {
                let off = time_start + i * time_byte_size;
                match time_byte_size {
                    1 => blob[off] as u32,
                    2 => u16::from_le_bytes([blob[off], blob[off + 1]]) as u32,
                    _ => unreachable!("time_byte_size must be 1 or 2"),
                }
            };
            let read_value = |key_idx: usize| -> T {
                let off = payload_start + key_idx * element_size;
                read_element(blob, off)
            };

            let mut frames = Vec::with_capacity(frames_count);
            if key_count == 1 {
                let v = read_value(0);
                for _ in 0..frames_count { frames.push(v.clone()); }
                out.push(frames);
                continue;
            }

            // Bracket finder: largest i in [0, key_count) such that
            // time_table[i] <= frame_idx. Linear scan is fine — keys
            // per node are typically a handful, rarely more than ~30.
            for frame_idx in 0..frames_count as u32 {
                let mut bracket = 0usize;
                for i in 0..key_count {
                    if read_time(i) <= frame_idx { bracket = i; } else { break; }
                }
                if bracket == key_count - 1 {
                    frames.push(read_value(bracket));
                    continue;
                }
                let t_a = read_time(bracket) as f32;
                let t_b = read_time(bracket + 1) as f32;
                let t = if t_b > t_a { (frame_idx as f32 - t_a) / (t_b - t_a) } else { 0.0 };
                let va = read_value(bracket);
                let vb = read_value(bracket + 1);
                frames.push(interpolate(&va, &vb, t));
            }
            out.push(frames);
        }
        Ok(out)
    }

    let rot_packs: Vec<_> = (0..n_rot).map(read_packed).collect();
    let trans_packs: Vec<_> = (n_rot..n_rot + n_trans).map(read_packed).collect();
    let scale_packs: Vec<_> = (n_rot + n_trans..packed_total).map(read_packed).collect();

    let rotations = decode_component(
        blob, codec, frame_count, time_byte_size,
        header.rotation_key_time_offset as usize,
        header.rotation_key_payload_offset as usize,
        /*element_size=*/8,
        rot_packs.into_iter(),
        identity_quat(),
        |b, off| normalize_quat(RealQuaternion {
            i: i16_to_unit(b, off),
            j: i16_to_unit(b, off + 2),
            k: i16_to_unit(b, off + 4),
            w: i16_to_unit(b, off + 6),
        }),
        nlerp_short_arc,
    )?;
    let translations = decode_component(
        blob, codec, frame_count, time_byte_size,
        header.translation_key_time_offset as usize,
        header.translation_key_payload_offset as usize,
        /*element_size=*/12,
        trans_packs.into_iter(),
        RealPoint3d::default(),
        |b, off| RealPoint3d {
            x: f32_at(b, off), y: f32_at(b, off + 4), z: f32_at(b, off + 8),
        },
        |a, b, t| RealPoint3d {
            x: a.x + (b.x - a.x) * t,
            y: a.y + (b.y - a.y) * t,
            z: a.z + (b.z - a.z) * t,
        },
    )?;
    let scales = decode_component(
        blob, codec, frame_count, time_byte_size,
        header.scale_key_time_offset as usize,
        header.scale_key_payload_offset as usize,
        /*element_size=*/4,
        scale_packs.into_iter(),
        1.0f32,
        |b, off| f32_at(b, off),
        |a, b, t| a + (b - a) * t,
    )?;

    Ok(AnimationTracks { codec, frame_count, rotations, translations, scales })
}

fn identity_quat() -> RealQuaternion {
    RealQuaternion { i: 0.0, j: 0.0, k: 0.0, w: 1.0 }
}

/// Tiny seekable byte cursor — lets the curve decoder mirror Foundry's
/// position-based read pattern (read forward, occasionally `skip(-6)`
/// to back up so the next keyframe's `p1` reads where the previous
/// keyframe's `p2` was).
struct Cursor<'a> { data: &'a [u8], pos: usize }
impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self { Self { data, pos: 0 } }
    fn seek(&mut self, off: usize) -> Result<(), AnimationError> {
        if off > self.data.len() {
            return Err(AnimationError::TruncatedPayload {
                codec: Codec::Curve, want_end: off, blob_size: self.data.len(),
            });
        }
        self.pos = off; Ok(())
    }
    fn skip(&mut self, delta: i32) {
        if delta >= 0 { self.pos = self.pos.saturating_add(delta as usize); }
        else { self.pos = self.pos.saturating_sub((-delta) as usize); }
    }
    fn read_u8(&mut self) -> Result<u8, AnimationError> {
        let v = *self.data.get(self.pos).ok_or(AnimationError::TruncatedPayload {
            codec: Codec::Curve, want_end: self.pos + 1, blob_size: self.data.len(),
        })?;
        self.pos += 1; Ok(v)
    }
    fn read_u16(&mut self) -> Result<u16, AnimationError> {
        let bs = self.data.get(self.pos..self.pos + 2).ok_or(AnimationError::TruncatedPayload {
            codec: Codec::Curve, want_end: self.pos + 2, blob_size: self.data.len(),
        })?;
        let v = u16::from_le_bytes([bs[0], bs[1]]);
        self.pos += 2; Ok(v)
    }
    fn read_s16(&mut self) -> Result<i16, AnimationError> {
        Ok(self.read_u16()? as i16)
    }
    fn read_u32(&mut self) -> Result<u32, AnimationError> {
        let bs = self.data.get(self.pos..self.pos + 4).ok_or(AnimationError::TruncatedPayload {
            codec: Codec::Curve, want_end: self.pos + 4, blob_size: self.data.len(),
        })?;
        let v = u32::from_le_bytes([bs[0], bs[1], bs[2], bs[3]]);
        self.pos += 4; Ok(v)
    }
    fn read_f32(&mut self) -> Result<f32, AnimationError> {
        Ok(f32::from_bits(self.read_u32()?))
    }
}

/// Slot 9 — Curve codec. Per-component (rotation/translation/scale),
/// each node has a packed payload starting at
/// `payload_data_offset + node_offset` (where `node_offset` is read
/// from a per-node u32 array right after the codec header).
///
/// Per-node payload header:
/// - u16 (unused), u16 key_count, u8 flags, u8 (unused), s16 (unused)
/// - For translation: + 4 f32 (offset_x/y/z, scale)
/// - For scale:       + 2 f32 (offset, scale)
///
/// If `flags & 1` is set, each frame stores a direct value. Otherwise,
/// `key_count` u8 deltas (cumulative-sum into keyframe indices) are
/// followed by per-frame curve segments. A keyframe segment is:
/// `p1 (i16s), tangent_bytes (4×u8 for quat / 3×u8 for vec / 1×u8 for
/// scalar), p2 (i16s)` — then the cursor backs up by `2 × element_size`
/// so `p2` becomes the next segment's `p1`. Frames between keyframes
/// are produced by cubic Hermite using the tangent bytes.
///
/// Quaternion decompression: input has 3 i16 values (i, j, w);
/// the missing component k is reconstructed via
/// `k = sqrt(max(1 - i² - j², 0))`, sign-flipped if `w < 0`, then
/// `w := 2|w| - 1` and all components scale by `sqrt(max(1 - w², 0))`
/// before final normalization. Mirrors Foundry's `_decompress_quat`.
fn decode_curve(
    blob: &[u8],
    codec: Codec,
    frame_count: u16,
    revised: bool,
) -> Result<AnimationTracks, AnimationError> {
    let mut c = Cursor::new(blob);
    if blob.len() < 32 {
        return Err(AnimationError::TruncatedHeader { codec, want: 32, have: blob.len() });
    }
    // 12-byte base header (we already validated codec_byte before
    // dispatching here — just consume).
    c.skip(12);
    let translation_data_offset = c.read_u32()? as usize;
    let scale_data_offset = c.read_u32()? as usize;
    let payload_data_offset = c.read_u32()? as usize;
    let total_compressed_size = c.read_u32()? as usize;
    c.read_u32()?; // reserved/unused

    let n_rot = blob[1] as usize;
    let n_trans = blob[2] as usize;
    let n_scale = blob[3] as usize;
    let frames = frame_count as usize;

    // Per-rotation-node u32 offsets array sits right after the 32-byte
    // header (we're at position 32 now after reading the 5 u32 + skip 12).
    let mut rotation_offsets = Vec::with_capacity(n_rot);
    for _ in 0..n_rot { rotation_offsets.push(c.read_u32()? as usize); }

    let mut rotations = Vec::with_capacity(n_rot);
    for &node_off in &rotation_offsets {
        c.seek(payload_data_offset + node_off)?;
        rotations.push(read_curve_rotation_node(&mut c, frames, revised)?);
    }

    let mut translations = Vec::with_capacity(n_trans);
    if n_trans > 0 {
        c.seek(payload_data_offset + translation_data_offset)?;
        let mut trans_offsets = Vec::with_capacity(n_trans);
        for _ in 0..n_trans { trans_offsets.push(c.read_u32()? as usize); }
        for &node_off in &trans_offsets {
            c.seek(payload_data_offset + node_off)?;
            translations.push(read_curve_translation_node(&mut c, frames)?);
        }
    }

    let mut scales = Vec::with_capacity(n_scale);
    if n_scale > 0 {
        c.seek(payload_data_offset + scale_data_offset)?;
        let mut scale_offsets = Vec::with_capacity(n_scale);
        for _ in 0..n_scale { scale_offsets.push(c.read_u32()? as usize); }
        for &node_off in &scale_offsets {
            c.seek(payload_data_offset + node_off)?;
            scales.push(read_curve_scale_node(&mut c, frames)?);
        }
    }

    // Position is now wherever the last per-node read left it; the
    // Reach get_data_offset model uses the explicit cumulative-sum
    // size from `data sizes`, so we don't need to advance to
    // total_compressed_size — but skip if needed for correctness.
    let _ = total_compressed_size;

    Ok(AnimationTracks { codec, frame_count, rotations, translations, scales })
}

fn read_curve_rotation_node(c: &mut Cursor<'_>, frames: usize, revised: bool) -> Result<Vec<RealQuaternion>, AnimationError> {
    c.read_u16()?; // unused
    let key_count = c.read_u16()? as usize;
    let flags = c.read_u8()?;
    c.read_u8()?; // unused
    c.read_s16()?; // unused
    let keyframes = if flags & 1 == 0 { read_curve_keyframe_deltas(c, key_count)? } else { Vec::new() };

    let read_quat = |c: &mut Cursor<'_>| -> Result<RealQuaternion, AnimationError> {
        let v3 = c.read_s16()?;
        let v4 = c.read_s16()?;
        let v5 = c.read_s16()?;
        Ok(if revised {
            decompress_revised_quat(v3, v4, v5)
        } else {
            decompress_curve_quat(
                v3 as f32 / i16::MAX as f32,
                v4 as f32 / i16::MAX as f32,
                v5 as f32 / i16::MAX as f32,
            )
        })
    };

    let mut out = Vec::with_capacity(frames);
    let mut p1 = identity_quat();
    let mut p2 = identity_quat();
    let mut tangent_bytes = [0u8; 4];
    let mut current_kf = 0u32;
    let mut next_kf = 0u32;
    let mut keyframe_index = 0usize;
    for frame_index in 0..frames as u32 {
        let q = if flags & 1 != 0 {
            read_quat(c)?
        } else {
            if keyframe_index < keyframes.len()
                && keyframes[keyframe_index] == frame_index
                && frame_index < frames as u32 - 1
            {
                p1 = read_quat(c)?;
                tangent_bytes = [c.read_u8()?, c.read_u8()?, c.read_u8()?, c.read_u8()?];
                p2 = read_quat(c)?;
                current_kf = keyframes[keyframe_index];
                next_kf = keyframes.get(keyframe_index + 1).copied().unwrap_or(current_kf + 1);
                keyframe_index += 1;
                c.skip(-6); // p2 becomes next segment's p1
            }
            let span = (next_kf.saturating_sub(current_kf) as f32).max(1.0);
            let t = (frame_index.saturating_sub(current_kf) as f32) / span;
            let tan1 = curve_tangent_quat(
                ((tangent_bytes[0] >> 4) as i32) - 7,
                ((tangent_bytes[1] >> 4) as i32) - 7,
                ((tangent_bytes[2] >> 4) as i32) - 7,
                ((tangent_bytes[3] >> 4) as i32) - 7,
                p1, p2,
            );
            let tan2 = curve_tangent_quat(
                ((tangent_bytes[0] & 0x0F) as i32) - 7,
                ((tangent_bytes[1] & 0x0F) as i32) - 7,
                ((tangent_bytes[2] & 0x0F) as i32) - 7,
                ((tangent_bytes[3] & 0x0F) as i32) - 7,
                p1, p2,
            );
            curve_position_quat(t, tan1, tan2, p1, p2)
        };
        out.push(q);
    }
    Ok(out)
}

fn read_curve_translation_node(c: &mut Cursor<'_>, frames: usize) -> Result<Vec<RealPoint3d>, AnimationError> {
    c.read_u16()?; // unused
    let key_count = c.read_u16()? as usize;
    let flags = c.read_u8()?;
    c.read_u8()?; // unused
    c.read_u16()?; // unused
    let offset_x = c.read_f32()?;
    let offset_y = c.read_f32()?;
    let offset_z = c.read_f32()?;
    let scale = c.read_f32()?;
    let keyframes = if flags & 1 == 0 { read_curve_keyframe_deltas(c, key_count)? } else { Vec::new() };

    let mut out = Vec::with_capacity(frames);
    let mut p1 = RealPoint3d::default();
    let mut p2 = RealPoint3d::default();
    let mut tangent_bytes = [0u8; 3];
    let mut current_kf = 0u32;
    let mut next_kf = 0u32;
    let mut keyframe_index = 0usize;
    for frame_index in 0..frames as u32 {
        let v = if flags & 1 != 0 {
            RealPoint3d {
                x: c.read_s16()? as f32 / i16::MAX as f32,
                y: c.read_s16()? as f32 / i16::MAX as f32,
                z: c.read_s16()? as f32 / i16::MAX as f32,
            }
        } else {
            if keyframe_index < keyframes.len()
                && keyframes[keyframe_index] == frame_index
                && frame_index < frames as u32 - 1
            {
                let x1 = c.read_s16()? as f32 / i16::MAX as f32;
                let y1 = c.read_s16()? as f32 / i16::MAX as f32;
                let z1 = c.read_s16()? as f32 / i16::MAX as f32;
                tangent_bytes = [c.read_u8()?, c.read_u8()?, c.read_u8()?];
                let x2 = c.read_s16()? as f32 / i16::MAX as f32;
                let y2 = c.read_s16()? as f32 / i16::MAX as f32;
                let z2 = c.read_s16()? as f32 / i16::MAX as f32;
                p1 = RealPoint3d { x: x1, y: y1, z: z1 };
                p2 = RealPoint3d { x: x2, y: y2, z: z2 };
                current_kf = keyframes[keyframe_index];
                next_kf = keyframes.get(keyframe_index + 1).copied().unwrap_or(current_kf + 1);
                keyframe_index += 1;
                c.skip(-6);
            }
            let span = (next_kf.saturating_sub(current_kf) as f32).max(1.0);
            let t = (frame_index.saturating_sub(current_kf) as f32) / span;
            let tan1 = curve_tangent_vec(
                ((tangent_bytes[0] >> 4) as i32) - 7,
                ((tangent_bytes[1] >> 4) as i32) - 7,
                ((tangent_bytes[2] >> 4) as i32) - 7,
                p1, p2,
            );
            let tan2 = curve_tangent_vec(
                ((tangent_bytes[0] & 0x0F) as i32) - 7,
                ((tangent_bytes[1] & 0x0F) as i32) - 7,
                ((tangent_bytes[2] & 0x0F) as i32) - 7,
                p1, p2,
            );
            curve_position_vec(t, tan1, tan2, p1, p2)
        };
        out.push(RealPoint3d {
            x: scale * v.x + offset_x,
            y: scale * v.y + offset_y,
            z: scale * v.z + offset_z,
        });
    }
    Ok(out)
}

fn read_curve_scale_node(c: &mut Cursor<'_>, frames: usize) -> Result<Vec<f32>, AnimationError> {
    c.read_u16()?;
    let key_count = c.read_u16()? as usize;
    let flags = c.read_u8()?;
    c.read_u8()?;
    c.read_u16()?;
    let offset = c.read_f32()?;
    let scale = c.read_f32()?;
    let keyframes = if flags & 1 == 0 { read_curve_keyframe_deltas(c, key_count)? } else { Vec::new() };

    let mut out = Vec::with_capacity(frames);
    let mut p1 = 0.0f32;
    let mut p2 = 0.0f32;
    let mut tangent_byte = 0u8;
    let mut current_kf = 0u32;
    let mut next_kf = 0u32;
    let mut keyframe_index = 0usize;
    for frame_index in 0..frames as u32 {
        let v = if flags & 1 != 0 {
            c.read_s16()? as f32 / i16::MAX as f32
        } else {
            if keyframe_index < keyframes.len()
                && keyframes[keyframe_index] == frame_index
                && frame_index < frames as u32 - 1
            {
                p1 = c.read_s16()? as f32 / i16::MAX as f32;
                tangent_byte = c.read_u8()?;
                p2 = c.read_s16()? as f32 / i16::MAX as f32;
                current_kf = keyframes[keyframe_index];
                next_kf = keyframes.get(keyframe_index + 1).copied().unwrap_or(current_kf + 1);
                keyframe_index += 1;
                c.skip(-2);
            }
            let span = (next_kf.saturating_sub(current_kf) as f32).max(1.0);
            let t = (frame_index.saturating_sub(current_kf) as f32) / span;
            let tan1 = curve_tangent_scalar(((tangent_byte >> 4) as i32) - 7, p1, p2);
            let tan2 = curve_tangent_scalar(((tangent_byte & 0x0F) as i32) - 7, p1, p2);
            curve_position_scalar(t, tan1, tan2, p1, p2)
        };
        out.push(v * scale + offset);
    }
    Ok(out)
}

/// Read `key_count` u8 keyframe deltas, prepended by an implicit 0,
/// cumulative-summed into absolute frame indices. Mirrors Foundry's
/// `_read_curve_keyframe_data`.
fn read_curve_keyframe_deltas(c: &mut Cursor<'_>, key_count: usize) -> Result<Vec<u32>, AnimationError> {
    let mut keyframes = Vec::with_capacity(key_count + 1);
    keyframes.push(0u32);
    let mut total = 0u32;
    for _ in 0..key_count {
        total = total.saturating_add(c.read_u8()? as u32);
        keyframes.push(total);
    }
    Ok(keyframes)
}

fn decompress_curve_quat(i: f32, j: f32, w: f32) -> RealQuaternion {
    let mut k = (1.0 - i * i - j * j).max(0.0).sqrt();
    if w < 0.0 { k = -k; }
    let w_unfolded = w.abs() * 2.0 - 1.0;
    let scale = (1.0 - w_unfolded * w_unfolded).max(0.0).sqrt();
    normalize_quat(RealQuaternion {
        i: i * scale, j: j * scale, k: k * scale, w: w_unfolded,
    })
}

/// Slot 10 (RevisedCurve, H4-era) quaternion decompression. Stores 3
/// of 4 components as i16, with the low bit of each value stealing
/// metadata: bit 0 of `v3` flips the sign of the reconstructed
/// component; bits 0 of `v4` (×2) and `v5` together encode which of
/// the four output slots holds the reconstructed (largest-magnitude)
/// component. Components are scaled by `sqrt(0.5)` because the
/// largest-magnitude component is at most that value (when the other
/// three are equal in unit-length quaternions).
///
/// Implementation matches Foundry's `_decompress_revised_quat`
/// (`animation_resource.py:747`) using the "cache" rotation_layout
/// which is what MCC re-imports use. The "h4_source" layout is for
/// raw H4 source jmads (uncompiled .source variant) and isn't seen
/// in the MCC corpus.
fn decompress_revised_quat(v3: i16, v4: i16, v5: i16) -> RealQuaternion {
    const SQRT_HALF: f32 = 0.707_106_77;
    // Strip the low metadata bit from each value, preserving sign.
    let i = ((v3 & !1i16) as f32 / i16::MAX as f32) * SQRT_HALF;
    let j = ((v4 & !1i16) as f32 / i16::MAX as f32) * SQRT_HALF;
    let k = ((v5 & !1i16) as f32 / i16::MAX as f32) * SQRT_HALF;
    let mut missing = (1.0 - i * i - j * j - k * k).max(0.0).sqrt();
    if v3 & 1 != 0 { missing = -missing; }
    let component_index = ((v5 & 1) as usize) | ((2 * (v4 & 1)) as usize);
    // Cache layout: place i/j/k at offsets +1 / -2 / -1 from the
    // missing-component slot, mod 4. Indices are bounded to 0..=3.
    let mut output = [0.0f32; 4];
    output[(component_index + 1) & 3] = i;
    output[(component_index + 2) & 3] = j; // (-2 mod 4) == +2
    output[(component_index + 3) & 3] = k; // (-1 mod 4) == +3
    output[component_index] = missing;
    normalize_quat(RealQuaternion {
        i: output[0], j: output[1], k: output[2], w: output[3],
    })
}

/// Curve tangent for a single component. `tangent_signed` is the
/// nibble's signed value (0..=15 → -7..=8). Result is the Hermite
/// tangent used in `curve_position_scalar`.
fn curve_tangent_scalar(tangent_signed: i32, p1: f32, p2: f32) -> f32 {
    let t = tangent_signed as f32 / 7.0;
    t.abs() * (t * 0.300_000_011_920_929) + (p2 - p1)
}

fn curve_tangent_quat(it: i32, jt: i32, kt: i32, wt: i32, p1: RealQuaternion, p2: RealQuaternion) -> [f32; 4] {
    [
        curve_tangent_scalar(it, p1.i, p2.i),
        curve_tangent_scalar(jt, p1.j, p2.j),
        curve_tangent_scalar(kt, p1.k, p2.k),
        curve_tangent_scalar(wt, p1.w, p2.w),
    ]
}

fn curve_tangent_vec(xt: i32, yt: i32, zt: i32, p1: RealPoint3d, p2: RealPoint3d) -> [f32; 3] {
    [
        curve_tangent_scalar(xt, p1.x, p2.x),
        curve_tangent_scalar(yt, p1.y, p2.y),
        curve_tangent_scalar(zt, p1.z, p2.z),
    ]
}

/// Cubic Hermite curve evaluation at `time` ∈ [0, 1].
fn curve_position_scalar(t: f32, tan1: f32, tan2: f32, p1: f32, p2: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    let h1 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h2 = t3 - 2.0 * t2 + t;
    let h3 = 3.0 * t2 - 2.0 * t3;
    let h4 = t3 - t2;
    h1 * p1 + h2 * tan1 + h3 * p2 + h4 * tan2
}

fn curve_position_quat(t: f32, tan1: [f32; 4], tan2: [f32; 4], p1: RealQuaternion, p2: RealQuaternion) -> RealQuaternion {
    normalize_quat(RealQuaternion {
        i: curve_position_scalar(t, tan1[0], tan2[0], p1.i, p2.i),
        j: curve_position_scalar(t, tan1[1], tan2[1], p1.j, p2.j),
        k: curve_position_scalar(t, tan1[2], tan2[2], p1.k, p2.k),
        w: curve_position_scalar(t, tan1[3], tan2[3], p1.w, p2.w),
    })
}

fn curve_position_vec(t: f32, tan1: [f32; 3], tan2: [f32; 3], p1: RealPoint3d, p2: RealPoint3d) -> RealPoint3d {
    RealPoint3d {
        x: curve_position_scalar(t, tan1[0], tan2[0], p1.x, p2.x),
        y: curve_position_scalar(t, tan1[1], tan2[1], p1.y, p2.y),
        z: curve_position_scalar(t, tan1[2], tan2[2], p1.z, p2.z),
    }
}

/// Short-arc normalized lerp for unit quaternions. Picks the sign of
/// `b` that yields the shorter arc relative to `a` (so `slerp(a, -b)`
/// = `slerp(a, b)` when they're 180° apart on the wrong side), then
/// linearly interpolates and re-normalizes. Mirrors the H3 binary's
/// `fast_short_arc_quaternion_interpolate_and_normalize`.
fn nlerp_short_arc(a: &RealQuaternion, b: &RealQuaternion, t: f32) -> RealQuaternion {
    let dot = a.i * b.i + a.j * b.j + a.k * b.k + a.w * b.w;
    let s = if dot < 0.0 { -1.0 } else { 1.0 };
    let one_minus_t = 1.0 - t;
    normalize_quat(RealQuaternion {
        i: a.i * one_minus_t + s * b.i * t,
        j: a.j * one_minus_t + s * b.j * t,
        k: a.k * one_minus_t + s * b.k * t,
        w: a.w * one_minus_t + s * b.w * t,
    })
}

/// Decode an int16 component as `s / 32767.0`. Matches the H3 binary's
/// `c_quantized_quaternion_8byte::decompress` (constant 0x38000100,
/// approximately 1/32767).
fn i16_to_unit(blob: &[u8], off: usize) -> f32 {
    let raw = i16::from_le_bytes([blob[off], blob[off + 1]]);
    raw as f32 / i16::MAX as f32
}

fn f32_at(blob: &[u8], off: usize) -> f32 {
    f32::from_le_bytes(blob[off..off + 4].try_into().unwrap())
}

// ---------------------------------------------------------------------
// Skeleton + Pose + JMA-family export.
// ---------------------------------------------------------------------

/// One node in the jmad's `definitions/skeleton nodes` block.
#[derive(Debug, Clone)]
pub struct SkeletonNode {
    pub name: String,
    /// Index into `Skeleton::nodes` of the first child, or `-1`.
    pub first_child: i16,
    /// Index of the next sibling under the same parent, or `-1`.
    pub next_sibling: i16,
    /// Index of the parent node, or `-1` for root.
    pub parent: i16,
}

/// jmad skeleton — the bone hierarchy that animations target.
#[derive(Debug, Clone)]
pub struct Skeleton {
    pub nodes: Vec<SkeletonNode>,
}

impl Skeleton {
    /// Walk `definitions/skeleton nodes` (or `resources/skeleton nodes`
    /// for older inline-layout tags) into a flat list of nodes.
    /// Returns an empty skeleton if the block is missing.
    pub fn from_tag(tag: &TagFile) -> Self {
        let root = tag.root();
        for prefix in TOP_LEVEL_NAMES {
            if let Some(block) = root
                .field_path(&format!("{prefix}/skeleton nodes"))
                .and_then(|f| f.as_block())
            {
                let mut nodes = Vec::with_capacity(block.len());
                for i in 0..block.len() {
                    let Some(elem) = block.element(i) else { continue };
                    let name = read_string_id(&elem, "name").unwrap_or_default();
                    let first_child = read_block_index(&elem, "first child node index");
                    let next_sibling = read_block_index(&elem, "next sibling node index");
                    let parent = read_block_index(&elem, "parent node index");
                    nodes.push(SkeletonNode { name, first_child, next_sibling, parent });
                }
                return Self { nodes };
            }
        }
        Self { nodes: Vec::new() }
    }

    /// Number of skeleton nodes (bones).
    pub fn len(&self) -> usize { self.nodes.len() }

    /// `true` when the tag has no skeleton nodes (e.g. inheriting jmads).
    pub fn is_empty(&self) -> bool { self.nodes.is_empty() }
}

fn read_block_index(s: &TagStruct<'_>, name: &str) -> i16 {
    match s.field(name).and_then(|f| f.value()) {
        Some(TagFieldData::CharBlockIndex(v)) => v as i16,
        Some(TagFieldData::ShortBlockIndex(v)) => v,
        Some(TagFieldData::LongBlockIndex(v)) => v as i16,
        _ => -1,
    }
}

/// One bone's transform at one frame — the unit JMA writes per
/// `(frame, node)` cell.
#[derive(Debug, Clone, Copy, Default)]
pub struct NodeTransform {
    pub rotation: RealQuaternion,
    pub translation: RealPoint3d,
    pub scale: f32,
}

/// Per-frame, per-bone transform table. `frames[frame_index][bone_index]`.
#[derive(Debug, Clone)]
pub struct Pose {
    pub frames: Vec<Vec<NodeTransform>>,
}

impl AnimationClip {
    /// Compose `static_tracks` + `animated_tracks` against the skeleton
    /// using the per-component `node_flags` bitarrays. Result has one
    /// `NodeTransform` per (frame, skeleton bone).
    ///
    /// Bones with neither flag set fall back to identity (rotation =
    /// (0,0,0,1), translation = (0,0,0), scale = 1.0). This is wrong
    /// for the rare bones whose rest pose lives in the skeleton's own
    /// `z_pos`/`base_vector` fields, but most exported animations
    /// have all bones in the static or animated set so the fallback
    /// rarely matters in practice.
    pub fn pose(&self, skeleton: &Skeleton) -> Pose {
        let bones = skeleton.len();
        let frames_n = self.frame_count.max(1) as usize;
        let mut frames = Vec::with_capacity(frames_n);

        // Pre-resolve every bone's source: which codec stream owns each
        // component, and what the codec_node_index is.
        let resolutions: Vec<BoneResolution> = (0..bones)
            .map(|b| BoneResolution::for_bone(b, self.node_flags.as_ref()))
            .collect();

        for f in 0..frames_n {
            let mut row = Vec::with_capacity(bones);
            for res in &resolutions {
                let rotation = pick_rotation(self, res, f).unwrap_or(identity_quat());
                let translation = pick_translation(self, res, f).unwrap_or(RealPoint3d::default());
                let scale = pick_scale(self, res, f).unwrap_or(1.0);
                row.push(NodeTransform { rotation, translation, scale });
            }
            frames.push(row);
        }
        Pose { frames }
    }
}

#[derive(Debug, Clone, Copy)]
struct BoneResolution {
    rotation: TrackSource,
    translation: TrackSource,
    scale: TrackSource,
}

#[derive(Debug, Clone, Copy)]
enum TrackSource {
    Static(usize),
    Animated(usize),
    Identity,
}

impl BoneResolution {
    fn for_bone(bone: usize, flags: Option<&NodeFlags>) -> Self {
        match flags {
            None => {
                // No flags available — fall back to "all bones use the
                // static track in skeleton order". Right for the most
                // common static-only case (mongoose-class inline
                // layouts), wrong for tagged bone subsets — but those
                // tags carry node_flags so we won't take this path.
                Self {
                    rotation: TrackSource::Static(bone),
                    translation: TrackSource::Static(bone),
                    scale: TrackSource::Static(bone),
                }
            }
            Some(f) => Self {
                rotation: pick_source(bone, &f.static_rotation, &f.animated_rotation),
                translation: pick_source(bone, &f.static_translation, &f.animated_translation),
                scale: pick_source(bone, &f.static_scale, &f.animated_scale),
            }
        }
    }
}

fn pick_source(bone: usize, static_flags: &BitArray, animated_flags: &BitArray) -> TrackSource {
    if static_flags.bit(bone) { TrackSource::Static(static_flags.popcount_below(bone)) }
    else if animated_flags.bit(bone) { TrackSource::Animated(animated_flags.popcount_below(bone)) }
    else { TrackSource::Identity }
}

fn pick_rotation(clip: &AnimationClip, res: &BoneResolution, frame: usize) -> Option<RealQuaternion> {
    match res.rotation {
        TrackSource::Static(i) => clip.static_tracks.rotations.get(i).and_then(|f| f.first()).copied(),
        TrackSource::Animated(i) => clip.animated_tracks.as_ref()
            .and_then(|t| t.rotations.get(i)).and_then(|f| f.get(frame.min(f.len() - 1))).copied(),
        TrackSource::Identity => None,
    }
}
fn pick_translation(clip: &AnimationClip, res: &BoneResolution, frame: usize) -> Option<RealPoint3d> {
    match res.translation {
        TrackSource::Static(i) => clip.static_tracks.translations.get(i).and_then(|f| f.first()).copied(),
        TrackSource::Animated(i) => clip.animated_tracks.as_ref()
            .and_then(|t| t.translations.get(i)).and_then(|f| f.get(frame.min(f.len() - 1))).copied(),
        TrackSource::Identity => None,
    }
}
fn pick_scale(clip: &AnimationClip, res: &BoneResolution, frame: usize) -> Option<f32> {
    match res.scale {
        TrackSource::Static(i) => clip.static_tracks.scales.get(i).and_then(|f| f.first()).copied(),
        TrackSource::Animated(i) => clip.animated_tracks.as_ref()
            .and_then(|t| t.scales.get(i)).and_then(|f| f.get(frame.min(f.len() - 1))).copied(),
        TrackSource::Identity => None,
    }
}

/// JMA-family file extension — picked from the animation's
/// `animation type` × `frame info type`.
#[derive(Debug, Clone, Copy)]
pub enum JmaKind {
    /// Base animation, no movement data.
    Jmm,
    /// Base + dx/dy.
    Jma,
    /// Base + dx/dy/dyaw.
    Jmt,
    /// Base + dx/dy/dz/dyaw.
    Jmz,
    /// Overlay animation.
    Jmo,
    /// Replacement animation.
    Jmr,
    /// World-relative (no movement).
    Jmw,
}

impl JmaKind {
    /// Uppercase JMA-family file extension (no leading dot).
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jmm => "JMM", Self::Jma => "JMA", Self::Jmt => "JMT", Self::Jmz => "JMZ",
            Self::Jmo => "JMO", Self::Jmr => "JMR", Self::Jmw => "JMW",
        }
    }

    /// Pick the right JMA-family kind from the per-animation metadata.
    /// Defaults to JMM when no movement data and base-type.
    pub fn from_metadata(animation_type: Option<&str>, frame_info_type: Option<&str>) -> Self {
        match animation_type.unwrap_or("base") {
            "overlay" => return Self::Jmo,
            "replacement" => return Self::Jmr,
            "world" => return Self::Jmw,
            _ => {}
        }
        match frame_info_type.unwrap_or("none") {
            "dx,dy" => Self::Jma,
            "dx,dy,dyaw" => Self::Jmt,
            "dx,dy,dz,dyaw" | "dx,dy,dz,dangle_axis" => Self::Jmz,
            _ => Self::Jmm,
        }
    }

    /// Whether this kind needs per-frame movement-data lines after
    /// the per-bone transforms. JMM/JMW/JMO/JMR don't; JMA/JMT/JMZ do.
    pub fn has_movement_data(self) -> bool {
        matches!(self, Self::Jma | Self::Jmt | Self::Jmz)
    }

    /// Number of float components written per movement-data frame
    /// (`0` when [`Self::has_movement_data`] is false).
    pub fn movement_components(self) -> usize {
        match self {
            Self::Jma => 2, // dx, dy
            Self::Jmt => 3, // dx, dy, dyaw
            Self::Jmz => 4, // dx, dy, dz, dyaw
            _ => 0,
        }
    }
}

impl Pose {
    /// Write this pose as a JMA-family text file (.JMM/.JMA/.JMT/etc).
    /// Applies the JMA-side conventions — translation `× 100` (Halo
    /// world-units → JMA centimeter convention) and quaternion
    /// **conjugate** serialization (JMA writes `(-i, -j, -k, w)`).
    ///
    /// `movement` carries per-frame root deltas in **local space** as
    /// stored in the tag. For movement-bearing kinds (JMA/JMT/JMZ)
    /// this writer composes them into **world space** at write time
    /// per Foundry commit `850d680d`: dx/dy rotate by accumulated yaw
    /// before the dyaw delta is applied. JMM/JMW/JMO/JMR ignore
    /// `movement` entirely.
    pub fn write_jma<W: std::io::Write>(
        &self,
        writer: &mut W,
        skeleton: &Skeleton,
        node_list_checksum: i32,
        kind: JmaKind,
        actor_name: &str,
        movement: Option<&MovementData>,
    ) -> std::io::Result<()> {
        // Header.
        writeln!(writer, "16392")?;
        writeln!(writer, "{}", self.frames.len())?;
        writeln!(writer, "30")?;
        writeln!(writer, "1")?;
        writeln!(writer, "{actor_name}")?;
        writeln!(writer, "{}", skeleton.len())?;
        writeln!(writer, "{node_list_checksum}")?;

        // Skeleton.
        for node in &skeleton.nodes {
            writeln!(writer, "{}", node.name)?;
            writeln!(writer, "{}", node.first_child)?;
            writeln!(writer, "{}", node.next_sibling)?;
        }

        // Frames + per-frame movement (composed local→world per Foundry).
        let mut accumulated_yaw = 0.0f32;
        for (frame_idx, frame) in self.frames.iter().enumerate() {
            for transform in frame {
                let t = transform.translation;
                // Halo world-units → JMA "centimeter" convention.
                write_floats(writer, &[t.x * 100.0, t.y * 100.0, t.z * 100.0])?;
                let q = transform.rotation;
                // JMA wants the conjugate: negate i,j,k, keep w.
                write_floats(writer, &[-q.i, -q.j, -q.k, q.w])?;
                write_floats(writer, &[transform.scale])?;
            }
            if kind.has_movement_data() {
                let local = movement
                    .and_then(|m| m.frames.get(frame_idx))
                    .copied()
                    .unwrap_or_default();
                // Rotate dx/dy from local space into world by the
                // yaw accumulated up through the previous frame
                // (Foundry's order: rotate first, then accumulate
                // this frame's dyaw).
                let cos_y = accumulated_yaw.cos();
                let sin_y = accumulated_yaw.sin();
                let world_dx = local.dx * cos_y - local.dy * sin_y;
                let world_dy = local.dx * sin_y + local.dy * cos_y;
                // JMA-side translation scale is ×100 (cm convention).
                let row: Vec<f32> = match kind {
                    JmaKind::Jma => vec![world_dx * 100.0, world_dy * 100.0],
                    JmaKind::Jmt => vec![world_dx * 100.0, world_dy * 100.0, local.dyaw],
                    JmaKind::Jmz => vec![world_dx * 100.0, world_dy * 100.0, local.dz * 100.0, local.dyaw],
                    _ => Vec::new(),
                };
                write_floats(writer, &row)?;
                accumulated_yaw += local.dyaw;
            }
        }
        writer.flush()?;
        Ok(())
    }
}

fn write_floats<W: std::io::Write>(writer: &mut W, values: &[f32]) -> std::io::Result<()> {
    for (i, v) in values.iter().enumerate() {
        let v = if *v == -0.0 { 0.0 } else { *v };
        if i + 1 < values.len() {
            write!(writer, "{:.10}\t", v)?;
        } else {
            writeln!(writer, "{:.10}", v)?;
        }
    }
    Ok(())
}

/// Quaternion normalization. Mirrors `fast_quaternion_normalize` in
/// the H3 binary — divides by the magnitude. Returns the input
/// unchanged on a zero-magnitude quat (no `1/0` blow-up; callers can
/// detect by looking at `i==j==k==w==0`).
fn normalize_quat(q: RealQuaternion) -> RealQuaternion {
    let mag2 = q.i * q.i + q.j * q.j + q.k * q.k + q.w * q.w;
    if mag2 <= 0.0 || !mag2.is_finite() {
        return q;
    }
    let inv = mag2.sqrt().recip();
    RealQuaternion { i: q.i * inv, j: q.j * inv, k: q.k * inv, w: q.w * inv }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic uncompressed_static blob with
    /// (n_rot, n_trans, n_scale) nodes, tightly packed.
    fn build_static(n_rot: u8, n_trans: u8, n_scale: u8) -> Vec<u8> {
        let rot_start = FullframeCodecHeader::SIZE;
        let trans_start = rot_start + (n_rot as usize) * 8;
        let scale_start = trans_start + (n_trans as usize) * 12;
        let total = scale_start + (n_scale as usize) * 4;

        let mut out = vec![0u8; total];
        out[0] = Codec::UncompressedStatic as u8;
        out[1] = n_rot;
        out[2] = n_trans;
        out[3] = n_scale;
        // error_value (4..8), compression_rate (8..12) left as 0.
        out[12..16].copy_from_slice(&(trans_start as u32).to_le_bytes());
        out[16..20].copy_from_slice(&(scale_start as u32).to_le_bytes());
        // strides
        out[20..24].copy_from_slice(&8u32.to_le_bytes());
        out[24..28].copy_from_slice(&12u32.to_le_bytes());
        out[28..32].copy_from_slice(&4u32.to_le_bytes());
        out
    }

    /// Build a synthetic slot-3 (8byte_quantized_rotation_only) blob
    /// with per-frame strides. Same header shape as static but with
    /// `frame_count` frames per node.
    fn build_animated_8byte(n_rot: u8, n_trans: u8, n_scale: u8, frame_count: u16) -> Vec<u8> {
        let f = frame_count as usize;
        let rot_start = FullframeCodecHeader::SIZE;
        let trans_start = rot_start + (n_rot as usize) * 8 * f;
        let scale_start = trans_start + (n_trans as usize) * 12 * f;
        let total = scale_start + (n_scale as usize) * 4 * f;

        let mut out = vec![0u8; total];
        out[0] = Codec::EightByteQuantizedRotationOnly as u8;
        out[1] = n_rot;
        out[2] = n_trans;
        out[3] = n_scale;
        out[12..16].copy_from_slice(&(trans_start as u32).to_le_bytes());
        out[16..20].copy_from_slice(&(scale_start as u32).to_le_bytes());
        out[20..24].copy_from_slice(&((8 * f) as u32).to_le_bytes());
        out[24..28].copy_from_slice(&((12 * f) as u32).to_le_bytes());
        out[28..32].copy_from_slice(&((4 * f) as u32).to_le_bytes());
        out
    }

    #[test]
    fn empty_animation_decodes() {
        let blob = build_static(0, 0, 0);
        let tracks = decode_uncompressed_static(&blob).unwrap();
        assert_eq!(tracks.frame_count, 1);
        assert!(tracks.rotations.is_empty());
        assert!(tracks.translations.is_empty());
        assert!(tracks.scales.is_empty());
    }

    #[test]
    fn one_node_identity_quaternion() {
        let mut blob = build_static(1, 0, 0);
        // Identity quat: (0, 0, 0, 1) = i16 (0, 0, 0, 32767).
        blob[32..34].copy_from_slice(&0i16.to_le_bytes());
        blob[34..36].copy_from_slice(&0i16.to_le_bytes());
        blob[36..38].copy_from_slice(&0i16.to_le_bytes());
        blob[38..40].copy_from_slice(&i16::MAX.to_le_bytes());
        let tracks = decode_uncompressed_static(&blob).unwrap();
        let q = tracks.rotations[0][0];
        assert!((q.i.abs()) < 1e-6);
        assert!((q.j.abs()) < 1e-6);
        assert!((q.k.abs()) < 1e-6);
        assert!((q.w - 1.0).abs() < 1e-6);
    }

    #[test]
    fn translation_uses_header_offset_not_implicit() {
        let mut blob = build_static(2, 1, 0);
        let trans_off = u32::from_le_bytes(blob[12..16].try_into().unwrap()) as usize;
        blob[trans_off..trans_off + 4].copy_from_slice(&1.5f32.to_le_bytes());
        blob[trans_off + 4..trans_off + 8].copy_from_slice(&(-2.0f32).to_le_bytes());
        blob[trans_off + 8..trans_off + 12].copy_from_slice(&3.25f32.to_le_bytes());
        let tracks = decode_uncompressed_static(&blob).unwrap();
        let t = tracks.translations[0][0];
        assert_eq!(t.x, 1.5);
        assert_eq!(t.y, -2.0);
        assert_eq!(t.z, 3.25);
    }

    /// Build a fullframe blob with raw 16-byte real_quaternions
    /// (slots 2 / 8). Same shape as the 8byte builder but with
    /// 16-byte rotation strides.
    fn build_animated_raw_quat(n_rot: u8, frame_count: u16) -> Vec<u8> {
        let f = frame_count as usize;
        let rot_start = FullframeCodecHeader::SIZE;
        let trans_start = rot_start + (n_rot as usize) * 16 * f;
        let scale_start = trans_start;
        let total = scale_start;
        let mut out = vec![0u8; total];
        out[0] = Codec::BlendScreen as u8;
        out[1] = n_rot;
        out[12..16].copy_from_slice(&(trans_start as u32).to_le_bytes());
        out[16..20].copy_from_slice(&(scale_start as u32).to_le_bytes());
        out[20..24].copy_from_slice(&((16 * f) as u32).to_le_bytes());
        out[24..28].copy_from_slice(&0u32.to_le_bytes());
        out[28..32].copy_from_slice(&0u32.to_le_bytes());
        out
    }

    #[test]
    fn animated_raw_quat_per_frame() {
        // 1 rotated node, 2 frames, raw f32 quaternions.
        let mut blob = build_animated_raw_quat(1, 2);
        // Frame 0: identity.
        blob[32..36].copy_from_slice(&0.0f32.to_le_bytes());
        blob[36..40].copy_from_slice(&0.0f32.to_le_bytes());
        blob[40..44].copy_from_slice(&0.0f32.to_le_bytes());
        blob[44..48].copy_from_slice(&1.0f32.to_le_bytes());
        // Frame 1: (0.5, 0.5, 0.5, 0.5) — already unit length.
        blob[48..52].copy_from_slice(&0.5f32.to_le_bytes());
        blob[52..56].copy_from_slice(&0.5f32.to_le_bytes());
        blob[56..60].copy_from_slice(&0.5f32.to_le_bytes());
        blob[60..64].copy_from_slice(&0.5f32.to_le_bytes());
        let tracks = decode_fullframe(&blob, Codec::BlendScreen, 2, /*quat_8byte=*/false).unwrap();
        assert_eq!(tracks.codec, Codec::BlendScreen);
        assert_eq!(tracks.frame_count, 2);
        let f0 = tracks.rotations[0][0];
        assert!((f0.w - 1.0).abs() < 1e-6);
        let f1 = tracks.rotations[0][1];
        assert!((f1.i - 0.5).abs() < 1e-6);
        assert!((f1.j - 0.5).abs() < 1e-6);
        assert!((f1.k - 0.5).abs() < 1e-6);
        assert!((f1.w - 0.5).abs() < 1e-6);
    }

    /// Build a synthetic keyframe blob with the given per-component
    /// node packs. Each `pack = (time_offset, key_count)` from the
    /// caller's perspective. Time entries (u8) and quaternion payloads
    /// are written tightly packed in the order rotation/translation/scale.
    fn build_keyframe_byte(
        rot_packs: &[(u32, u32)],
        rot_keys: &[(u8, [i16; 4])],     // (time, quat_components)
        trans_packs: &[(u32, u32)],
        trans_keys: &[(u8, [f32; 3])],
        scale_packs: &[(u32, u32)],
        scale_keys: &[(u8, f32)],
    ) -> Vec<u8> {
        let n_rot = rot_packs.len() as u8;
        let n_trans = trans_packs.len() as u8;
        let n_scale = scale_packs.len() as u8;
        let packed_count = (n_rot as usize) + (n_trans as usize) + (n_scale as usize);

        // Layout: header (48) | packed_data (packed_count*4)
        //       | rot times (rot_keys.len()*1) | trans times | scale times
        //       | rot payload (rot_keys.len()*8) | trans payload (n*12) | scale payload (n*4)
        let packed_start = 48;
        let rot_time_start = packed_start + packed_count * 4;
        let trans_time_start = rot_time_start + rot_keys.len();
        let scale_time_start = trans_time_start + trans_keys.len();
        let rot_payload_start = scale_time_start + scale_keys.len();
        let trans_payload_start = rot_payload_start + rot_keys.len() * 8;
        let scale_payload_start = trans_payload_start + trans_keys.len() * 12;
        let total = scale_payload_start + scale_keys.len() * 4;

        let mut out = vec![0u8; total];
        out[0] = Codec::ByteKeyframeLightlyQuantized as u8;
        out[1] = n_rot;
        out[2] = n_trans;
        out[3] = n_scale;
        // bytes 4..12 (error_value, compression_rate) left zero.
        // bytes 12..20 (translation_offset, scale_offset) — base
        // header fields not used by the keyframe decoder. Leave zero.
        out[20..24].copy_from_slice(&(rot_time_start as u32).to_le_bytes());
        out[24..28].copy_from_slice(&(trans_time_start as u32).to_le_bytes());
        out[28..32].copy_from_slice(&(scale_time_start as u32).to_le_bytes());
        out[32..36].copy_from_slice(&(rot_payload_start as u32).to_le_bytes());
        out[36..40].copy_from_slice(&(trans_payload_start as u32).to_le_bytes());
        out[40..44].copy_from_slice(&(scale_payload_start as u32).to_le_bytes());

        let mut idx = 0;
        for &(t, c) in rot_packs.iter().chain(trans_packs.iter()).chain(scale_packs.iter()) {
            let pd = (t << 12) | (c & 0xFFF);
            out[packed_start + idx * 4..packed_start + idx * 4 + 4]
                .copy_from_slice(&pd.to_le_bytes());
            idx += 1;
        }

        for (i, (t, _)) in rot_keys.iter().enumerate() { out[rot_time_start + i] = *t; }
        for (i, (t, _)) in trans_keys.iter().enumerate() { out[trans_time_start + i] = *t; }
        for (i, (t, _)) in scale_keys.iter().enumerate() { out[scale_time_start + i] = *t; }

        for (i, (_, q)) in rot_keys.iter().enumerate() {
            let off = rot_payload_start + i * 8;
            out[off..off + 2].copy_from_slice(&q[0].to_le_bytes());
            out[off + 2..off + 4].copy_from_slice(&q[1].to_le_bytes());
            out[off + 4..off + 6].copy_from_slice(&q[2].to_le_bytes());
            out[off + 6..off + 8].copy_from_slice(&q[3].to_le_bytes());
        }
        for (i, (_, p)) in trans_keys.iter().enumerate() {
            let off = trans_payload_start + i * 12;
            out[off..off + 4].copy_from_slice(&p[0].to_le_bytes());
            out[off + 4..off + 8].copy_from_slice(&p[1].to_le_bytes());
            out[off + 8..off + 12].copy_from_slice(&p[2].to_le_bytes());
        }
        for (i, (_, s)) in scale_keys.iter().enumerate() {
            let off = scale_payload_start + i * 4;
            out[off..off + 4].copy_from_slice(&s.to_le_bytes());
        }
        out
    }

    #[test]
    fn keyframe_single_key_constant() {
        // One rotated node with a single key — value held across all
        // frames regardless of frame_count.
        let blob = build_keyframe_byte(
            &[(0, 1)],
            &[(0, [0, 0, 0, i16::MAX])], // identity quat at time 0
            &[], &[], &[], &[],
        );
        let tracks = decode_keyframe(&blob, Codec::ByteKeyframeLightlyQuantized, 5, 1).unwrap();
        assert_eq!(tracks.rotations.len(), 1);
        assert_eq!(tracks.rotations[0].len(), 5);
        for q in &tracks.rotations[0] {
            assert!((q.w - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn keyframe_two_keys_lerp() {
        // Translation node with keys at frames 0 and 4: (0,0,0) → (4,0,0).
        // Frame 2 should land halfway: (2, 0, 0).
        let blob = build_keyframe_byte(
            &[],
            &[],
            &[(0, 2)],
            &[(0, [0.0, 0.0, 0.0]), (4, [4.0, 0.0, 0.0])],
            &[], &[],
        );
        let tracks = decode_keyframe(&blob, Codec::ByteKeyframeLightlyQuantized, 5, 1).unwrap();
        let t = &tracks.translations[0];
        assert!((t[0].x - 0.0).abs() < 1e-6);
        assert!((t[2].x - 2.0).abs() < 1e-6);
        assert!((t[4].x - 4.0).abs() < 1e-6);
    }

    #[test]
    fn keyframe_packed_data_skips_to_correct_node() {
        // Two rotated nodes; node 0 has 1 key starting at time_offset 0,
        // node 1 has 1 key starting at time_offset 1. Verify node 1
        // reads its OWN payload (not node 0's).
        let blob = build_keyframe_byte(
            &[(0, 1), (1, 1)],
            &[
                (0, [0, 0, 0, i16::MAX]),     // node 0: identity
                (0, [i16::MAX, 0, 0, 0]),     // node 1: (1, 0, 0, 0)
            ],
            &[], &[], &[], &[],
        );
        let tracks = decode_keyframe(&blob, Codec::ByteKeyframeLightlyQuantized, 1, 1).unwrap();
        assert!((tracks.rotations[0][0].w - 1.0).abs() < 1e-6);
        assert!((tracks.rotations[1][0].i - 1.0).abs() < 1e-6);
    }

    #[test]
    fn keyframe_clamp_past_last_key() {
        // Single translation node, keys at time 0 and 2. Frame 4 (past
        // last key) should clamp to the last key's value, not extrapolate.
        let blob = build_keyframe_byte(
            &[],
            &[],
            &[(0, 2)],
            &[(0, [0.0, 0.0, 0.0]), (2, [2.0, 0.0, 0.0])],
            &[], &[],
        );
        let tracks = decode_keyframe(&blob, Codec::ByteKeyframeLightlyQuantized, 5, 1).unwrap();
        assert!((tracks.translations[0][3].x - 2.0).abs() < 1e-6);
        assert!((tracks.translations[0][4].x - 2.0).abs() < 1e-6);
    }

    #[test]
    fn revised_quat_decompresses_unit_length() {
        // Encode an identity quaternion with `missing` at slot 3 (w):
        // component_index = 3 means v5_low=1 AND v4_low=1.
        // For identity, i=j=k=0 (encoded zeros), missing=w=1.
        // With component_index=3, the layout maps:
        //   output[(3+1)&3=0] = i = 0
        //   output[(3+2)&3=1] = j = 0
        //   output[(3+3)&3=2] = k = 0
        //   output[3] = missing = 1
        // RealQuaternion fields {i,j,k,w} = {output[0..3]}.
        // v3 = 0, v4 has bit 0 set (=1), v5 has bit 0 set (=1)
        let q = decompress_revised_quat(0, 1, 1);
        assert!(q.i.abs() < 1e-5, "i={}", q.i);
        assert!(q.j.abs() < 1e-5, "j={}", q.j);
        assert!(q.k.abs() < 1e-5, "k={}", q.k);
        assert!((q.w - 1.0).abs() < 1e-5, "w={}", q.w);
    }

    #[test]
    fn revised_quat_sign_bit_negates_missing() {
        // v3 bit 0 set should flip the sign of the reconstructed (missing) component.
        let q_pos = decompress_revised_quat(0, 1, 1);
        let q_neg = decompress_revised_quat(1, 1, 1);
        assert!((q_pos.w - 1.0).abs() < 1e-5);
        assert!((q_neg.w + 1.0).abs() < 1e-5);
    }

    #[test]
    fn nlerp_short_arc_picks_shorter_path() {
        let a = identity_quat();
        // -a should be treated as +a (same orientation, opposite sign).
        let neg_a = RealQuaternion { i: 0.0, j: 0.0, k: 0.0, w: -1.0 };
        let mid = nlerp_short_arc(&a, &neg_a, 0.5);
        // After short-arc flip, mid should be ≈ identity (not zero).
        assert!((mid.w - 1.0).abs() < 1e-6 || (mid.w + 1.0).abs() < 1e-6);
    }

    #[test]
    fn animated_8byte_per_frame_quaternions() {
        // 1 rotated node, 3 frames, distinct quats per frame.
        let mut blob = build_animated_8byte(1, 0, 0, 3);
        // frame 0: (0, 0, 0, 32767) = identity
        // frame 1: (32767, 0, 0, 0)
        // frame 2: (0, 32767, 0, 0)
        let writes = [
            (32, [0i16, 0, 0, i16::MAX]),
            (40, [i16::MAX, 0, 0, 0]),
            (48, [0, i16::MAX, 0, 0]),
        ];
        for (off, vals) in writes {
            for (i, v) in vals.iter().enumerate() {
                blob[off + i * 2..off + i * 2 + 2].copy_from_slice(&v.to_le_bytes());
            }
        }
        let tracks = decode_fullframe(&blob, Codec::EightByteQuantizedRotationOnly, 3, /*quat_8byte=*/true).unwrap();
        assert_eq!(tracks.codec, Codec::EightByteQuantizedRotationOnly);
        assert_eq!(tracks.frame_count, 3);
        assert_eq!(tracks.rotations.len(), 1);
        assert_eq!(tracks.rotations[0].len(), 3);
        // Frame 0 = identity (w ≈ 1).
        assert!((tracks.rotations[0][0].w - 1.0).abs() < 1e-6);
        // Frame 1 = (1, 0, 0, 0) after normalize.
        assert!((tracks.rotations[0][1].i - 1.0).abs() < 1e-6);
        // Frame 2 = (0, 1, 0, 0) after normalize.
        assert!((tracks.rotations[0][2].j - 1.0).abs() < 1e-6);
    }

    #[test]
    fn truncated_blob_errors() {
        let blob = vec![0u8; 10]; // less than header
        let err = decode_uncompressed_static(&blob).unwrap_err();
        assert!(matches!(err, AnimationError::TruncatedHeader { .. }));
    }

    #[test]
    fn quat_no_normalize_on_zero() {
        let q = RealQuaternion { i: 0.0, j: 0.0, k: 0.0, w: 0.0 };
        let n = normalize_quat(q);
        assert_eq!(n.i, 0.0);
        assert_eq!(n.w, 0.0);
    }

    #[test]
    fn quat_normalize_unit_magnitude() {
        let q = RealQuaternion { i: 2.0, j: 0.0, k: 0.0, w: 0.0 };
        let n = normalize_quat(q);
        assert!((n.i - 1.0).abs() < 1e-6);
    }
}
