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

pub mod codec;
pub mod jma;
pub mod pose;

pub use codec::Codec;
pub use jma::JmaKind;
pub use pose::{NodeTransform, Pose, Skeleton, SkeletonNode};

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
            let name = anim.read_string_id("name");

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
            let animation_type = metadata.read_enum_name("animation type")
                .or_else(|| metadata.read_enum_name("type"));
            let frame_info_type = metadata.read_enum_name("frame info type");
            let frame_count = metadata.read_int_any("frame count").unwrap_or(0) as i16;
            let node_count = metadata.read_int_any("node count").unwrap_or(0) as i8;
            let node_list_checksum = metadata.read_int_any("node list checksum").unwrap_or(0) as i32;
            let resource_group = metadata.read_int_any("resource_group").unwrap_or(-1) as i16;
            let resource_group_member = metadata.read_int_any("resource_group_member").unwrap_or(-1) as i16;

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
                    checksum = metadata.read_int_any("production checksum").map(|v| v as i32);
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

    let checksum = member.read_int_any("animation_checksum").map(|v| v as i32);
    let codec_frame_count = member.read_int_any("frame count").map(|v| v as i16);
    let movement_type = member.read_enum_name("movement_data_type");
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
        if let Some(v) = s.read_int_any(&name) {
            fields.push((name, v));
        }
    }
    Some(PackedDataSizes { fields })
}


//================================================================================
// Decoded data types
//
// Produced by the codec module, consumed by the pose composer and the
// JMA writer. Public API surface, so they live here at the parent
// module rather than inside `codec`.
//================================================================================

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
